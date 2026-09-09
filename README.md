# maimai Public MCP

这是 `maimai` 的公开 Rust stdio MCP。一个 `maimai-public` 进程提供 60 个工具，不再启动旧版 Python MCP 子进程。

## 能力

| 能力组 | 工具数 |
| --- | ---: |
| 曲库、搜索、别名、刷新与计分 | 13 |
| QQ 身份 | 5 |
| 群 B50 / 单曲排行 | 11 |
| B50、完成表、曲目信息、成绩与统计绘图 | 14 |
| 单曲成绩路由 | 1 |
| 水鱼成绩查询与 Developer-Token 设置 | 10 |
| 落雪 OAuth 生命周期 | 6 |
| 合计 | 60 |

公开版不包含官服登录/导入、快速官服 B50、友人对战、Region、Upper、日服专属曲库或运行时成绩源切换。`maimai-public` 的正常依赖树不包含私有扩展 crate，公共 SQLite 也不会创建官服私有表。

### 成绩与绘图兼容行为

落雪 B50 兼容非整数 `dx_rating` 和未内嵌玩家资料的响应，并按实际来源谱面区分一人、双人宴谱。静态曲绘同时匹配基础曲目编号及 `+10000` 编号。

水鱼普通谱面优先使用 `level_index` 判定难度，历史 `Utage` / `DX` 成绩由曲库确定宴谱类型。标题、昵称等展示文本原样保留，包括真实的全角空白曲名；FC/FS 的空白及大小写 `none/null/nan` 视为未标记。数值、身份和未知状态仍按既有约束校验。

## 构建

需要 Rust 1.94：

```bash
cargo +1.94.0 build --locked --release \
  -p maimai-stdio --bin maimai-public
```

产物位于 `target/release/maimai-public`。

## 数据与绘图资源

仓库内置 Public 运行所需的水鱼、落雪、别名、统计和牌子数据快照。默认数据目录为 `./data`，也可设置：

```bash
export MAIMAI_DATA_DIR=/srv/maimai/data
export MAIMAI_STATE_DB=/srv/maimai/config/maimai-local.db
```

大型 Yuzu 字体和 UI 图片不进入公开 Git 历史。请从 [Yuri-YuzuChaN/maimaiDX](https://github.com/Yuri-YuzuChaN/maimaiDX) 的资源包取得 `Resource/static`，然后设置：

```bash
export MAIMAIDX_STATIC_DIR=/srv/maimai/yuzu/Resource/static
export MAIMAIDX_COVER_CACHE_DIR=/srv/maimai/covers
export MAIMAIDX_RENDER_OUTPUT_DIR=/srv/maimai/images
```

兼容变量 `B50_IMAGE_YUZU_STATIC_DIR`、`B50_IMAGE_STATIC_DIR`、`B50_IMAGE_COVER_CACHE_DIR` 和 `B50_IMAGE_OUTPUT_DIR` 仍可使用。

## MCP 配置

```json
{
  "mcpServers": {
    "maimai-public": {
      "command": "/absolute/path/to/maimai-public",
      "cwd": "/absolute/path/to/maimai"
    }
  }
}
```

stdio 的 stdout 只输出 MCP 协议帧，诊断写入 stderr。

## 外部服务

| 变量 | 默认值/说明 |
| --- | --- |
| `NAPCAT_BASE_URL` | `http://napcat:3000/` |
| `NAPCAT_ACCESS_TOKEN` / `NAPCAT_TOKEN` | 可选 NapCat token |
| `NAPCAT_TIMEOUT_MS` | 默认 `10000` |
| `DIVING_FISH_API_BASE_URL` | `https://www.diving-fish.com/api/` |
| `DIVING_FISH_COVER_BASE_URL` | `https://www.diving-fish.com/covers/` |
| `MAIMAI_DISPLAY_UTC_OFFSET` | 默认 `+08:00` |

Provider 正式地址只接受 HTTPS；测试用回环地址可使用 HTTP。客户端禁止重定向并限制超时和响应体。

## 落雪 OAuth

未配置 OAuth 客户端时，其他 54 个工具仍可启动；需要 OAuth 的操作会返回结构化未配置错误。

```bash
export LXNS_OAUTH_CLIENT_ID=
export LXNS_OAUTH_CLIENT_SECRET=
export LXNS_OAUTH_REDIRECT_URI=
export LXNS_OAUTH_SCOPES="write_player read_user_profile read_player"
```

兼容变量 `LXNS_CLIENT_ID`、`LXNS_CLIENT_SECRET`、`LXNS_REDIRECT_URI`、`LXNS_AUTHORIZE_URL` 与 `LXNS_SCOPES` 仍受支持。

## 验证

```bash
cargo +1.94.0 fmt --all -- --check
CARGO_INCREMENTAL=0 cargo +1.94.0 check --locked --workspace --all-targets
CARGO_INCREMENTAL=0 cargo +1.94.0 test --locked -p maimai-stdio --all-targets
CARGO_INCREMENTAL=0 cargo +1.94.0 clippy --locked --workspace --all-targets -- -D warnings
```

真实 stdio 回归会逐项校验 60 个工具的名称、顺序、Schema 和路由，并确认 EOF 后数据库可重开、公共 schema 不含官服私有表。

## 架构

```text
maimai-public
    └── maimai-stdio
          └── maimai-mcp / maimai-app
                ├── maimai-catalog / maimai-core
                ├── maimai-storage
                └── maimai-providers / maimai-render
```

冻结合同位于 `contracts/public/`。共享代码中保留 main contract 的兼容测试资料，但本仓库不包含 `maimai-main` binary 或私有实现。

## 致谢

- 绘图设计和资源接口来自 [Yuri-YuzuChaN/maimaiDX](https://github.com/Yuri-YuzuChaN/maimaiDX)。
- 项目采用 [MIT License](LICENSE)，第三方许可见 [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md)。
- 参与开发前请阅读 [CONTRIBUTING.md](CONTRIBUTING.md)。
