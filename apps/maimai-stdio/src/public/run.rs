use maimai_mcp::{
    ContractServer, StdioServerError,
    contract::{ContractError, SurfaceContract},
    serve_stdio,
};
use thiserror::Error;

use super::{
    config::{PublicConfig, PublicConfigError},
    services::{PublicServiceError, PublicServices},
    surfaces::{CompositionError, LifecycleError, compose},
};

const PUBLIC_FRAGMENTS: [&str; 7] = [
    include_str!("../../../../contracts/public/catalog.json"),
    include_str!("../../../../contracts/public/identity.json"),
    include_str!("../../../../contracts/public/oauth.json"),
    include_str!("../../../../contracts/public/rankings.json"),
    include_str!("../../../../contracts/public/render.json"),
    include_str!("../../../../contracts/public/score_query.json"),
    include_str!("../../../../contracts/public/scores.json"),
];

pub async fn run_public() -> Result<(), PublicProcessError> {
    let contract = public_contract()?;
    let config = PublicConfig::from_env()?;
    let services = PublicServices::open_public(config).await?;
    let composition = compose(services)?;
    let server = ContractServer::new(contract, composition.dispatcher);
    let stdio = serve_stdio(server).await;
    let shutdown = composition.lifecycle.shutdown().await;
    let _reason = stdio?;
    shutdown?;
    Ok(())
}

fn public_contract() -> Result<SurfaceContract, PublicProcessError> {
    let contract = SurfaceContract::compose(
        "maimai-public",
        env!("CARGO_PKG_VERSION"),
        &PUBLIC_FRAGMENTS,
    )?;
    if contract.tools().len() != 60 {
        return Err(PublicProcessError::UnexpectedToolCount(
            contract.tools().len(),
        ));
    }
    Ok(contract)
}

#[derive(Debug, Error)]
pub enum PublicProcessError {
    #[error("public MCP 合同无效：{0}")]
    Contract(#[from] ContractError),
    #[error("public MCP 合同应精确包含 60 个工具，实际为 {0}")]
    UnexpectedToolCount(usize),
    #[error("public MCP 配置无效：{0}")]
    Config(#[source] Box<PublicConfigError>),
    #[error("public MCP 服务初始化失败：{0}")]
    Service(#[source] Box<PublicServiceError>),
    #[error("public MCP surface 组合失败：{0}")]
    Composition(#[source] Box<CompositionError>),
    #[error("public MCP stdio 运行失败：{0}")]
    Stdio(#[from] StdioServerError),
    #[error("public MCP 关闭失败：{0}")]
    Lifecycle(#[source] Box<LifecycleError>),
}

impl From<PublicConfigError> for PublicProcessError {
    fn from(value: PublicConfigError) -> Self {
        Self::Config(Box::new(value))
    }
}

impl From<PublicServiceError> for PublicProcessError {
    fn from(value: PublicServiceError) -> Self {
        Self::Service(Box::new(value))
    }
}

impl From<CompositionError> for PublicProcessError {
    fn from(value: CompositionError) -> Self {
        Self::Composition(Box::new(value))
    }
}

impl From<LifecycleError> for PublicProcessError {
    fn from(value: LifecycleError) -> Self {
        Self::Lifecycle(Box::new(value))
    }
}

#[cfg(test)]
mod tests {
    use std::error::Error;

    use super::*;

    #[test]
    fn public_contract_is_exact_ordered_sixty_tool_union() -> Result<(), Box<dyn Error>> {
        let contract = public_contract()?;
        let expected = PUBLIC_FRAGMENTS
            .iter()
            .map(|source| SurfaceContract::parse(source))
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flat_map(|fragment| {
                fragment
                    .tools()
                    .iter()
                    .map(|tool| tool.name().to_owned())
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>();
        assert_eq!(contract.server_info().name(), "maimai-public");
        assert_eq!(contract.server_info().version(), env!("CARGO_PKG_VERSION"));
        assert_eq!(contract.tools().len(), 60);
        assert_eq!(
            contract
                .tools()
                .iter()
                .map(|tool| tool.name())
                .collect::<Vec<_>>(),
            expected.iter().map(String::as_str).collect::<Vec<_>>()
        );
        assert!(expected.iter().all(|name| name != "scoring"));
        Ok(())
    }
}
