# 贡献指南

欢迎提交 Issue 和 Pull Request。

## 开发环境

- Rust 1.94
- Rust 2024 edition

```bash
git clone https://github.com/hedssaz/maimai-mcp-toolkit.git
cd maimai-mcp-toolkit
cargo +1.94.0 build --locked -p maimai-stdio --bin maimai-public
```

绘图测试需要 Yuzu 资源，目录配置见 [README.md](README.md)。

## Public 边界

- 运行入口只有 `maimai-public`，通过一个 stdio 进程提供 60 个工具。
- `contracts/public/` 是公开合同；工具名称、顺序和 Schema 需要保持兼容。
- 不加入 `maimai-main`、官服登录/导入、Region、Upper 或 Python MCP 子进程。
- `maimai-stdio` 的正常依赖树不得包含 `maimai-extended`。

## 提交前验证

```bash
cargo +1.94.0 fmt --all -- --check
CARGO_INCREMENTAL=0 cargo +1.94.0 check --locked --workspace --all-targets
CARGO_INCREMENTAL=0 cargo +1.94.0 test --locked --workspace --all-targets
CARGO_INCREMENTAL=0 cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings
git diff --check
```

涉及合同或 composition 的修改还要运行真实 stdio 回归，确认精确列出并路由 60 个工具，且公共 SQLite 不创建 Main 私有表。

## 代码约束

- 保留用户可观察行为，修复兼容差异时补回归测试。
- 业务逻辑放在对应领域或应用 crate，不放进 MCP handler。
- 生产路径使用结构化错误，不泄漏 token、响应正文、数据库路径或内部堆栈。
- 不提交数据库、WAL/SHM、缓存、输出图片、凭据、原始成绩数据、`.venv` 或 `target/`。
