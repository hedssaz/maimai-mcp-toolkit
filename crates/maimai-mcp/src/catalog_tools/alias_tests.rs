use std::{error::Error, sync::Arc};

use maimai_catalog::{CatalogQuery, CatalogStore};
use serde_json::{Value, json};

use super::{
    execute_call,
    format::render,
    tests::{Fixture, object},
};

#[tokio::test]
async fn song_artist_and_charter_aliases_match_legacy_json_and_text() -> Result<(), Box<dyn Error>>
{
    let fixture = Fixture::new()?;
    let store = Arc::new(CatalogStore::load(fixture.files.clone()).await?);
    let custom_path = fixture.files.custom_aliases.to_string_lossy();

    let (song, song_text) = call(
        &store,
        "add_maimai_alias",
        json!({"song_id":1,"alias":"測試別名"}),
    )
    .await?;
    assert_eq!(
        song,
        json!({
            "song_id":"1",
            "source_id":"1",
            "source_ids":{"cn":"1","jp":"Alpha","divingfish":"10001"},
            "title":"Alpha",
            "alias":"測試別名",
            "existed":false,
            "aliases":["测试别名"],
            "document":custom_path,
        })
    );
    assert_eq!(
        song_text,
        format!(
            "别名已新增: 測試別名 -> Alpha (ID 1)\n保存位置: {}",
            fixture.files.custom_aliases.display()
        )
    );
    let (duplicate, duplicate_text) = call(
        &store,
        "add_maimai_alias",
        json!({"song_id":1,"alias":"测试别名"}),
    )
    .await?;
    assert_eq!(duplicate["existed"], true);
    assert_eq!(
        duplicate_text,
        format!(
            "别名已存在: 测试别名 -> Alpha (ID 1)\n保存位置: {}",
            fixture.files.custom_aliases.display()
        )
    );
    assert_eq!(store.snapshot().search_text("测试别名", 5).len(), 1);

    for (kind, canonical, label, path) in [
        (
            "artist",
            "Alice",
            "曲师",
            fixture
                .files
                .artist_aliases
                .as_ref()
                .ok_or_else(|| std::io::Error::other("artist path missing"))?,
        ),
        (
            "charter",
            "Carol",
            "谱师",
            fixture
                .files
                .charter_aliases
                .as_ref()
                .ok_or_else(|| std::io::Error::other("charter path missing"))?,
        ),
    ] {
        let (value, text) = call(
            &store,
            "add_maimai_alias",
            json!({"kind":kind,"canonical":canonical,"alias":"測試別名"}),
        )
        .await?;
        assert_eq!(
            value,
            json!({
                "kind":kind,
                "canonical":canonical,
                "alias":"測試別名",
                "existed":false,
                "aliases":["测试别名"],
                "document":path.to_string_lossy(),
            })
        );
        assert_eq!(
            text,
            format!(
                "{label}别名已新增: 測試別名 -> {canonical}\n该 {label} 当前别名: 测试别名\n保存位置: {}",
                path.display()
            )
        );
        let (listed, listed_text) = call(
            &store,
            "list_maimai_aliases",
            json!({"kind":kind,"query":"测试"}),
        )
        .await?;
        assert_eq!(listed["count"], 1);
        assert_eq!(listed["entries"][0]["canonical"], canonical);
        assert_eq!(
            listed_text,
            format!(
                "{label}别名词典: 共 1 条 (保存位置: {})\n1. {canonical} → 测试别名",
                path.display()
            )
        );
        let query = if kind == "artist" {
            CatalogQuery {
                artist: Some("测试别名".to_owned()),
                ..CatalogQuery::default()
            }
        } else {
            CatalogQuery {
                charter: Some("测试别名".to_owned()),
                ..CatalogQuery::default()
            }
        };
        assert_eq!(store.snapshot().query(&query)?.len(), 1);
        let (deleted, deleted_text) = call(
            &store,
            "delete_maimai_alias",
            json!({"kind":kind,"canonical":canonical,"alias":"测试别名"}),
        )
        .await?;
        assert_eq!(deleted["removed_alias"], "測試別名");
        assert_eq!(
            deleted_text,
            format!(
                "已删除{label}别名: 測試別名 ({label}: {canonical})\n剩余别名: 无\n保存位置: {}",
                path.display()
            )
        );
    }

    let (listed, listed_text) = call(
        &store,
        "list_maimai_aliases",
        json!({"query":"测试别名","limit":20}),
    )
    .await?;
    assert_eq!(listed["count"], 1);
    assert_eq!(listed["songs"][0]["alias_count"], 1);
    assert!(listed_text.contains("1. Alpha | ID 1"));

    let (deleted, deleted_text) = call(
        &store,
        "delete_maimai_alias",
        json!({"song_id":1,"alias":"测试别名"}),
    )
    .await?;
    assert_eq!(deleted["removed_alias"], "測試別名");
    assert_eq!(
        deleted_text,
        format!(
            "已删除别名: 測試別名 (歌曲: Alpha, ID 1)\n剩余别名: 无\n保存位置: {}",
            fixture.files.custom_aliases.display()
        )
    );
    Ok(())
}

#[tokio::test]
async fn invalid_and_missing_alias_inputs_are_tool_errors() -> Result<(), Box<dyn Error>> {
    let fixture = Fixture::new()?;
    let store = Arc::new(CatalogStore::load(fixture.files).await?);
    for arguments in [
        json!({"song_id":1,"alias":"   "}),
        json!({"title":"Missing","alias":"x"}),
        json!({"title":"Alpha","alias":"bad\nvalue"}),
    ] {
        assert!(
            execute_call(&store, None, None, "add_maimai_alias", object(arguments)?,)
                .await
                .is_err()
        );
    }
    assert!(
        execute_call(
            &store,
            None,
            None,
            "delete_maimai_alias",
            object(json!({"song_id":1,"alias":"missing"}))?,
        )
        .await
        .is_err()
    );
    Ok(())
}

async fn call(
    store: &CatalogStore,
    tool: &str,
    arguments: Value,
) -> Result<(Value, String), Box<dyn Error>> {
    let (value, options) = execute_call(store, None, None, tool, object(arguments)?).await?;
    let text = render(tool, &value, options)?;
    Ok((value, text))
}
