use std::collections::HashSet;

use serde::Deserialize;
use serde_json::{Map, Value};
use thiserror::Error;

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SurfaceContract {
    server_info: ServerInfo,
    tools: Vec<ToolContract>,
}

#[derive(Debug, Deserialize)]
pub struct ServerInfo {
    name: String,
    version: String,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolContract {
    name: String,
    description: String,
    input_schema: Map<String, Value>,
}

#[derive(Debug, Error)]
pub enum ContractError {
    #[error("MCP 契约 JSON 无效：{0}")]
    Json(#[from] serde_json::Error),

    #[error("MCP 契约的 serverInfo.{field} 不能为空")]
    EmptyServerInfo { field: &'static str },

    #[error("MCP 契约含重复工具名：{0}")]
    DuplicateTool(String),

    #[error("组合 MCP 契约至少需要一个片段")]
    NoFragments,

    #[error("组合 MCP 契约的第 {index} 个片段为空或不含工具")]
    EmptyFragment { index: usize },

    #[error("MCP 工具 {tool} 的 {field} 不能为空")]
    EmptyToolField { tool: String, field: &'static str },

    #[error("MCP 工具 {tool} 的 inputSchema.type 必须是 object")]
    InvalidInputSchema { tool: String },
}

impl SurfaceContract {
    pub fn parse(source: &str) -> Result<Self, ContractError> {
        let contract: Self = serde_json::from_str(source)?;
        contract.validate()?;
        Ok(contract)
    }

    /// Compose independently validated contract fragments in their declared
    /// order. Tool names must be unique across the complete surface.
    pub fn compose(
        server_name: impl Into<String>,
        server_version: impl Into<String>,
        fragments: &[&str],
    ) -> Result<Self, ContractError> {
        if fragments.is_empty() {
            return Err(ContractError::NoFragments);
        }

        let parsed = fragments
            .iter()
            .enumerate()
            .map(|(index, source)| {
                if source.trim().is_empty() {
                    return Err(ContractError::EmptyFragment { index: index + 1 });
                }
                Self::parse(source)
            })
            .collect::<Result<Vec<_>, _>>()?;

        if let Some((index, _)) = parsed
            .iter()
            .enumerate()
            .find(|(_, fragment)| fragment.tools.is_empty())
        {
            return Err(ContractError::EmptyFragment { index: index + 1 });
        }

        let tool_count = parsed.iter().map(|fragment| fragment.tools.len()).sum();
        let mut tools = Vec::with_capacity(tool_count);
        let mut names = HashSet::with_capacity(tool_count);
        for fragment in parsed {
            for tool in fragment.tools {
                if !names.insert(tool.name.clone()) {
                    return Err(ContractError::DuplicateTool(tool.name));
                }
                tools.push(tool);
            }
        }

        let contract = Self {
            server_info: ServerInfo {
                name: server_name.into(),
                version: server_version.into(),
            },
            tools,
        };
        contract.validate()?;
        Ok(contract)
    }

    fn validate(&self) -> Result<(), ContractError> {
        if self.server_info.name.trim().is_empty() {
            return Err(ContractError::EmptyServerInfo { field: "name" });
        }
        if self.server_info.version.trim().is_empty() {
            return Err(ContractError::EmptyServerInfo { field: "version" });
        }

        let mut names = HashSet::with_capacity(self.tools.len());
        for tool in &self.tools {
            if tool.name.trim().is_empty() {
                return Err(ContractError::EmptyToolField {
                    tool: "<unknown>".to_owned(),
                    field: "name",
                });
            }
            if tool.description.trim().is_empty() {
                return Err(ContractError::EmptyToolField {
                    tool: tool.name.clone(),
                    field: "description",
                });
            }
            if !names.insert(tool.name.as_str()) {
                return Err(ContractError::DuplicateTool(tool.name.clone()));
            }
            if tool.input_schema.get("type") != Some(&Value::String("object".to_owned())) {
                return Err(ContractError::InvalidInputSchema {
                    tool: tool.name.clone(),
                });
            }
        }
        Ok(())
    }

    pub fn server_info(&self) -> &ServerInfo {
        &self.server_info
    }

    pub fn tools(&self) -> &[ToolContract] {
        &self.tools
    }

    /// Keep only an explicitly implemented vertical slice while preserving the
    /// frozen contract order and schemas for those tools.
    pub fn retain_tools(mut self, names: &[&str]) -> Self {
        self.tools
            .retain(|tool| names.contains(&tool.name.as_str()));
        self
    }
}

impl ServerInfo {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn version(&self) -> &str {
        &self.version
    }
}

impl ToolContract {
    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn description(&self) -> &str {
        &self.description
    }

    pub fn input_schema(&self) -> &Map<String, Value> {
        &self.input_schema
    }
}

#[cfg(test)]
mod tests {
    use std::collections::{BTreeMap, BTreeSet};

    use serde_json::json;

    use super::SurfaceContract;

    type FrozenTool = (String, String, serde_json::Map<String, serde_json::Value>);

    const MAIN: &[(&str, &str)] = &[
        (
            "b50_image",
            include_str!("../../../contracts/main/b50_image.json"),
        ),
        (
            "catalog",
            include_str!("../../../contracts/main/catalog.json"),
        ),
        (
            "identity",
            include_str!("../../../contracts/main/identity.json"),
        ),
        (
            "rankings",
            include_str!("../../../contracts/main/rankings.json"),
        ),
        (
            "render",
            include_str!("../../../contracts/main/render.json"),
        ),
        (
            "score_query",
            include_str!("../../../contracts/main/score_query.json"),
        ),
        (
            "scores",
            include_str!("../../../contracts/main/scores.json"),
        ),
        (
            "scoring",
            include_str!("../../../contracts/main/scoring.json"),
        ),
        (
            "update",
            include_str!("../../../contracts/main/update.json"),
        ),
    ];

    const PUBLIC: &[(&str, &str)] = &[
        (
            "catalog",
            include_str!("../../../contracts/public/catalog.json"),
        ),
        (
            "identity",
            include_str!("../../../contracts/public/identity.json"),
        ),
        (
            "oauth",
            include_str!("../../../contracts/public/oauth.json"),
        ),
        (
            "rankings",
            include_str!("../../../contracts/public/rankings.json"),
        ),
        (
            "render",
            include_str!("../../../contracts/public/render.json"),
        ),
        (
            "score_query",
            include_str!("../../../contracts/public/score_query.json"),
        ),
        (
            "scores",
            include_str!("../../../contracts/public/scores.json"),
        ),
        (
            "scoring",
            include_str!("../../../contracts/public/scoring.json"),
        ),
    ];

    const MAIN_UNIFIED: &[(&str, &str)] = &[
        (
            "b50_image",
            include_str!("../../../contracts/main/b50_image.json"),
        ),
        (
            "catalog",
            include_str!("../../../contracts/main/catalog.json"),
        ),
        (
            "identity",
            include_str!("../../../contracts/main/identity.json"),
        ),
        (
            "rankings",
            include_str!("../../../contracts/main/rankings.json"),
        ),
        (
            "render",
            include_str!("../../../contracts/main/render.json"),
        ),
        (
            "score_query",
            include_str!("../../../contracts/main/score_query.json"),
        ),
        (
            "scores",
            include_str!("../../../contracts/main/scores.json"),
        ),
        (
            "update",
            include_str!("../../../contracts/main/update.json"),
        ),
    ];

    const PUBLIC_UNIFIED: &[(&str, &str)] = &[
        (
            "catalog",
            include_str!("../../../contracts/public/catalog.json"),
        ),
        (
            "identity",
            include_str!("../../../contracts/public/identity.json"),
        ),
        (
            "oauth",
            include_str!("../../../contracts/public/oauth.json"),
        ),
        (
            "rankings",
            include_str!("../../../contracts/public/rankings.json"),
        ),
        (
            "render",
            include_str!("../../../contracts/public/render.json"),
        ),
        (
            "score_query",
            include_str!("../../../contracts/public/score_query.json"),
        ),
        (
            "scores",
            include_str!("../../../contracts/public/scores.json"),
        ),
    ];

    fn parse_all(
        fixtures: &[(&str, &str)],
    ) -> Result<BTreeMap<String, SurfaceContract>, Box<dyn std::error::Error>> {
        fixtures
            .iter()
            .map(|(name, source)| {
                SurfaceContract::parse(source)
                    .map(|contract| ((*name).to_owned(), contract))
                    .map_err(|error| error.into())
            })
            .collect()
    }

    fn unique_tools(contracts: &BTreeMap<String, SurfaceContract>) -> BTreeSet<&str> {
        contracts
            .values()
            .flat_map(|contract| contract.tools.iter().map(|tool| tool.name.as_str()))
            .collect()
    }

    fn exposure_count(contracts: &BTreeMap<String, SurfaceContract>) -> usize {
        contracts
            .values()
            .map(|contract| contract.tools.len())
            .sum()
    }

    fn compose(
        server_name: &str,
        fragments: &[(&str, &str)],
    ) -> Result<SurfaceContract, super::ContractError> {
        let sources = fragments
            .iter()
            .map(|(_, source)| *source)
            .collect::<Vec<_>>();
        SurfaceContract::compose(server_name, env!("CARGO_PKG_VERSION"), &sources)
    }

    fn expected_tools(fragments: &[(&str, &str)]) -> Result<Vec<FrozenTool>, super::ContractError> {
        fragments
            .iter()
            .map(|(_, source)| SurfaceContract::parse(source))
            .collect::<Result<Vec<_>, _>>()
            .map(|contracts| {
                contracts
                    .into_iter()
                    .flat_map(|contract| contract.tools)
                    .map(|tool| (tool.name, tool.description, tool.input_schema))
                    .collect()
            })
    }

    fn assert_frozen_composition(
        server_name: &str,
        expected_count: usize,
        fragments: &[(&str, &str)],
    ) -> Result<(), Box<dyn std::error::Error>> {
        let contract = compose(server_name, fragments)?;
        assert_eq!(contract.server_info().name(), server_name);
        assert_eq!(contract.server_info().version(), env!("CARGO_PKG_VERSION"));
        assert_eq!(contract.tools().len(), expected_count);

        let expected = expected_tools(fragments)?;
        assert_eq!(expected.len(), expected_count);
        for (actual, (name, description, input_schema)) in contract.tools().iter().zip(expected) {
            assert_eq!(actual.name(), name);
            assert_eq!(actual.description(), description);
            assert_eq!(actual.input_schema(), &input_schema);
        }
        Ok(())
    }

    #[test]
    fn frozen_contracts_are_well_formed() -> Result<(), Box<dyn std::error::Error>> {
        let main = parse_all(MAIN)?;
        let public = parse_all(PUBLIC)?;
        assert_eq!(main.len(), 9);
        assert_eq!(public.len(), 8);
        Ok(())
    }

    #[test]
    fn surface_counts_match_the_audited_baseline() -> Result<(), Box<dyn std::error::Error>> {
        let main = parse_all(MAIN)?;
        let public = parse_all(PUBLIC)?;
        assert_eq!(unique_tools(&main).len(), 78);
        assert_eq!(exposure_count(&main), 80);
        assert_eq!(unique_tools(&public).len(), 60);
        assert_eq!(exposure_count(&public), 62);
        Ok(())
    }

    #[test]
    fn unified_contracts_preserve_the_exact_frozen_surfaces()
    -> Result<(), Box<dyn std::error::Error>> {
        assert_frozen_composition("maimai-main", 78, MAIN_UNIFIED)?;
        assert_frozen_composition("maimai-public", 60, PUBLIC_UNIFIED)?;
        Ok(())
    }

    #[test]
    fn compose_rejects_duplicate_tools_across_fragments() -> Result<(), Box<dyn std::error::Error>>
    {
        let duplicate = r#"{
            "serverInfo":{"name":"duplicate","version":"1"},
            "tools":[{
                "name":"score_counts",
                "description":"duplicate",
                "inputSchema":{"type":"object"}
            }]
        }"#;
        let catalog = include_str!("../../../contracts/main/catalog.json");
        let error = match SurfaceContract::compose("maimai-main", "0.1.0", &[catalog, duplicate]) {
            Ok(_) => return Err("duplicate tool was accepted".into()),
            Err(error) => error,
        };
        assert!(matches!(
            error,
            super::ContractError::DuplicateTool(name) if name == "score_counts"
        ));
        Ok(())
    }

    #[test]
    fn compose_rejects_missing_empty_and_malformed_fragments() {
        let empty_tools = r#"{
            "serverInfo":{"name":"empty","version":"1"},
            "tools":[]
        }"#;
        assert!(matches!(
            SurfaceContract::compose("maimai-main", "0.1.0", &[]),
            Err(super::ContractError::NoFragments)
        ));
        assert!(matches!(
            SurfaceContract::compose("maimai-main", "0.1.0", &["  \n"]),
            Err(super::ContractError::EmptyFragment { index: 1 })
        ));
        assert!(matches!(
            SurfaceContract::compose("maimai-main", "0.1.0", &[empty_tools]),
            Err(super::ContractError::EmptyFragment { index: 1 })
        ));
        assert!(matches!(
            SurfaceContract::compose("maimai-main", "0.1.0", &["{"]),
            Err(super::ContractError::Json(_))
        ));
    }

    #[test]
    fn standalone_scoring_fragments_are_not_part_of_unified_surfaces()
    -> Result<(), Box<dyn std::error::Error>> {
        for (server_name, fragments, scoring) in [
            (
                "maimai-main",
                MAIN_UNIFIED,
                include_str!("../../../contracts/main/scoring.json"),
            ),
            (
                "maimai-public",
                PUBLIC_UNIFIED,
                include_str!("../../../contracts/public/scoring.json"),
            ),
        ] {
            let mut sources = fragments
                .iter()
                .map(|(_, source)| *source)
                .collect::<Vec<_>>();
            sources.push(scoring);
            let error =
                match SurfaceContract::compose(server_name, env!("CARGO_PKG_VERSION"), &sources) {
                    Ok(_) => return Err("standalone scoring fragment was accepted".into()),
                    Err(error) => error,
                };
            assert!(matches!(
                error,
                super::ContractError::DuplicateTool(name) if name == "score_counts"
            ));
        }
        Ok(())
    }

    #[test]
    fn retained_public_tools_are_a_subset_of_main() -> Result<(), Box<dyn std::error::Error>> {
        let main = parse_all(MAIN)?;
        let public = parse_all(PUBLIC)?;
        let main_tools = unique_tools(&main);
        let public_tools = unique_tools(&public);
        assert!(public_tools.is_subset(&main_tools));
        Ok(())
    }

    #[test]
    fn main_score_source_schema_exposes_lxns_without_enabling_it_in_public()
    -> Result<(), Box<dyn std::error::Error>> {
        let main = SurfaceContract::parse(include_str!("../../../contracts/main/scores.json"))?;
        let public = SurfaceContract::parse(include_str!("../../../contracts/public/scores.json"))?;
        for tool_name in ["switch_score_source", "switch_b50_source"] {
            let main_tool = main
                .tools()
                .iter()
                .find(|tool| tool.name() == tool_name)
                .ok_or("main score-source tool missing")?;
            assert_eq!(
                main_tool.input_schema()["properties"]["source"]["enum"],
                json!(["local", "sy", "lxns"])
            );
            assert!(
                public.tools().iter().all(|tool| tool.name() != tool_name),
                "public must not expose the main-only score-source switch tool"
            );
        }
        Ok(())
    }
}
