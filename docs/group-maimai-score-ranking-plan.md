# 群 maimai 成绩排行榜 MCP 计划

## 可行性结论

可行。

当前仓库已经登记了 Diving-Fish 的 maimai 完整成绩开发者接口：

- operation：`maimai_dev_player_records_get`
- 方法和路径：`GET /api/maimaidxprober/dev/player/records`
- 鉴权：`Developer-Token`
- 位置：`diving_fish_b50_mcp/api_catalog.py`

现有 `diving_fish_api` 已经可以在绑定 Developer-Token 后调用这个接口，但它是通用 API 调用器，不适合直接给群排行榜编排 MCP 使用。更合适的做法是保留现有 `query_maimai_song_score`，再在水鱼 MCP 里开放曲目元数据工具、结构化完整成绩查询工具和批量完整成绩查询工具，最后由群排行榜 MCP 调用这些工具生成群榜。

## 推荐架构

1. 已将现有 MCP serverInfo name 从 `group-b50-mcp` 改名为 `group-rank-mcp`（Python 包名 `group_b50_mcp/`、工具名 `group_b50_*`、环境变量 `GROUP_B50_*`、缓存目录 `group-b50-cache/` 都保留原样，避免破坏现有 AstrBot 配置）。
2. 在 `diving_fish_b50_mcp` 增加曲目元数据工具 `get_maimai_song_data`。
3. 在 `diving_fish_b50_mcp` 增加单人完整成绩查询工具。
4. 在 `diving_fish_b50_mcp` 增加批量完整成绩查询工具，形态参考现有 `query_b50_batch`。
5. 在 `group-rank-mcp` 增加群成绩排行榜工具（新工具采用独立命名前缀，如 `group_song_score_*`）。
6. 复用现有群 B50 榜的群成员获取、后台刷新任务、stdio 子 MCP 调用、进度写入和一天缓存机制；新增成绩榜使用独立缓存目录如 `group-song-cache/`。

这样 token 处理、水鱼错误映射、曲目元数据、响应标准化、重试和单人查询都留在水鱼 MCP 里；群 rank MCP 只负责群成员、缓存、过滤、排序和输出。

## MCP 改名范围（已完成）

现有 `group_b50_mcp` 实际已经不只做 B50：它负责群成员、身份缓存、群内 ranking、后台任务和缓存文件。MCP serverInfo name 已从 `group-b50-mcp` 调整为 `group-rank-mcp`。

实际改动范围（已最小化以避免破坏现有部署）：

- ✅ MCP serverInfo name：`group-b50-mcp` -> `group-rank-mcp`
- ✅ README 和 MCP 客户端配置示例
- ✅ agent prompt 里的工具归属说明
- 🚫 Python 模块/包名：保持 `group_b50_mcp/` 不变（破坏面太大）
- 🚫 环境变量、缓存目录：保持 `GROUP_B50_*` / `group-b50-cache/` 不变
- 🚫 现有 B50 工具名：保持 `group_b50_report`、`group_b50_member_rank` 等不变；新增成绩榜工具使用独立命名前缀

## 第一版范围

第一版建议先支持“指定谱面”的群内成绩排行榜：

- 必填：`groupId`
- 必填：`musicId`
- 可选：`levelIndex`
- 可选：`songType`
- 可选：`sortOrder`，默认 `desc`
- 可选：`outputLimit`
- 可选：`forceRefresh`
- 可选：和群 B50 一致的查询控制参数：`timeoutMs`、`queryDelayMs`、`maxConcurrency`、`batchSize`、`maxMembers`

默认排序规则：

1. `achievements` 降序
2. `dxScore` 降序
3. `ra` 降序
4. `userId` 升序，作为稳定兜底排序

第一版先直接接收 `musicId`。曲名/别名检索可以在后续版本复用 `maimai_score_mcp` 或 `maimai-local-search` 再加。

## 曲目元数据工具

建议新增 `get_maimai_song_data`，用于查询和缓存歌曲元数据。它不查询玩家成绩，只负责把 `musicId` 解析成曲名、类型、难度、等级和定数等信息。

底层数据来源：

- operation：`maimai_music_data_get`
- 方法和路径：`GET /api/maimaidxprober/music_data`
- 鉴权：无

建议输入：

```json
{
  "musicId": 11466,
  "includeCharts": true
}
```

建议输出：

```json
{
  "musicId": 11466,
  "title": "歌曲名",
  "artist": "曲师",
  "type": "DX",
  "basicInfo": {},
  "charts": [
    {
      "levelIndex": 3,
      "levelLabel": "Master",
      "level": "13+",
      "ds": 13.8
    }
  ]
}
```

群成绩排行榜 MCP 使用它来：

- 校验 `musicId` 是否存在
- 在输出里显示曲名、类型和谱面信息
- 根据 `levelIndex` 或 `songType` 辅助过滤目标谱面
- 为后续曲名/别名入口预留能力

## 缓存策略

群成绩缓存应和 B50 群榜保持一致：

- 默认 TTL：1 天（与现有群 B50 缓存保持一致）
- 缓存不存在、过期或 `forceRefresh` 时，启动后台刷新任务并立即返回
- 缓存内容：可查询群成员的完整 maimai 成绩
- 查不到、隐私、无可用成绩的成员跳过
- 如果仍存在网络、限流或服务端 5xx 等临时失败，不写入成功缓存，避免缓存残缺群榜

建议缓存路径：

```text
GROUP_MAIMAI_SCORE_CACHE_DIR/<groupId>/cache.json
```

如果没有配置单独环境变量，默认路径可以是：

```text
group-maimai-score-cache/<groupId>/cache.json
```

## 计划表

| 状态 | 步骤 | 内容 |
| --- | --- | --- |
| ☑ | 1. 定义改名范围 | 已完成：仅修改 serverInfo name `group-b50-mcp` -> `group-rank-mcp`；Python 包名/工具名/环境变量/缓存目录全部保留以维持现有 AstrBot 配置。 |
| ☐ | 2. 定义排行榜契约 | 确定第一版输入/输出 schema、排序规则、缓存结构和错误语义。 |
| ☐ | 3. 增加曲目元数据工具 | 在 `diving_fish_b50_mcp` 增加 `get_maimai_song_data`，基于 `maimai_music_data_get` 返回标准化歌曲和谱面信息。 |
| ☐ | 4. 增加单人完整成绩工具 | 在 `diving_fish_b50_mcp` 增加 `query_maimai_records`，调用 `maimai_dev_player_records_get` 并返回标准化结构。 |
| ☐ | 5. 增加批量完整成绩工具 | 增加 `query_maimai_records_batch`，支持 `qqs`、`timeoutMs`、`queryDelayMs`、`maxConcurrency`、`groupId`。 |
| ☐ | 6. 补水鱼 MCP 测试 | 覆盖曲目元数据、token 绑定、单人完整成绩、批量部分失败、鉴权错误和临时失败。 |
| ☑ | 7. 改名 group rank MCP | 已完成：serverInfo name 改为 `group-rank-mcp`，Python 包名 `group_b50_mcp/` 保留，无需兼容 shim。 |
| ☐ | 8. 增加群成绩榜工具 | 在 group rank MCP 增加群成绩排行榜工具，复用群成员加载并通过 stdio 调 `query_maimai_records_batch` 和 `get_maimai_song_data`。 |
| ☐ | 9. 增加一天群成绩缓存 | 保存全群完整成绩，支持 B50 式 TTL、后台刷新、job status、过期和强制刷新。 |
| ☐ | 10. 生成排行榜输出 | 按 `musicId`/谱面过滤缓存成绩，排序并输出 Markdown 表和 structuredContent。 |
| ☐ | 11. 更新文档和 changelog | 记录工具参数、缓存行为、改名兼容策略、示例和实现注意事项。 |
| ☐ | 12. 提交实现 | 跑相关测试，只暂存目标文件并提交。 |

后续正式开始实现时，每完成一项就把对应状态从 `☐` 改成 `☑`。

## 注意事项

- 这个功能依赖有效的 Diving-Fish Developer-Token。
- `get_maimai_song_data` 不依赖 Developer-Token，但完整成绩查询依赖。
- 完整成绩数据量明显大于 B50，整群同步查询容易超时，必须走后台刷新。
- group rank MCP 不应该直接暴露或持久化 token 明文。
- 后续可以增加曲名/别名入口，但第一版先用 `musicId` 能降低实现风险。
