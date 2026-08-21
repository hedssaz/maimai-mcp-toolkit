# 项目开发规范

本文件只描述公开版 `maimai` Rust 工作区的工程约束。

## 项目边界

- 使用 Rust 1.94、Rust 2024 edition。
- 最终运行入口只有 `maimai-public`，通过一个 stdio 连接提供 60 个工具。
- `contracts/public/` 是对外合同；工具名称、顺序、描述和输入 Schema 必须逐字段校验。
- 不引入官服登录、成绩导入、Region、Upper、拆分 MCP 进程、跨 MCP 调用或 Python MCP 子进程。
- `cargo tree -p maimai-stdio --edges normal` 不得出现 `maimai-extended`。

## 分层职责

- `maimai-core`：领域类型和纯算法。
- `maimai-catalog`：多来源曲库归一、索引和查询。
- `maimai-storage`：公共 SQLite 状态与原子持久化。
- `maimai-providers`：NapCat、水鱼和落雪 HTTP 边界。
- `maimai-render`：纯绘图与资源解析。
- `maimai-app`：应用用例和生命周期。
- `maimai-mcp`：合同、DTO、handler 与错误映射。
- `maimai-stdio`：Public composition root 与进程生命周期。

依赖只向领域和基础设施内层流动，不增加通用服务定位器、动态 DI 或跨层 re-export。

## 实现原则

- 保留有效需求和用户可观察行为，不复刻旧 Python 结构。
- 使用强类型表达 QQ、歌曲/谱面 ID、来源、难度、达成率、FC/FS 和任务状态。
- 生产路径禁止 `unwrap()`、`expect()`、`panic!()`、`todo!()` 和 `unimplemented!()`。
- 错误不得泄漏 token、响应正文、数据库路径或内部堆栈。
- 外部客户端复用连接池，并限制 URL、重定向、超时、响应体和分页。
- 不持有数据库事务或互斥锁跨越网络 `await`。
- 文件和快照发布必须原子失败，刷新失败时保留旧快照。
- 模块按职责拆分，避免只有转发和 re-export 的碎片层。

## 交付门禁

```bash
cargo +1.94.0 fmt --all -- --check
CARGO_INCREMENTAL=0 cargo +1.94.0 check --locked --workspace --all-targets
CARGO_INCREMENTAL=0 cargo +1.94.0 test --locked -p maimai-stdio --all-targets
CARGO_INCREMENTAL=0 cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings
git diff --check
```

还必须真实启动 `maimai-public`，精确列出并路由 60 个工具；公开状态库不得创建官服私有表。

## 仓库卫生

- MCP stdout 只写协议帧，诊断写 stderr且脱敏。
- 不提交数据库、WAL/SHM、图片输出、缓存、凭据、原始成绩 dump、`.venv` 或 `target/`。
- Yuzu 大型字体和图片资源由使用者单独下载，不提交进公开 Git 历史。
- 用户可见合同、部署或兼容行为变化时同步更新中文 `CHANGELOG.md` 与 `README.md`。
