use rmcp::model::Tool;

use crate::contract::SurfaceContract;

/// 逐字复用冻结契约中的描述与 JSON Schema，不经过 schemars 生成或规范化。
pub fn tools_from_contract(contract: &SurfaceContract) -> Vec<Tool> {
    contract
        .tools()
        .iter()
        .map(|tool| {
            Tool::new(
                tool.name().to_owned(),
                tool.description().to_owned(),
                tool.input_schema().clone(),
            )
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use serde_json::{Value, json};

    use super::tools_from_contract;
    use crate::contract::SurfaceContract;

    #[test]
    fn preserves_frozen_schema_without_schemars_rewrite() -> Result<(), Box<dyn std::error::Error>>
    {
        let source = json!({
            "serverInfo": {"name": "fixture", "version": "1.0.0"},
            "tools": [{
                "name": "inspect",
                "description": "Inspect the fixture.",
                "inputSchema": {
                    "type": "object",
                    "properties": {
                        "target": {
                            "oneOf": [{"type": "integer"}, {"type": "string"}]
                        }
                    },
                    "required": ["target"],
                    "additionalProperties": false
                }
            }]
        });
        let contract = SurfaceContract::parse(&serde_json::to_string(&source)?)?;
        let tools = tools_from_contract(&contract);
        let serialized = serde_json::to_value(&tools)?;

        assert_eq!(
            serialized[0]["inputSchema"],
            source["tools"][0]["inputSchema"]
        );
        assert_eq!(serialized[0].get("title"), None);
        assert_eq!(serialized[0].get("outputSchema"), None);
        assert_eq!(serialized[0]["name"], Value::String("inspect".to_owned()));
        Ok(())
    }
}
