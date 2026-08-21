use std::{error::Error, fs, io, sync::Arc, time::Duration};

use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_core::{
    AchievementRate, ChartConstant, ChartGeneration, ChartKey, Difficulty, QqId, ScoreSource,
    SongIdNamespace, SourceSongId,
};
use maimai_providers::{
    DivingFishClient, DivingFishCredentials, DivingFishScoreClient, LxnsOAuthClient, OAuthConfig,
    lxns_score::LxnsScoreEndpoint,
};
use maimai_storage::{
    AuthorizationClaimResult, NewOAuthAuthorization, NewOAuthToken, PlayerProfile, PlayerRecord,
    StateStore,
};
use secrecy::SecretString;
use serde_json::{Value, json};
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};
use url::Url;

use crate::{
    oauth::OAuthService,
    scores::{Lookup, RatingMode, SongFilter},
};

use super::{
    B50Mode, B50Request, PlayerScoreService, PlayerScoreServiceErrorCode, ScoreQuery,
    SongScoresRequest,
};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

struct Fixture {
    _temp: TempDir,
    store: StateStore,
    catalog: Arc<CatalogStore>,
}

#[tokio::test]
async fn diving_fish_is_default_and_username_is_always_diving_fish() -> TestResult {
    let fixture = fixture().await?;
    fixture
        .store
        .set_score_source_preference(&QqId::new("10001")?, ScoreSource::Lxns)
        .await?;
    let response = df_b50();
    let (df_url, mut requests) = mock_server(vec![response.clone(), response]).await?;
    let service = PlayerScoreService::diving_fish_only(
        fixture.store.clone(),
        Arc::clone(&fixture.catalog),
        DivingFishScoreClient::new(DivingFishClient::with_base_urls(
            df_url.join("api/")?.as_str(),
            df_url.join("covers/")?.as_str(),
        )?),
    );

    let first = service
        .b50(B50Request {
            query: ScoreQuery::new(qq_lookup()?, 100),
            mode: B50Mode::Provider,
        })
        .await?;
    assert_eq!(first.source, ScoreSource::DivingFish);

    let mut query = ScoreQuery::new(
        Lookup::Username(maimai_core::PlayerUsername::new("tester")?),
        100,
    );
    query.source = Some(ScoreSource::Local);
    let second = service
        .b50(B50Request {
            query,
            mode: B50Mode::Provider,
        })
        .await?;
    assert_eq!(second.source, ScoreSource::DivingFish);
    let captured = receive(&mut requests, 2).await?;
    assert_eq!(request_json(&captured[0])?, json!({"qq":"10001","b50":"1"}));
    assert_eq!(
        request_json(&captured[1])?,
        json!({"username":"tester","b50":"1"})
    );

    let mut unavailable = ScoreQuery::new(qq_lookup()?, 100);
    unavailable.source = Some(ScoreSource::Lxns);
    let error = service
        .b50(B50Request {
            query: unavailable,
            mode: B50Mode::Provider,
        })
        .await
        .err()
        .ok_or_else(|| io::Error::other("unconfigured LXNS access was accepted"))?;
    assert_eq!(error.code(), PlayerScoreServiceErrorCode::SourceUnavailable);
    Ok(())
}

#[tokio::test]
async fn diving_fish_provider_b50_populates_shared_cache_and_other_sources_do_not_overwrite()
-> TestResult {
    let fixture = fixture().await?;
    seed_local(&fixture.store).await?;
    let (df_url, _) = mock_server(vec![
        df_b50(),
        MockResponse {
            status: 200,
            body: json!({"nickname":"Computed","rating":280,"records":[{
                "song_id":383,"title":"Link(CoF)","type":"SD","level":"13",
                "level_index":3,"ds":"13.0","achievements":"100.0","ra":280
            }]})
            .to_string(),
        },
    ])
    .await?;
    let (oauth_url, _) = mock_server(Vec::new()).await?;
    let service = service(&fixture, &df_url, &oauth_url, &oauth_url)?;
    let fetched_at = OffsetDateTime::UNIX_EPOCH + time::Duration::days(10);
    let mut query = ScoreQuery::new(qq_lookup()?, fetched_at.unix_timestamp());
    query.source = Some(ScoreSource::DivingFish);
    service
        .b50(B50Request {
            query,
            mode: B50Mode::Provider,
        })
        .await?;
    let mut computed = ScoreQuery::new(qq_lookup()?, fetched_at.unix_timestamp() + 2);
    computed.source = Some(ScoreSource::DivingFish);
    computed.diving_fish_credentials = Some(
        DivingFishCredentials::new().with_developer_token(SecretString::from("developer-token")),
    );
    service
        .b50(B50Request {
            query: computed,
            mode: B50Mode::Computed(RatingMode::Actual),
        })
        .await?;
    let qq = QqId::new("10001")?;
    let cached = service
        .cached_diving_fish_b50(&qq, fetched_at)
        .await?
        .ok_or_else(|| io::Error::other("shared B50 cache missing"))?;
    assert!(cached.fit_index.available());
    assert_eq!(cached.total_count(), 2);
    assert_eq!(cached.player.rating, Some(600));

    let mut local = ScoreQuery::new(qq_lookup()?, fetched_at.unix_timestamp() + 1);
    local.source = Some(ScoreSource::Local);
    service
        .b50(B50Request {
            query: local,
            mode: B50Mode::Provider,
        })
        .await?;
    assert_eq!(
        service
            .cached_diving_fish_b50(&qq, OffsetDateTime::UNIX_EPOCH)
            .await?
            .ok_or_else(|| io::Error::other("DF cache was overwritten"))?
            .player
            .rating,
        Some(600)
    );
    Ok(())
}

#[tokio::test]
async fn b50_cache_write_failure_is_best_effort_and_does_not_fail_provider_query() -> TestResult {
    let fixture = fixture().await?;
    let pool = sqlx::SqlitePool::connect(&format!(
        "sqlite://{}",
        fixture._temp.path().join("state.db").display()
    ))
    .await?;
    sqlx::query(
        r#"CREATE TRIGGER reject_shared_b50 BEFORE INSERT ON player_b50_cache
           BEGIN SELECT RAISE(FAIL, 'fixture failure'); END"#,
    )
    .execute(&pool)
    .await?;
    pool.close().await;
    let (df_url, _) = mock_server(vec![df_b50()]).await?;
    let (oauth_url, _) = mock_server(Vec::new()).await?;
    let service = service(&fixture, &df_url, &oauth_url, &oauth_url)?;
    let result = service
        .b50(B50Request {
            query: ScoreQuery::new(qq_lookup()?, 100),
            mode: B50Mode::Provider,
        })
        .await?;
    assert_eq!(result.player.rating, Some(600));
    assert!(
        service
            .cached_diving_fish_b50(&QqId::new("10001")?, OffsetDateTime::UNIX_EPOCH)
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn local_preference_does_not_call_network_and_missing_local_does_not_fallback() -> TestResult
{
    let context = fixture().await?;
    seed_local(&context.store).await?;
    context
        .store
        .set_score_source_preference(&QqId::new("10001")?, ScoreSource::Local)
        .await?;
    let (df_url, mut requests) = mock_server(vec![df_b50()]).await?;
    let (oauth_url, _) = mock_server(Vec::new()).await?;
    let score_service = service(&context, &df_url, &oauth_url, &oauth_url)?;
    let result = score_service
        .b50(B50Request {
            query: ScoreQuery::new(qq_lookup()?, 100),
            mode: B50Mode::Computed(RatingMode::Actual),
        })
        .await?;
    assert_eq!(result.source, ScoreSource::Local);
    assert!(requests.try_recv().is_err());

    let empty = fixture().await?;
    let score_service = service(&empty, &df_url, &oauth_url, &oauth_url)?;
    let mut query = ScoreQuery::new(qq_lookup()?, 100);
    query.source = Some(ScoreSource::Local);
    let error = score_service
        .records(query)
        .await
        .err()
        .ok_or_else(|| io::Error::other("expected missing local error"))?;
    assert_eq!(error.code(), PlayerScoreServiceErrorCode::SourceUnavailable);
    assert!(requests.try_recv().is_err());
    Ok(())
}

#[tokio::test]
async fn provider_failure_preserves_existing_local_records_and_redacts_error() -> TestResult {
    let fixture = fixture().await?;
    seed_local(&fixture.store).await?;
    let token = "DEVELOPER_SECRET_SENTINEL";
    let qq = "10001";
    let (df_url, _) = mock_server(vec![MockResponse {
        status: 500,
        body: json!({"token":token,"qq":qq}).to_string(),
    }])
    .await?;
    let (oauth_url, _) = mock_server(Vec::new()).await?;
    let service = service(&fixture, &df_url, &oauth_url, &oauth_url)?;
    let mut missing_credentials = ScoreQuery::new(qq_lookup()?, 100);
    missing_credentials.source = Some(ScoreSource::DivingFish);
    let auth_error = service
        .records(missing_credentials)
        .await
        .err()
        .ok_or_else(|| io::Error::other("expected auth error"))?;
    assert_eq!(auth_error.code(), PlayerScoreServiceErrorCode::AuthRequired);
    let mut query = ScoreQuery::new(qq_lookup()?, 100);
    query.source = Some(ScoreSource::DivingFish);
    query.diving_fish_credentials =
        Some(DivingFishCredentials::new().with_developer_token(SecretString::from(token)));
    let error = service
        .records(query)
        .await
        .err()
        .ok_or_else(|| io::Error::other("expected provider error"))?;
    assert_eq!(error.code(), PlayerScoreServiceErrorCode::Provider);
    assert_eq!(error.status(), Some(500));
    let body = error
        .body()
        .ok_or_else(|| io::Error::other("missing body"))?;
    assert!(!body.contains(token) && !body.contains(qq));

    let mut local = ScoreQuery::new(qq_lookup()?, 100);
    local.source = Some(ScoreSource::Local);
    assert_eq!(service.records(local).await?.records.len(), 1);
    assert_eq!(
        fixture
            .store
            .records_for_player(&QqId::new("10001")?)
            .await?[0]
            .chart
            .generation(),
        ChartGeneration::Deluxe
    );
    Ok(())
}

#[tokio::test]
async fn successful_full_provider_snapshot_atomically_replaces_cache_and_returns_raw() -> TestResult
{
    let fixture = fixture().await?;
    seed_local(&fixture.store).await?;
    fixture
        .store
        .set_diving_fish_developer_token(
            &SecretString::from("developer-token"),
            OffsetDateTime::UNIX_EPOCH,
        )
        .await?;
    let (df_url, _) = mock_server(vec![MockResponse {
        status: 200,
        body: json!({
            "nickname":"Fresh DF","rating":280,
            "records":[{"song_id":383,"title":"Link(CoF)","type":"SD",
                "level":"13","level_index":3,"ds":"13.0",
                "achievements":"100.0","ra":280}]
        })
        .to_string(),
    }])
    .await?;
    let (oauth_url, _) = mock_server(Vec::new()).await?;
    let service = service(&fixture, &df_url, &oauth_url, &oauth_url)?;
    let mut query = ScoreQuery::new(qq_lookup()?, 100);
    query.source = Some(ScoreSource::DivingFish);
    let (result, raw, _) = service
        .records_with_evidence(query, true)
        .await?
        .into_parts();
    assert_eq!(result.records.len(), 1);
    assert_eq!(
        raw.ok_or_else(|| io::Error::other("missing raw evidence"))?
            .into_value()?["nickname"],
        "Fresh DF"
    );
    let cached = fixture
        .store
        .records_for_player(&QqId::new("10001")?)
        .await?;
    assert_eq!(cached.len(), 1);
    assert_eq!(cached[0].chart.generation(), ChartGeneration::Standard);
    assert_eq!(
        fixture
            .store
            .profile(&QqId::new("10001")?)
            .await?
            .and_then(|profile| profile.nickname),
        Some("Fresh DF".to_owned())
    );
    let qq = QqId::new("10001")?;
    let cached = service
        .cached_full_scores(&qq, ScoreSource::DivingFish, OffsetDateTime::UNIX_EPOCH)
        .await?
        .ok_or_else(|| io::Error::other("full record cache missing"))?;
    assert_eq!(cached.source, ScoreSource::DivingFish);
    assert_eq!(cached.records.len(), 1);
    assert!(
        service
            .cached_full_scores(&qq, ScoreSource::Lxns, OffsetDateTime::UNIX_EPOCH)
            .await?
            .is_none()
    );
    assert!(
        service
            .cached_full_scores(
                &qq,
                ScoreSource::DivingFish,
                OffsetDateTime::from_unix_timestamp(101)?,
            )
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn lxns_fresh_token_uses_song_bests_and_keeps_dual_charts() -> TestResult {
    let fixture = fixture().await?;
    seed_oauth_token(&fixture.store, "fresh-token", "refresh-token", 1_000).await?;
    let (lxns_url, mut requests) = mock_server(vec![MockResponse {
        status: 200,
        body: json!({"success":true,"data":{
            "standard":[lxns_score("standard")],
            "dx":[lxns_score("dx")]
        }})
        .to_string(),
    }])
    .await?;
    let (df_url, _) = mock_server(Vec::new()).await?;
    let (oauth_url, _) = mock_server(Vec::new()).await?;
    let service = service(&fixture, &df_url, &lxns_url, &oauth_url)?;
    let mut query = ScoreQuery::new(qq_lookup()?, 100);
    query.source = Some(ScoreSource::Lxns);
    let result = service
        .song_scores(SongScoresRequest {
            query,
            filter: SongFilter::new(df_id(10_383)),
        })
        .await?;
    assert_eq!(result.records.len(), 2);
    assert_eq!(
        result.records[0].key.generation(),
        ChartGeneration::Standard
    );
    assert_eq!(result.records[1].key.generation(), ChartGeneration::Deluxe);
    let request = receive(&mut requests, 1).await?.remove(0);
    assert!(request.starts_with("GET /api/v0/user/maimai/player/bests?song_id=383"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer fresh-token")
    );
    assert!(
        service
            .cached_full_scores(
                &QqId::new("10001")?,
                ScoreSource::Lxns,
                OffsetDateTime::UNIX_EPOCH,
            )
            .await?
            .is_none()
    );
    Ok(())
}

#[tokio::test]
async fn lxns_401_refreshes_generation_and_retries_exactly_once() -> TestResult {
    let fixture = fixture().await?;
    seed_oauth_token(&fixture.store, "old-token", "old-refresh", 1_000).await?;
    let (lxns_url, mut lxns_requests) = mock_server(vec![
        MockResponse {
            status: 401,
            body: json!({"message":"old-token rejected"}).to_string(),
        },
        MockResponse {
            status: 200,
            body: json!({"success":true,"data":{
                "standard":[lxns_score("standard")],"dx":[]
            }})
            .to_string(),
        },
    ])
    .await?;
    let (oauth_url, mut oauth_requests) = mock_server(vec![MockResponse {
        status: 200,
        body: json!({
            "access_token":"new-token","refresh_token":"rotated-refresh",
            "token_type":"Bearer","expires_in":3600
        })
        .to_string(),
    }])
    .await?;
    let (df_url, _) = mock_server(Vec::new()).await?;
    let service = service(&fixture, &df_url, &lxns_url, &oauth_url)?;
    let mut query = ScoreQuery::new(qq_lookup()?, 100);
    query.source = Some(ScoreSource::Lxns);
    let result = service
        .b50(B50Request {
            query,
            mode: B50Mode::Provider,
        })
        .await?;
    assert_eq!(result.source, ScoreSource::Lxns);
    let requests = receive(&mut lxns_requests, 2).await?;
    assert!(
        requests[0]
            .to_ascii_lowercase()
            .contains("bearer old-token")
    );
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("bearer new-token")
    );
    assert_eq!(receive(&mut oauth_requests, 1).await?.len(), 1);
    let stored = fixture
        .store
        .oauth_token("10001")
        .await?
        .ok_or_else(|| io::Error::other("token disappeared"))?;
    assert_eq!(stored.generation, 2);
    Ok(())
}

#[tokio::test]
async fn lxns_player_profile_refreshes_once_and_keeps_typed_collections() -> TestResult {
    let fixture = fixture().await?;
    seed_oauth_token(&fixture.store, "old-token", "refresh-token", 1_000).await?;
    let (lxns_url, mut lxns_requests) = mock_server(vec![
        MockResponse {
            status: 401,
            body: json!({"message":"expired"}).to_string(),
        },
        MockResponse {
            status: 200,
            body: json!({"success":true,"data":{
                "name":"LXNS Fresh","rating":15602,"course_rank":7,"class_rank":4,
                "star":9,"trophy":{"id":258174,"name":"落雪称号","color":"rainbow"},
                "icon":{"id":400405},"name_plate":{"id":550101},
                "frame":{"id":400401},"upload_time":"2026-08-19T01:00:00Z"
            }})
            .to_string(),
        },
    ])
    .await?;
    let (oauth_url, mut oauth_requests) = mock_server(vec![MockResponse {
        status: 200,
        body: json!({
            "access_token":"new-token","refresh_token":"rotated-refresh",
            "token_type":"Bearer","expires_in":3600
        })
        .to_string(),
    }])
    .await?;
    let (df_url, _) = mock_server(Vec::new()).await?;
    let service = service(&fixture, &df_url, &lxns_url, &oauth_url)?;

    let profile = service
        .lxns_player_profile(&QqId::new("10001")?, 100)
        .await?;
    assert_eq!(profile.nickname.as_deref(), Some("LXNS Fresh"));
    assert_eq!(profile.rating, Some(15_602));
    assert_eq!(profile.course_rank, Some(7));
    assert_eq!(profile.class_rank, Some(4));
    assert_eq!(profile.star, Some(9));
    assert_eq!(profile.trophy_id, Some(258_174));
    assert_eq!(profile.trophy_name.as_deref(), Some("落雪称号"));
    assert_eq!(
        profile.trophy_color,
        Some(super::PlayerPresentationTrophyColor::Rainbow)
    );
    assert_eq!(profile.icon_id, Some(400_405));
    assert_eq!(profile.plate_id, Some(550_101));
    assert_eq!(profile.frame_id, Some(400_401));
    assert_eq!(receive(&mut lxns_requests, 2).await?.len(), 2);
    assert_eq!(receive(&mut oauth_requests, 1).await?.len(), 1);
    Ok(())
}

fn service(
    fixture: &Fixture,
    df_url: &Url,
    lxns_url: &Url,
    oauth_url: &Url,
) -> Result<PlayerScoreService, Box<dyn Error + Send + Sync>> {
    let df_api = df_url.join("api/")?;
    let df_covers = df_url.join("covers/")?;
    let diving_fish = DivingFishScoreClient::new(DivingFishClient::with_base_urls(
        df_api.as_str(),
        df_covers.as_str(),
    )?);
    let oauth_config = OAuthConfig::new(
        "client-id",
        None,
        None,
        oauth_url.join("oauth/authorize")?,
        oauth_url.join("oauth/token")?,
        vec!["read_player".to_owned()],
    )?;
    let oauth = OAuthService::new(
        fixture.store.clone(),
        LxnsOAuthClient::new(oauth_config)?.with_timeout(Duration::from_secs(1))?,
    );
    Ok(PlayerScoreService::with_lxns(
        fixture.store.clone(),
        Arc::clone(&fixture.catalog),
        diving_fish,
        oauth,
        LxnsScoreEndpoint::new(lxns_url.join("api/v0/")?, Duration::from_secs(1))?,
    ))
}

async fn fixture() -> Result<Fixture, Box<dyn Error + Send + Sync>> {
    let temp = TempDir::new()?;
    write_catalog(&temp)?;
    let store = StateStore::open(temp.path().join("state.db")).await?;
    let catalog = Arc::new(CatalogStore::load(CatalogFiles::from_data_dir(temp.path())).await?);
    Ok(Fixture {
        _temp: temp,
        store,
        catalog,
    })
}

async fn seed_local(store: &StateStore) -> TestResult {
    let qq = QqId::new("10001")?;
    store
        .upsert_profile(&PlayerProfile {
            qq: qq.clone(),
            nickname: Some("Local Tester".to_owned()),
            player_rating: Some(300),
            player_old_rating: None,
            player_new_rating: None,
            score_source: Some(ScoreSource::Local),
            source_detail: None,
            raw: None,
            updated_at: "2026-08-18T00:00:00Z".to_owned(),
        })
        .await?;
    store
        .upsert_record(&PlayerRecord {
            qq,
            chart: ChartKey::new(lxns_id(383), ChartGeneration::Deluxe, Difficulty::Master)?,
            title: "stale title".to_owned(),
            level: Some("13+".to_owned()),
            level_label: Some("Master".to_owned()),
            ds: Some(ChartConstant::from_decimal_str("13.8")?),
            achievements: Some(AchievementRate::from_decimal_str("100.0")?.into()),
            dx_score: Some(1_000),
            fc: None,
            fs: None,
            rate: Some("sss".to_owned()),
            ra: Some(298),
            version: Some("PRiSM".to_owned()),
            is_new: false,
            score_source: ScoreSource::DivingFish,
            source_detail: None,
            raw: None,
            payload: json!({}),
            updated_at: "2026-08-18T00:00:00Z".to_owned(),
        })
        .await?;
    Ok(())
}

async fn seed_oauth_token(
    store: &StateStore,
    access_token: &str,
    refresh_token: &str,
    expires_at: i64,
) -> TestResult {
    let authorization = store
        .save_oauth_authorization(&NewOAuthAuthorization::new(
            "10001".to_owned(),
            SecretString::from("state"),
            SecretString::from("verifier"),
            None,
            0,
            1_000,
        ))
        .await?;
    let claim = store
        .claim_oauth_authorization("10001", None, None, None, 1)
        .await?;
    let generation = match claim {
        AuthorizationClaimResult::Claimed(claim) => claim.authorization.generation,
        _ => return Err(io::Error::other("authorization was not claimed").into()),
    };
    assert_eq!(generation, authorization.generation);
    store
        .commit_oauth_authorization(
            "10001",
            generation,
            &NewOAuthToken::new(
                SecretString::from(access_token),
                SecretString::from(refresh_token),
                "Bearer".to_owned(),
                Some("read_player".to_owned()),
                "client-id".to_owned(),
                Some(expires_at),
            ),
            1,
        )
        .await?
        .ok_or_else(|| io::Error::other("token commit failed"))?;
    Ok(())
}

fn write_catalog(temp: &TempDir) -> Result<(), io::Error> {
    let files = [
        (
            "lxns_song_list.json",
            json!({
                "songs":[{"id":383,"title":"Link","artist":"A","genre":"maimai",
                    "bpm":150,"version":25000,"difficulties":{
                        "standard":[{"difficulty":3,"level":"13","level_value":13.0,"notes":{}}],
                        "dx":[{"difficulty":3,"level":"13+","level_value":13.8,"notes":{}}]
                    }}],
                "genres":[],"versions":[{"title":"PRiSM","version":25000}]
            })
            .to_string(),
        ),
        (
            "divingfish_song_list.json",
            json!([
                df_song(383, "SD", "13.0", "13"),
                df_song(10383, "DX", "13.8", "13+")
            ])
            .to_string(),
        ),
        ("lxns_alias_list.json", r#"{"aliases":[]}"#.to_owned()),
        ("music_alias.json", r#"{"content":[]}"#.to_owned()),
        ("custom_aliases.json", "{}".to_owned()),
        ("pinyin_aliases.json", r#"{"aliases":[]}"#.to_owned()),
        ("zh_s2t.json", "{}".to_owned()),
        (
            "divingfish_chart_stats.json",
            json!({"charts":{"383":[{},{},{},{"fit_diff":13.1}],
                "10383":[{},{},{},{"fit_diff":13.9}]}})
            .to_string(),
        ),
    ];
    for (name, contents) in files {
        fs::write(temp.path().join(name), contents)?;
    }
    Ok(())
}

fn df_song(id: u32, kind: &str, constant: &str, level: &str) -> Value {
    let constant = if constant == "13.8" {
        json!(13.8)
    } else {
        json!(13.0)
    };
    json!({"id":id.to_string(),"title":"Link(CoF)","type":kind,
        "ds":[1,2,3,constant],"level":["1","2","3",level],
        "charts":[{},{},{},{}],"basic_info":{"bpm":150,"from":"PRiSM","is_new":true}})
}

fn df_b50() -> MockResponse {
    MockResponse {
        status: 200,
        body: json!({"nickname":"DF Tester","rating":600,"charts":{
            "sd":[{"song_id":383,"title":"Link(CoF)","type":"SD","level":"13",
                "level_index":3,"ds":"13.0","achievements":"100.0","ra":280}],
            "dx":[{"song_id":10383,"title":"Link(CoF)","type":"DX","level":"13+",
                "level_index":3,"ds":"13.8","achievements":"100.0","ra":298}]
        }})
        .to_string(),
    }
}

fn lxns_score(kind: &str) -> Value {
    json!({"id":383,"type":kind,"level_index":3,"achievements":"100.0",
        "dx_score":1000,"ds":if kind == "dx" {"13.8"} else {"13.0"}})
}

fn qq_lookup() -> Result<Lookup, maimai_core::ValidationError> {
    Ok(Lookup::Qq(QqId::new("10001")?))
}

fn lxns_id(value: u32) -> SourceSongId {
    SourceSongId::numeric(SongIdNamespace::Lxns, value)
}

fn df_id(value: u32) -> SourceSongId {
    SourceSongId::numeric(SongIdNamespace::DivingFish, value)
}

#[derive(Clone)]
struct MockResponse {
    status: u16,
    body: String,
}

async fn mock_server(
    responses: Vec<MockResponse>,
) -> Result<(Url, mpsc::Receiver<String>), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(responses.len().max(1));
    tokio::spawn(async move {
        for response in responses {
            let Ok((mut stream, _)) = listener.accept().await else {
                break;
            };
            let Ok(request) = read_request(&mut stream).await else {
                break;
            };
            let _ = sender.send(request).await;
            let reason = if response.status >= 400 {
                "Error"
            } else {
                "OK"
            };
            let encoded = format!(
                "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                response.status,
                reason,
                response.body.len(),
                response.body
            );
            if stream.write_all(encoded.as_bytes()).await.is_err() {
                break;
            }
        }
    });
    Ok((Url::parse(&format!("http://{address}/"))?, receiver))
}

async fn read_request(stream: &mut tokio::net::TcpStream) -> Result<String, io::Error> {
    let mut request = Vec::new();
    let mut buffer = [0_u8; 4096];
    loop {
        let read = stream.read(&mut buffer).await?;
        if read == 0 {
            break;
        }
        request.extend_from_slice(&buffer[..read]);
        if request.windows(4).any(|window| window == b"\r\n\r\n") {
            let headers = String::from_utf8_lossy(&request);
            let body_length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")?
                        .trim()
                        .parse::<usize>()
                        .ok()
                })
                .unwrap_or(0);
            let header_end = request
                .windows(4)
                .position(|window| window == b"\r\n\r\n")
                .map_or(request.len(), |index| index + 4);
            if request.len() >= header_end + body_length {
                break;
            }
        }
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}

async fn receive(
    receiver: &mut mpsc::Receiver<String>,
    count: usize,
) -> Result<Vec<String>, io::Error> {
    let mut output = Vec::new();
    for _ in 0..count {
        output.push(
            receiver
                .recv()
                .await
                .ok_or_else(|| io::Error::other("mock request channel closed"))?,
        );
    }
    Ok(output)
}

fn request_json(request: &str) -> Result<Value, io::Error> {
    let body = request.split_once("\r\n\r\n").map_or("", |(_, body)| body);
    serde_json::from_str(body).map_err(io::Error::other)
}
