# MCP 冻结契约

本目录保存 `maimai-main` 与 `maimai-public` 的用户可观察协议表面。每个 fragment 包含 `serverInfo` 与按固定顺序排列的 `tools`，工具项冻结：

- 名称；
- 描述；
- 输入 JSON Schema；
- 在最终 surface 中的顺序。

运行时把 fragment 解析为类型化 `SurfaceContract`；业务 handler 仍使用 Rust DTO，不把 contract 当作动态业务模型。

## 最终 surface

### `maimai-main`：78 项

组合顺序固定为：

| fragment | 数量 | 范围 |
| --- | ---: | --- |
| `main/b50_image.json` | 3 | B50 图片兼容入口与样式。 |
| `main/catalog.json` | 14 | 曲库、别名、刷新、随机、版本、定数历史、计分。 |
| `main/identity.json` | 5 | QQ 身份。 |
| `main/rankings.json` | 11 | 群 B50 与群单曲排行。 |
| `main/render.json` | 16 | 主绘图与两个 Upper 工具。 |
| `main/score_query.json` | 1 | 玩家单曲成绩路由。 |
| `main/scores.json` | 12 | B50、水鱼 API、Developer-Token、成绩源切换。 |
| `main/update.json` | 16 | 落雪 OAuth 6 项与 main 官服扩展 10 项。 |

main 的 dispatcher 分区必须同时满足：

```text
core = 66
extension = 12
交集 = 0
并集 = contract 中的 78 个工具
```

### `maimai-public`：60 项

组合顺序固定为：

| fragment | 数量 | 范围 |
| --- | ---: | --- |
| `public/catalog.json` | 13 | 公开曲库与计分。 |
| `public/identity.json` | 5 | QQ 身份。 |
| `public/oauth.json` | 6 | 落雪 OAuth 生命周期。 |
| `public/rankings.json` | 11 | 群排行。 |
| `public/render.json` | 14 | 公开绘图。 |
| `public/score_query.json` | 1 | 玩家单曲成绩路由。 |
| `public/scores.json` | 10 | B50、水鱼 API 与 Developer-Token。 |

`main` 覆盖 `public` 的全部 60 个工具名并额外提供 18 个名字，但两个 surface 分别组合自己的 fragment；30 个同名工具的描述或输入 schema 不同，所以不能把 public fragment 当成 main fragment 的直接子集。public 不通过黑名单或运行时隐藏来排除私有工具。

## `scoring.json`

`main/scoring.json` 与 `public/scoring.json` 各冻结 2 个独立计分工具，供独立 contract/兼容测试使用。最终 `maimai-main` 和 `maimai-public` 已在 `catalog.json` 中包含同一对工具，因此组合最终 surface 时不再追加 `scoring.json`；否则会错误得到 80/62 项并产生重复名称。

## 测试要求

契约测试必须逐字段比较实际 `tools/list` 与 fragment，不能只比较数量或名称集合。至少验证：

1. server 名称分别为 `maimai-main` / `maimai-public`；
2. 工具总数精确为 78/60；
3. 工具顺序、名称、描述和输入 schema 完全一致；
4. 每个工具恰好由一个 dispatcher 分区处理；
5. 未知工具返回协议错误，不落入兜底业务分支。

AstrBot 安装器也会在改配置前真实启动 staged `maimai-main`，执行 `initialize` 和 `tools/list`，并与 main 的 8 个 fragment 精确比较。

## 修改规则

允许修改冻结契约的情况只有两种：

1. 已确认旧行为属于 bug，并用回归测试锁定修正后的结果；
2. 明确变更 MCP 公共接口，并同步 README、Agent prompt 与中文 CHANGELOG。

内部 crate 拆分、状态迁移、性能优化或绘图重构本身不是改 contract 的理由。
