use std::{collections::BTreeMap, error::Error, time::Duration};

use url::Url;

use super::{
    BundleStatus, CatalogSource, CatalogSourceClient, CatalogSourceConfig, CatalogSourceErrorCode,
    DocumentStatistics, EntityTag, SourceTarget,
    config::EndpointSet,
    mock::{MockResponse, Routes, server},
};

const LXNS_ALIASES: &str = r#"{"aliases":[{"aliases":["a"],"song_id":1}]}"#;
const DXDATA: &str = r#"{"songs":[{}],"updateTime":"2026-08-18"}"#;
const DIVING_FISH: &str = r#"[{"id":"1"}]"#;
const YUZU: &str = r#"{"code":0,"content":[{"Alias":["a"],"SongID":1}]}"#;
const CHART_STATS: &str = r#"{"charts":{"1":[]},"diff_data":{}}"#;
const DXRATING_ALIASES: &str = r#"[{"name":"a","song_id":"A"}]"#;
const PLATE: &str = r#"{"code":0,"content":{"真":[1]}}"#;
const LOCATION: &str = r#"{"data":{"locations":[{"id":"1"}]}}"#;

const LXNS_SONGS_RAW: &str = " { \"songs\" : [ { \"id\" : 1 } ] } ";
const DXDATA_RAW: &str = r#"{"updateTime":"2026-08-18","songs":[{}]}"#;
const YUZU_RAW: &str = r#"{"content":[{"SongID":1,"Alias":["a"]}],"code":0}"#;
const DXRATING_TAGS_RAW: &str = r#"{"tags":[],"tagSongs":[],"tagGroups":[]}"#;

fn all_routes() -> Routes {
    BTreeMap::from([
        (
            "/lxns/songs?notes=true",
            vec![MockResponse::json(LXNS_SONGS_RAW)],
        ),
        ("/lxns/aliases", vec![MockResponse::json(LXNS_ALIASES)]),
        (
            "/dxdata/proxy",
            vec![MockResponse {
                content_type: "text/plain; charset=utf-8",
                ..MockResponse::json(DXDATA_RAW)
            }],
        ),
        ("/divingfish/music", vec![MockResponse::json(DIVING_FISH)]),
        ("/yuzu/aliases", vec![MockResponse::json(YUZU_RAW)]),
        ("/divingfish/stats", vec![MockResponse::json(CHART_STATS)]),
        (
            "/dxrating/aliases",
            vec![MockResponse::json(DXRATING_ALIASES)],
        ),
        (
            "/dxrating/tags",
            vec![MockResponse::json(DXRATING_TAGS_RAW)],
        ),
        ("/yuzu/plate", vec![MockResponse::json(PLATE)]),
        ("/wahlap/locations", vec![MockResponse::json(LOCATION)]),
    ])
}

fn test_client(
    base: &Url,
    config: CatalogSourceConfig,
) -> Result<CatalogSourceClient, super::CatalogSourceError> {
    CatalogSourceClient::with_endpoints(config, EndpointSet::local(base)?)
}

#[tokio::test]
async fn every_source_preserves_validated_remote_document_bytes() -> Result<(), Box<dyn Error>> {
    let (base, mut requests, task) = server(all_routes()).await?;
    let client = test_client(&base, CatalogSourceConfig::default())?;
    let expected = [
        (
            CatalogSource::Lxns,
            vec![
                (
                    SourceTarget::LxnsSongList,
                    LXNS_SONGS_RAW,
                    "2d29a69fca90a86f2e91ef6a4bfcf89c45638231827af485309c0baba14598a5",
                ),
                (
                    SourceTarget::LxnsAliasList,
                    LXNS_ALIASES,
                    "bab6911833823577cc2d90481470961c674381d54e20f44f822efa3b1aa53a7a",
                ),
            ],
        ),
        (
            CatalogSource::DxData,
            vec![(
                SourceTarget::DxData,
                DXDATA_RAW,
                "561ec5de726f2600bcb89ed1bd014ef6f10663c4930ed0eefb6acb02e3128769",
            )],
        ),
        (
            CatalogSource::DivingFish,
            vec![(
                SourceTarget::DivingFishSongList,
                DIVING_FISH,
                "51232916983b4485ec0e59dd333847051d7c31114772b995bf875aea43ebfded",
            )],
        ),
        (
            CatalogSource::Yuzu,
            vec![(
                SourceTarget::YuzuAliasList,
                YUZU_RAW,
                "1bb8a577f5218350120df9215d0b8026cab7f7c845324b4eede3085e98ca88dd",
            )],
        ),
        (
            CatalogSource::ChartStats,
            vec![(
                SourceTarget::DivingFishChartStats,
                CHART_STATS,
                "bfe6dea817c49b54190ff418d23bb5ea35f307d7506111e7b5888e4cd2522d21",
            )],
        ),
        (
            CatalogSource::DxRatingAliases,
            vec![(
                SourceTarget::DxRatingAliases,
                DXRATING_ALIASES,
                "58cd55d1aa3a37da9c091673ce464f4dcd0e90ace8bb96e36e6f6f71337c4009",
            )],
        ),
        (
            CatalogSource::DxRatingTags,
            vec![(
                SourceTarget::DxRatingTags,
                DXRATING_TAGS_RAW,
                "38e03c757c3fd7b4b5434424829935f55b751d27b37a284f2ec7190e0b9dfb3b",
            )],
        ),
        (
            CatalogSource::Plate,
            vec![(
                SourceTarget::Plate,
                PLATE,
                "b4abd95a091ed790e020a0b0228d8c75e0006cca38f944c71637e4cd64242afc",
            )],
        ),
        (
            CatalogSource::Location,
            vec![(
                SourceTarget::Location,
                LOCATION,
                "feb324965aee001aa55f23b05cfedb922b1b0b28bdc03fbf0c46041ddf4ba1dd",
            )],
        ),
    ];

    for (source, expected_documents) in expected {
        let bundle = client.fetch(source).await?;
        assert_eq!(bundle.source(), source);
        assert_eq!(bundle.status(), BundleStatus::Updated);
        assert_eq!(bundle.documents().len(), expected_documents.len());
        for (document, (target, bytes, digest)) in bundle.documents().iter().zip(expected_documents)
        {
            assert_eq!(document.target(), target);
            assert_eq!(document.bytes(), bytes.as_bytes());
            assert_eq!(document.digest().to_hex(), digest);
        }
    }
    let mut captured = Vec::new();
    while let Ok(request) = requests.try_recv() {
        captured.push(request);
    }
    task.await??;
    assert_eq!(captured.len(), 10);
    for (path, user_agent) in [
        ("/lxns/songs?notes=true", "maimai-catalog-source/0.1"),
        ("/lxns/aliases", "maimai-catalog-source/0.1"),
        ("/dxdata/proxy", "maimai-bot dxdata-updater"),
        ("/divingfish/music", "maimai-catalog-source/0.1"),
        ("/yuzu/aliases", "maimai-bot yuzu-alias-updater"),
        ("/divingfish/stats", "maimai-catalog-source/0.1"),
        ("/dxrating/aliases", "Mozilla/5.0 maimai-bot"),
        ("/dxrating/tags", "Mozilla/5.0 maimai-bot"),
        ("/yuzu/plate", "maimai-bot plate-updater"),
        ("/wahlap/locations", "maimai-bot location-updater"),
    ] {
        assert!(
            captured
                .iter()
                .any(|request| request_matches(request, path, user_agent))
        );
    }
    Ok(())
}

fn request_matches(request: &str, path: &str, user_agent: &str) -> bool {
    request.starts_with(&format!("GET {path} "))
        && request
            .to_ascii_lowercase()
            .contains(&format!("user-agent: {}", user_agent.to_ascii_lowercase()))
}

#[test]
fn source_target_matrix_is_complete_and_stable() {
    let targets = CatalogSource::ALL
        .iter()
        .flat_map(|source| source.targets())
        .copied()
        .collect::<Vec<_>>();
    assert_eq!(targets.len(), 10);
    assert_eq!(targets[0].file_name(), "lxns_song_list.json");
    assert_eq!(targets[1].file_name(), "lxns_alias_list.json");
    assert_eq!(targets[9].file_name(), "sega_maidx_locations.json");
}

#[test]
fn entity_tags_are_canonical_and_reject_header_or_grammar_injection() -> Result<(), Box<dyn Error>>
{
    assert_eq!(
        EntityTag::parse("catalog-v1")?.as_header_value(),
        "\"catalog-v1\""
    );
    assert_eq!(
        EntityTag::parse("W/\"weak\"")?.as_header_value(),
        "W/\"weak\""
    );
    for invalid in ["", "\"unterminated", "\"a\"b\"", "W/\"\"", "line\r\nbreak"] {
        let error = EntityTag::parse(invalid)
            .err()
            .ok_or("expected invalid entity tag")?;
        assert_eq!(error.code(), CatalogSourceErrorCode::InvalidEntityTag);
    }
    Ok(())
}

#[tokio::test]
async fn diving_fish_sends_etag_and_handles_not_modified() -> Result<(), Box<dyn Error>> {
    let mut response = MockResponse::status(304);
    response.headers.push(("ETag", "\"catalog-v2\""));
    let (base, mut requests, task) =
        server(BTreeMap::from([("/divingfish/music", vec![response])])).await?;
    let etag = EntityTag::parse("catalog-v1")?;
    let client = test_client(&base, CatalogSourceConfig::default())?;
    let bundle = client
        .fetch_with_etag(CatalogSource::DivingFish, Some(&etag))
        .await?;
    let request = requests.recv().await.ok_or("request not captured")?;
    task.await??;
    assert_eq!(bundle.status(), BundleStatus::NotModified);
    assert!(bundle.documents().is_empty());
    assert_eq!(
        bundle.etag().map(EntityTag::as_header_value),
        Some("\"catalog-v2\"")
    );
    assert!(
        request
            .to_ascii_lowercase()
            .contains("if-none-match: \"catalog-v1\"")
    );
    Ok(())
}

#[tokio::test]
async fn dxdata_falls_back_only_after_request_failure() -> Result<(), Box<dyn Error>> {
    let (base, mut requests, task) = server(BTreeMap::from([
        ("/dxdata/proxy", vec![MockResponse::status(503)]),
        ("/dxdata/github", vec![MockResponse::json(DXDATA)]),
    ]))
    .await?;
    let bundle = test_client(&base, CatalogSourceConfig::default())?
        .fetch(CatalogSource::DxData)
        .await?;
    let first = requests.recv().await.ok_or("first request not captured")?;
    let second = requests.recv().await.ok_or("second request not captured")?;
    task.await??;
    assert!(first.starts_with("GET /dxdata/proxy "));
    assert!(second.starts_with("GET /dxdata/github "));
    assert_eq!(bundle.metadata().update_time(), Some("2026-08-18"));
    Ok(())
}

#[tokio::test]
async fn transport_limits_and_shape_errors_are_structured_and_redacted()
-> Result<(), Box<dyn Error>> {
    let cases = [
        (
            MockResponse {
                status: 302,
                content_type: "application/json",
                headers: vec![("Location", "https://secret.invalid/?token=sentinel")],
                body: "sentinel-body",
                delay: None,
            },
            CatalogSourceErrorCode::Redirect,
        ),
        (
            MockResponse {
                status: 200,
                content_type: "text/html",
                headers: Vec::new(),
                body: "<html>sentinel-body</html>",
                delay: None,
            },
            CatalogSourceErrorCode::InvalidContentType,
        ),
        (
            MockResponse::json("sentinel-not-json"),
            CatalogSourceErrorCode::InvalidJson,
        ),
        (
            MockResponse::json(r#"{"sentinel":"wrong-shape"}"#),
            CatalogSourceErrorCode::InvalidShape,
        ),
    ];
    for (response, expected_code) in cases {
        let (base, _requests, task) =
            server(BTreeMap::from([("/yuzu/aliases", vec![response])])).await?;
        let error = test_client(&base, CatalogSourceConfig::default())?
            .fetch(CatalogSource::Yuzu)
            .await
            .err()
            .ok_or("expected provider error")?;
        task.await??;
        assert_eq!(error.code(), expected_code);
        let rendered = format!("{error:?} {error}");
        assert!(!rendered.contains("sentinel"));
        assert!(!rendered.contains(base.as_str()));
    }

    let (base, _requests, task) = server(BTreeMap::from([(
        "/yuzu/aliases",
        vec![MockResponse::json(YUZU)],
    )]))
    .await?;
    let error = test_client(&base, CatalogSourceConfig::new("test", 4))?
        .fetch(CatalogSource::Yuzu)
        .await
        .err()
        .ok_or("expected body limit error")?;
    task.await??;
    assert_eq!(error.code(), CatalogSourceErrorCode::BodyTooLarge);
    Ok(())
}

#[tokio::test]
async fn timeout_and_invalid_configuration_fail_closed() -> Result<(), Box<dyn Error>> {
    let (base, _requests, task) = server(BTreeMap::from([(
        "/yuzu/aliases",
        vec![MockResponse {
            delay: Some(Duration::from_millis(80)),
            ..MockResponse::json(YUZU)
        }],
    )]))
    .await?;
    let config = CatalogSourceConfig::default().with_timeout_override(Duration::from_millis(10));
    let error = test_client(&base, config)?
        .fetch(CatalogSource::Yuzu)
        .await
        .err()
        .ok_or("expected timeout")?;
    assert_eq!(error.code(), CatalogSourceErrorCode::Timeout);
    task.abort();

    let invalid = CatalogSourceClient::new(CatalogSourceConfig::new("bad\r\nheader", 1))
        .err()
        .ok_or("expected invalid user agent")?;
    assert_eq!(invalid.code(), CatalogSourceErrorCode::InvalidConfiguration);
    let invalid_url = Url::parse("http://user:secret@127.0.0.1/root?token=secret")?;
    let invalid = EndpointSet::local(&invalid_url)
        .err()
        .ok_or("expected invalid endpoint")?;
    assert_eq!(invalid.code(), CatalogSourceErrorCode::InvalidConfiguration);
    Ok(())
}

#[test]
fn statistics_are_typed_not_stringly_maps() {
    let stats = DocumentStatistics::Tags {
        tags: 2,
        groups: 1,
        associations: 3,
    };
    assert!(matches!(
        stats,
        DocumentStatistics::Tags {
            tags: 2,
            groups: 1,
            associations: 3
        }
    ));
}
