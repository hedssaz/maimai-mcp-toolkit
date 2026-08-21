use std::{error::Error, fs, io, sync::Arc, time::Duration};

use maimai_catalog::{CatalogFiles, CatalogStore};
use maimai_core::{QqId, ScoreSource};
use maimai_providers::{
    DivingFishClient, DivingFishScoreClient, LxnsOAuthClient, OAuthConfig,
    lxns_score::LxnsScoreEndpoint,
};
use maimai_storage::StateStore;
use secrecy::SecretString;
use serde_json::json;
use tempfile::TempDir;
use time::OffsetDateTime;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::mpsc,
};
use url::Url;

use crate::{identity::IdentityDirectory, oauth::OAuthService, score_service::PlayerScoreService};

use super::{PlayerLookupRequest, ScoreBySongRequest, ScoreBySongService, SongLookupRequest};

type TestResult = Result<(), Box<dyn Error + Send + Sync>>;

#[tokio::test]
async fn one_full_df_request_partitions_ids_and_overrides_local_preference() -> TestResult {
    let fixture = fixture().await?;
    fixture
        .store
        .set_score_source_preference(&QqId::new("10001")?, ScoreSource::Local)
        .await?;
    fixture
        .store
        .set_diving_fish_developer_token(
            &SecretString::from("developer-secret"),
            OffsetDateTime::UNIX_EPOCH,
        )
        .await?;
    let (api, mut requests) = mock_server(
        200,
        json!({
            "nickname":"Tester","rating":15000,
            "records":[
                {"id":288,"title":"六兆年と一夜物語","type":"SD","level":"13",
                    "level_index":3,"ds":"13.0","achievements":"99.9000","ra":280},
                {"id":10288,"title":"六兆年と一夜物語","type":"DX","level":"13+",
                    "level_index":3,"ds":"13.8","achievements":"100.0000","ra":300}
            ]
        })
        .to_string(),
    )
    .await?;
    let service = score_by_song_service(&fixture, &api)?;
    let result = service
        .query(ScoreBySongRequest {
            player: PlayerLookupRequest::Qq(QqId::new("10001")?),
            group_id: None,
            song: SongLookupRequest {
                query: "六兆年".to_owned(),
                difficulty: None,
                generation: None,
                limit: 5,
            },
            include_raw: false,
            now: OffsetDateTime::UNIX_EPOCH,
        })
        .await?;
    assert_eq!(result.source, ScoreSource::DivingFish);
    assert_eq!(result.selected_song.music_ids, [288, 10288]);
    assert_eq!(result.scores.len(), 2);
    assert_eq!(result.scores[0].records.len(), 1);
    assert_eq!(result.scores[1].records.len(), 1);
    let request = requests.recv().await.ok_or("request missing")?;
    assert!(request.starts_with("GET /api/maimaidxprober/dev/player/records?qq=10001"));
    assert!(
        request
            .to_ascii_lowercase()
            .contains("developer-token: developer-secret")
    );
    assert!(requests.try_recv().is_err());
    let stored = fixture
        .store
        .records_for_player(&QqId::new("10001")?)
        .await?;
    assert_eq!(stored.len(), 2);
    assert!(
        stored
            .iter()
            .all(|record| record.score_source == ScoreSource::DivingFish)
    );
    Ok(())
}

struct Fixture {
    _temp: TempDir,
    store: StateStore,
    catalog: Arc<CatalogStore>,
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

fn score_by_song_service(
    fixture: &Fixture,
    api: &Url,
) -> Result<ScoreBySongService, Box<dyn Error + Send + Sync>> {
    let identities = IdentityDirectory::new(fixture.store.clone());
    let oauth_config = OAuthConfig::new(
        "client",
        None,
        None,
        Url::parse("http://127.0.0.1:9/oauth/authorize")?,
        Url::parse("http://127.0.0.1:9/oauth/token")?,
        vec!["read_player".to_owned()],
    )?;
    let api_base = api.join("api/")?;
    let cover_base = api.join("covers/")?;
    let scores = Arc::new(PlayerScoreService::with_lxns(
        fixture.store.clone(),
        Arc::clone(&fixture.catalog),
        DivingFishScoreClient::new(DivingFishClient::with_base_urls(
            api_base.as_str(),
            cover_base.as_str(),
        )?),
        OAuthService::new(fixture.store.clone(), LxnsOAuthClient::new(oauth_config)?),
        LxnsScoreEndpoint::new(
            Url::parse("http://127.0.0.1:9/api/v0/")?,
            Duration::from_secs(1),
        )?,
    ));
    Ok(ScoreBySongService::new(
        identities,
        Arc::clone(&fixture.catalog),
        scores,
    ))
}

async fn mock_server(
    status: u16,
    body: String,
) -> Result<(Url, mpsc::Receiver<String>), Box<dyn Error + Send + Sync>> {
    let listener = TcpListener::bind("127.0.0.1:0").await?;
    let address = listener.local_addr()?;
    let (sender, receiver) = mpsc::channel(2);
    tokio::spawn(async move {
        let Ok((mut stream, _)) = listener.accept().await else {
            return;
        };
        let Ok(request) = read_request(&mut stream).await else {
            return;
        };
        let _ = sender.send(request).await;
        let response = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
            body.len(),
            body,
        );
        let _ = stream.write_all(response.as_bytes()).await;
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
            break;
        }
    }
    Ok(String::from_utf8_lossy(&request).into_owned())
}

fn write_catalog(temp: &TempDir) -> Result<(), io::Error> {
    let files = [
        (
            "lxns_song_list.json",
            json!({"songs":[{"id":288,"title":"六兆年と一夜物語","artist":"kemu","genre":"niconico","bpm":186,"version":25000,"difficulties":{"standard":[{"difficulty":3,"level":"13","level_value":13.0,"notes":{}}],"dx":[{"difficulty":3,"level":"13+","level_value":13.8,"notes":{}}]}}],"genres":[],"versions":[{"title":"PRiSM","version":25000}]}).to_string(),
        ),
        (
            "divingfish_song_list.json",
            json!([
                {"id":"288","title":"六兆年と一夜物語","type":"SD","ds":[1,2,3,13.0],"level":["1","2","3","13"],"charts":[{},{},{},{}],"basic_info":{"artist":"kemu","genre":"niconico","bpm":186,"from":"PRiSM","is_new":false}},
                {"id":"10288","title":"六兆年と一夜物語","type":"DX","ds":[1,2,3,13.8],"level":["1","2","3","13+"],"charts":[{},{},{},{}],"basic_info":{"artist":"kemu","genre":"niconico","bpm":186,"from":"PRiSM","is_new":false}}
            ]).to_string(),
        ),
        ("lxns_alias_list.json", r#"{"aliases":[]}"#.to_owned()),
        ("music_alias.json", r#"{"content":[]}"#.to_owned()),
        ("custom_aliases.json", r#"{"六兆年":["288"]}"#.to_owned()),
        ("pinyin_aliases.json", r#"{"aliases":[]}"#.to_owned()),
        ("zh_s2t.json", "{}".to_owned()),
    ];
    for (name, contents) in files {
        fs::write(temp.path().join(name), contents)?;
    }
    Ok(())
}
