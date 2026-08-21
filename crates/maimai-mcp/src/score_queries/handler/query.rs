use maimai_app::{
    score_service::{B50Mode, B50Request, ScoreQuery, SongScoresRequest},
    scores::{B50Chart, RatingMode},
};
use maimai_catalog::{CatalogQuery, CatalogSnapshot, PlateName, PlateQuery, PlateServer};
use maimai_core::SongIdValue;
use serde_json::Value;

use crate::{DispatchError, ToolOutput};

use super::{ScoreQueryHandler, clock, within};
use crate::score_queries::{
    convert::{self, LookupInput},
    dto::{B50Args, RecordsArgs, SongScoreArgs},
    error::ScoreQueryToolError,
    filter::{DisplayOptions, filter_records},
    format, serialize,
};

impl ScoreQueryHandler {
    pub(super) async fn query_b50(
        &self,
        args: B50Args,
        computed: bool,
    ) -> Result<ToolOutput, DispatchError> {
        let execution = self.execute_b50(args, computed, false).await?;
        Ok(ToolOutput::text(execution.text).with_structured_content(execution.structured))
    }

    pub(super) async fn execute_b50(
        &self,
        args: B50Args,
        computed: bool,
        force_diving_fish: bool,
    ) -> Result<B50Execution, ScoreQueryToolError> {
        let mut options = DisplayOptions::from_args(&args)?;
        if computed {
            options.include_chart_metadata = true;
        }
        let timeout = convert::timeout(args.timeout_ms)?;
        let resolved = convert::lookup(lookup_input(&args), &self.identities).await?;
        let source = if force_diving_fish {
            Some(maimai_core::ScoreSource::DivingFish)
        } else {
            convert::source(args.source.clone(), self.deployment.is_public())?
        };
        let credentials = None;
        let (now, requested_at) = clock()?;
        let request = B50Request {
            query: ScoreQuery {
                lookup: resolved.lookup,
                source,
                diving_fish_credentials: credentials,
                now,
            },
            mode: if computed {
                B50Mode::Computed(RatingMode::Fit)
            } else {
                B50Mode::Provider
            },
        };
        let response = within(timeout, async {
            self.service
                .b50_with_evidence(request, args.include_raw)
                .await
                .map_err(Into::into)
        })
        .await?;
        let (result, raw, selection) = response.into_parts();
        let selection = self.visible_selection(selection);
        let text = format::b50(&result, resolved.identity.as_ref(), &options)?;
        let structured = serialize::b50(
            &result,
            serialize::OutputContext {
                requested_at,
                identity: resolved.identity,
                selection,
                include_chart_metadata: options.include_chart_metadata,
                raw,
            },
        )?;
        Ok(B50Execution { text, structured })
    }

    pub(super) async fn song_score(
        &self,
        args: SongScoreArgs,
    ) -> Result<ToolOutput, DispatchError> {
        let timeout = convert::timeout(args.timeout_ms)?;
        let resolved = convert::lookup(
            LookupInput {
                qq: args.qq,
                username: args.username,
                target: args.target,
                group_id: args.group_id,
            },
            &self.identities,
        )
        .await?;
        let snapshot = self.catalog.snapshot();
        let filter = convert::song_filter(
            args.music_id.clone(),
            args.song_query,
            args.difficulty,
            args.song_type,
            args.search_limit,
            &snapshot,
        )?;
        let music_id = source_id_json(filter.song.value());
        let source = convert::source(args.source, self.deployment.is_public())?;
        let credentials = convert::credentials(args.developer_token)?;
        let (now, requested_at) = clock()?;
        let response = within(timeout, async {
            self.service
                .song_scores_with_evidence(
                    SongScoresRequest {
                        query: ScoreQuery {
                            lookup: resolved.lookup,
                            source,
                            diving_fish_credentials: credentials,
                            now,
                        },
                        filter,
                    },
                    args.include_raw,
                )
                .await
                .map_err(Into::into)
        })
        .await?;
        let (scores, raw, selection) = response.into_parts();
        let selection = self.visible_selection(selection);
        let structured = serialize::song_scores(
            &scores,
            music_id,
            serialize::OutputContext {
                requested_at,
                identity: resolved.identity,
                selection,
                include_chart_metadata: true,
                raw,
            },
        )?;
        let text = format::pretty_json(&structured)?;
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }

    pub(super) async fn player_records(
        &self,
        args: RecordsArgs,
    ) -> Result<ToolOutput, DispatchError> {
        let timeout = convert::timeout(args.timeout_ms)?;
        let resolved = convert::lookup(
            LookupInput {
                qq: args.qq,
                username: args.username,
                target: args.target,
                group_id: args.group_id,
            },
            &self.identities,
        )
        .await?;
        let source = convert::source(args.source, self.deployment.is_public())?;
        let credentials = None;
        let snapshot = self.catalog.snapshot();
        let song_filter = args
            .music_id
            .clone()
            .map(|value| convert::song_filter(Some(value), None, None, None, None, &snapshot))
            .transpose()?;
        let (now, requested_at) = clock()?;
        let response = within(timeout, async {
            let query = ScoreQuery {
                lookup: resolved.lookup,
                source,
                diving_fish_credentials: credentials,
                now,
            };
            match song_filter {
                Some(filter) => self
                    .service
                    .song_scores_with_evidence(
                        SongScoresRequest { query, filter },
                        args.include_raw,
                    )
                    .await
                    .map_err(Into::into),
                None => self
                    .service
                    .records_with_evidence(query, args.include_raw)
                    .await
                    .map_err(Into::into),
            }
        })
        .await?;
        let (scores, raw, selection) = response.into_parts();
        let selection = self.visible_selection(selection);
        let mut records = filter_records(&scores, args.level.as_deref(), args.version.as_deref());
        let plate = if let Some(plate) = args.plate {
            let (filtered, metadata) = filter_plate(
                records,
                &snapshot,
                plate,
                args.server.as_deref(),
                self.deployment.is_public(),
            )?;
            records = filtered;
            Some(metadata)
        } else {
            validate_server(args.server.as_deref(), self.deployment.is_public())?;
            None
        };
        let structured = serialize::records(
            &scores,
            &records,
            scores.records.len(),
            plate,
            serialize::OutputContext {
                requested_at,
                identity: resolved.identity,
                selection,
                include_chart_metadata: true,
                raw,
            },
        )?;
        let text = format::pretty_json(&structured)?;
        Ok(ToolOutput::text(text).with_structured_content(structured))
    }
}

pub(super) struct B50Execution {
    pub(super) text: String,
    pub(super) structured: Value,
}

fn lookup_input(args: &B50Args) -> LookupInput {
    LookupInput {
        qq: args.qq.clone(),
        username: args.username.clone(),
        target: args.target.clone(),
        group_id: args.group_id.clone(),
    }
}

fn filter_plate(
    records: Vec<B50Chart>,
    snapshot: &CatalogSnapshot,
    name: String,
    server: Option<&str>,
    public: bool,
) -> Result<(Vec<B50Chart>, serialize::PlateFilterMetadata), ScoreQueryToolError> {
    let server = validate_server(server, public)?;
    let name =
        PlateName::new(name).map_err(|_| ScoreQueryToolError::invalid("plate 格式不正确。"))?;
    let membership = snapshot.plate_membership(&PlateQuery::new(name.clone(), server));
    let diving_fish_ids = diving_fish_ids(snapshot)?;
    let records = records
        .into_iter()
        .filter(|record| {
            membership.matches(
                diving_fish_ids.get(record.key.song()).copied(),
                &record.title,
                record.key.generation(),
            )
        })
        .collect();
    Ok((
        records,
        serialize::PlateFilterMetadata {
            name: name.as_str().to_owned(),
            server,
            song_count: membership.song_count(),
        },
    ))
}

fn validate_server(value: Option<&str>, public: bool) -> Result<PlateServer, ScoreQueryToolError> {
    match value.unwrap_or("cn") {
        "cn" => Ok(PlateServer::Cn),
        "jp" if !public => Ok(PlateServer::Jp),
        "jp" => Err(ScoreQueryToolError::invalid(
            "public surface 的 server 只支持 cn。",
        )),
        _ => Err(ScoreQueryToolError::invalid("server 必须是 cn 或 jp。")),
    }
}

fn diving_fish_ids(
    snapshot: &CatalogSnapshot,
) -> Result<std::collections::BTreeMap<maimai_core::SourceSongId, u32>, ScoreQueryToolError> {
    let hits = snapshot
        .query(&CatalogQuery::default())
        .map_err(|_| ScoreQueryToolError::invalid("牌子筛选曲库查询失败。"))?;
    let mut values = std::collections::BTreeMap::new();
    for hit in hits {
        if let Ok(id) = convert::diving_fish_id(&hit) {
            values.insert(hit.music.primary_id.clone(), id);
        }
    }
    Ok(values)
}

fn source_id_json(value: &SongIdValue) -> Value {
    match value {
        SongIdValue::Numeric(value) => Value::from(*value),
        SongIdValue::Text(value) => Value::from(value.as_str()),
    }
}
