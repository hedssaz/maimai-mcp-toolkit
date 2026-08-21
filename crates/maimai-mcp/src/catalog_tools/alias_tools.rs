use std::collections::BTreeMap;

use maimai_catalog::{
    AddAliasOutcome, AddAliasRequest, AddAliasResult, AliasKind, AliasListRequest, AliasListResult,
    AliasSong, AliasText, CanonicalName, CatalogStore, DeleteAliasRequest, DeleteAliasResult,
    SongAliasMutation, SongAliasTarget, SongTitle, SourceKind,
};
use maimai_core::SongIdValue;
use serde_json::{Map, Value, json};

use super::{
    dto::{AliasListArgs, AliasMutationArgs, OneOrManyString, Scalar, SearchArgs},
    error::CatalogToolError,
};

pub(super) async fn add(
    store: &CatalogStore,
    arguments: &AliasMutationArgs,
) -> Result<Value, CatalogToolError> {
    let kind = AliasKind::parse(arguments.kind.as_deref())?;
    let alias = AliasText::new(&arguments.alias)?;
    let request = match kind {
        AliasKind::Song => AddAliasRequest::song(song_target(arguments)?, alias),
        AliasKind::Artist | AliasKind::Charter => AddAliasRequest::name(
            kind,
            CanonicalName::new(arguments.canonical.clone().unwrap_or_default(), kind)?,
            alias,
        )?,
    };
    add_value(store.add_alias(request).await?)
}

pub(super) async fn delete(
    store: &CatalogStore,
    arguments: &AliasMutationArgs,
) -> Result<Value, CatalogToolError> {
    let kind = AliasKind::parse(arguments.kind.as_deref())?;
    let alias = AliasText::new(&arguments.alias)?;
    let request = match kind {
        AliasKind::Song => DeleteAliasRequest::song(song_target(arguments)?, alias),
        AliasKind::Artist | AliasKind::Charter => DeleteAliasRequest::name(
            kind,
            CanonicalName::new(arguments.canonical.clone().unwrap_or_default(), kind)?,
            alias,
        )?,
    };
    Ok(delete_value(store.delete_alias(request).await?))
}

pub(super) async fn list(
    store: &CatalogStore,
    arguments: &AliasListArgs,
) -> Result<Value, CatalogToolError> {
    let kind = AliasKind::parse(arguments.kind.as_deref())?;
    let query = list_query(kind, arguments)?;
    let request = AliasListRequest::new(
        kind,
        query.map(AliasText::new).transpose()?,
        arguments.limit,
    )?;
    list_value(store.list_aliases(request).await?)
}

fn song_target(arguments: &AliasMutationArgs) -> Result<SongAliasTarget, CatalogToolError> {
    if let Some(value) = &arguments.song_id {
        return scalar_target(value);
    }
    if let Some(title) = arguments
        .title
        .as_ref()
        .filter(|value| !value.trim().is_empty())
    {
        return Ok(SongAliasTarget::Title(SongTitle::new(title)?));
    }
    Err(maimai_catalog::AliasError::SongTargetRequired.into())
}

fn scalar_target(value: &Scalar) -> Result<SongAliasTarget, CatalogToolError> {
    let text = value.text();
    let value = match text.parse::<u32>() {
        Ok(value) => SongIdValue::Numeric(value),
        Err(_) => {
            SongIdValue::text(text).map_err(|error| CatalogToolError::input(error.to_string()))?
        }
    };
    Ok(SongAliasTarget::Id(value))
}

fn list_query(
    kind: AliasKind,
    arguments: &AliasListArgs,
) -> Result<Option<String>, CatalogToolError> {
    if kind != AliasKind::Song {
        return Ok(arguments
            .query
            .as_ref()
            .map(|value| value.trim().to_owned())
            .filter(|value| !value.is_empty()));
    }
    let query = arguments
        .query
        .as_ref()
        .map(|value| value.trim().to_owned())
        .filter(|value| !value.is_empty())
        .or_else(|| arguments.song_id.as_ref().map(Scalar::text))
        .or_else(|| {
            arguments
                .title
                .as_ref()
                .map(|value| value.trim().to_owned())
                .filter(|value| !value.is_empty())
        });
    if query.is_none() {
        return Err(maimai_catalog::AliasError::SongListQueryRequired.into());
    }
    Ok(query)
}

fn add_value(result: AddAliasResult) -> Result<Value, CatalogToolError> {
    let existed = result.outcome() == AddAliasOutcome::AlreadyExists;
    Ok(match result {
        AddAliasResult::Song { value, .. } => {
            let mut output = song_mutation(&value);
            output.insert("alias".to_owned(), json!(value.alias));
            output.insert("existed".to_owned(), json!(existed));
            output.insert("aliases".to_owned(), json!(value.aliases));
            Value::Object(output)
        }
        AddAliasResult::Name { value, .. } => {
            let mut output = name_mutation(&value);
            output.insert("alias".to_owned(), json!(value.alias));
            output.insert("existed".to_owned(), json!(existed));
            output.insert("aliases".to_owned(), json!(value.aliases));
            if let Some(warning) = value.warning {
                output.insert("warning".to_owned(), json!(warning));
            }
            Value::Object(output)
        }
    })
}

fn delete_value(result: DeleteAliasResult) -> Value {
    match result {
        DeleteAliasResult::Song(value) => {
            let mut output = song_mutation(&value);
            output.insert("removed_alias".to_owned(), json!(value.alias));
            output.insert("remaining_aliases".to_owned(), json!(value.aliases));
            Value::Object(output)
        }
        DeleteAliasResult::Name(value) => {
            let mut output = name_mutation(&value);
            output.insert("removed_alias".to_owned(), json!(value.alias));
            output.insert("remaining_aliases".to_owned(), json!(value.aliases));
            Value::Object(output)
        }
    }
}

fn list_value(result: AliasListResult) -> Result<Value, CatalogToolError> {
    Ok(match result {
        AliasListResult::Songs {
            query,
            limit,
            total_matches,
            truncated,
            entries,
        } => json!({
            "count": entries.len(),
            "total_matches": total_matches,
            "truncated": truncated,
            "criteria": song_list_criteria(query, limit)?,
            "songs": entries.into_iter().map(|entry| {
                let mut value = song_value(entry.song);
                value.insert("alias_count".to_owned(), json!(entry.aliases.len()));
                value.insert("aliases".to_owned(), json!(entry.aliases));
                Value::Object(value)
            }).collect::<Vec<_>>(),
        }),
        AliasListResult::Names {
            kind,
            entries,
            document,
        } => json!({
            "kind": kind.key(),
            "count": entries.len(),
            "entries": entries.into_iter().map(|entry| json!({
                "canonical": entry.canonical,
                "count": entry.aliases.len(),
                "aliases": entry.aliases,
            })).collect::<Vec<_>>(),
            "document": document.to_string_lossy(),
        }),
    })
}

fn song_mutation(value: &SongAliasMutation) -> Map<String, Value> {
    let mut output = song_value(value.song.clone());
    output.remove("id");
    output.remove("artist");
    output.remove("source");
    output.remove("source_labels");
    output.insert(
        "document".to_owned(),
        json!(value.document.to_string_lossy()),
    );
    output
}

fn name_mutation(value: &maimai_catalog::NameAliasMutation) -> Map<String, Value> {
    Map::from_iter([
        ("kind".to_owned(), json!(value.kind.key())),
        ("canonical".to_owned(), json!(value.canonical)),
        (
            "document".to_owned(),
            json!(value.document.to_string_lossy()),
        ),
    ])
}

fn song_value(song: AliasSong) -> Map<String, Value> {
    let source = song
        .source_labels
        .iter()
        .map(|source| source.source_name())
        .collect::<Vec<_>>()
        .join("+");
    Map::from_iter([
        ("id".to_owned(), json!(id_value(&song.song_id))),
        ("song_id".to_owned(), json!(id_value(&song.song_id))),
        (
            "source_id".to_owned(),
            json!(id_value(song.source_id.value())),
        ),
        ("source_ids".to_owned(), source_ids(song.source_ids)),
        ("title".to_owned(), json!(song.title)),
        ("artist".to_owned(), json!(song.artist)),
        ("source".to_owned(), json!(source)),
        (
            "source_labels".to_owned(),
            json!(
                song.source_labels
                    .iter()
                    .map(|source| source.key())
                    .collect::<Vec<_>>()
            ),
        ),
    ])
}

fn source_ids(values: BTreeMap<SourceKind, maimai_core::SourceSongId>) -> Value {
    Value::Object(
        values
            .into_iter()
            .map(|(source, value)| (source.key().to_owned(), json!(id_value(value.value()))))
            .collect(),
    )
}

fn song_list_criteria(query: String, limit: usize) -> Result<Value, CatalogToolError> {
    let mut search = SearchArgs {
        query: Some(query),
        limit: Some(limit),
        region_has: Some(OneOrManyString::Many(Vec::new())),
        region_missing: Some(OneOrManyString::Many(Vec::new())),
        ..SearchArgs::default()
    };
    search.is_new = None;
    let mut value = serde_json::to_value(search)?;
    if let Some(output) = value.as_object_mut() {
        for key in ["format", "include_raw", "debug", "is_new", "is_new_source"] {
            output.remove(key);
        }
    }
    Ok(value)
}

fn id_value(value: &SongIdValue) -> String {
    match value {
        SongIdValue::Numeric(value) => value.to_string(),
        SongIdValue::Text(value) => value.as_str().to_owned(),
    }
}
