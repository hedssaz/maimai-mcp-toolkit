use super::{DispatchError, ToolCall, ToolDispatcher, ToolOutput};

/// 在 composition root 中把两个互不重叠的工具处理器组成一个 MCP surface。
///
/// 工具存在性仍由 `ContractServer` 的冻结契约校验；这里只决定一个已知工具
/// 应交给哪一个处理器。线性扫描适合当前十几个工具的固定小集合，也避免为每个
/// server 建立额外的动态注册表或对象容器。
pub struct PairDispatcher<L, R> {
    left_tools: &'static [&'static str],
    left: L,
    right: R,
}

impl<L, R> PairDispatcher<L, R> {
    pub const fn new(left_tools: &'static [&'static str], left: L, right: R) -> Self {
        Self {
            left_tools,
            left,
            right,
        }
    }
}

impl<L, R> ToolDispatcher for PairDispatcher<L, R>
where
    L: ToolDispatcher,
    R: ToolDispatcher,
{
    async fn dispatch(&self, call: ToolCall) -> Result<ToolOutput, DispatchError> {
        if self
            .left_tools
            .iter()
            .any(|tool_name| *tool_name == call.name())
        {
            self.left.dispatch(call).await
        } else {
            self.right.dispatch(call).await
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };

    use serde_json::Map;

    use super::PairDispatcher;
    use crate::{DispatchError, ToolCall, ToolDispatcher, ToolOutput};

    struct CountingDispatcher {
        calls: Arc<AtomicUsize>,
        label: &'static str,
    }

    impl ToolDispatcher for CountingDispatcher {
        async fn dispatch(&self, _call: ToolCall) -> Result<ToolOutput, DispatchError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            Ok(ToolOutput::text(self.label))
        }
    }

    #[tokio::test]
    async fn routes_only_declared_left_tools_and_supports_nesting()
    -> Result<(), Box<dyn std::error::Error>> {
        static FIRST_TOOLS: &[&str] = &["first"];
        static SECOND_TOOLS: &[&str] = &["second"];
        let first_calls = Arc::new(AtomicUsize::new(0));
        let second_calls = Arc::new(AtomicUsize::new(0));
        let fallback_calls = Arc::new(AtomicUsize::new(0));
        let dispatcher = PairDispatcher::new(
            FIRST_TOOLS,
            CountingDispatcher {
                calls: Arc::clone(&first_calls),
                label: "first",
            },
            PairDispatcher::new(
                SECOND_TOOLS,
                CountingDispatcher {
                    calls: Arc::clone(&second_calls),
                    label: "second",
                },
                CountingDispatcher {
                    calls: Arc::clone(&fallback_calls),
                    label: "fallback",
                },
            ),
        );

        for (name, expected) in [
            ("first", "first"),
            ("second", "second"),
            ("contract-validated-fallback", "fallback"),
        ] {
            let output = dispatcher
                .dispatch(ToolCall::new(name.to_owned(), Map::new()))
                .await?;
            let (content, structured) = output.into_parts();
            assert_eq!(structured, None);
            assert_eq!(serde_json::to_value(content)?[0]["text"], expected);
        }
        assert_eq!(first_calls.load(Ordering::Relaxed), 1);
        assert_eq!(second_calls.load(Ordering::Relaxed), 1);
        assert_eq!(fallback_calls.load(Ordering::Relaxed), 1);
        Ok(())
    }
}
