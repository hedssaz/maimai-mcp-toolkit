use maimai_catalog::{CatalogQuery, CatalogSnapshot, SearchHit};
use serde_json::{Map, Value, json};

use super::{
    convert::catalog_query,
    dto::{BatchArgs, SearchArgs},
    error::CatalogToolError,
    serialize::song_value,
};

pub(super) fn search(
    snapshot: &CatalogSnapshot,
    arguments: &SearchArgs,
) -> Result<Value, CatalogToolError> {
    let mut query = catalog_query(snapshot, arguments)?;
    let limit = query.limit.take();
    let mut hits = snapshot.query(&query)?;
    let total_matches = hits.len();
    if let Some(limit) = limit {
        hits.truncate(limit);
    }
    search_result(
        snapshot,
        arguments,
        &hits,
        total_matches,
        limit.is_some_and(|limit| total_matches > limit),
    )
}

pub(super) fn batch(
    snapshot: &CatalogSnapshot,
    arguments: &BatchArgs,
) -> Result<Value, CatalogToolError> {
    if arguments.items.is_empty() {
        return Err(CatalogToolError::input("items must be a non-empty array"));
    }
    if arguments.items.len() > 200 {
        return Err(CatalogToolError::input(
            "items can contain at most 200 searches",
        ));
    }
    let mut success = 0_usize;
    let items = arguments
        .items
        .iter()
        .enumerate()
        .map(|(index, item)| match search(snapshot, &item.search) {
            Ok(result) => {
                success += 1;
                json!({
                    "index": index,
                    "key": item.key,
                    "ok": true,
                    "result": result,
                    "error": null,
                })
            }
            Err(error) => json!({
                "index": index,
                "key": item.key,
                "ok": false,
                "result": null,
                "error": {"message": error.to_string()},
            }),
        })
        .collect::<Vec<_>>();
    Ok(json!({
        "count": items.len(),
        "counts": {
            "requested": items.len(),
            "success": success,
            "failure": items.len() - success,
        },
        "items": items,
    }))
}

pub(super) fn execute_query<'a>(
    snapshot: &'a CatalogSnapshot,
    arguments: &SearchArgs,
) -> Result<(CatalogQuery, Vec<SearchHit<'a>>), CatalogToolError> {
    let mut query = catalog_query(snapshot, arguments)?;
    query.limit = None;
    let hits = snapshot.query(&query)?;
    Ok((query, hits))
}

pub(super) fn search_result(
    snapshot: &CatalogSnapshot,
    arguments: &SearchArgs,
    hits: &[SearchHit<'_>],
    total_matches: usize,
    truncated: bool,
) -> Result<Value, CatalogToolError> {
    let mut output = Map::new();
    output.insert("count".to_owned(), json!(hits.len()));
    output.insert("total_matches".to_owned(), json!(total_matches));
    if truncated {
        output.insert("truncated".to_owned(), json!(true));
    }
    output.insert("criteria".to_owned(), criteria(arguments)?);
    output.insert(
        "songs".to_owned(),
        Value::Array(hits.iter().map(|hit| song_value(snapshot, hit)).collect()),
    );
    Ok(Value::Object(output))
}

fn criteria(arguments: &SearchArgs) -> Result<Value, CatalogToolError> {
    let mut value = serde_json::to_value(arguments)?;
    if let Some(output) = value.as_object_mut() {
        output.remove("format");
        output.remove("include_raw");
        output.remove("debug");
    }
    Ok(value)
}
