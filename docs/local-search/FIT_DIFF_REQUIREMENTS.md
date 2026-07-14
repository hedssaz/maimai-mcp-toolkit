# 拟合定数与区服筛选改造需求

整理时间：2026-05-27

## 背景

本项目当前已经是多数据源版本：

- 主曲库：`data/lxns_song_list.json`
- 主别名：`data/lxns_alias_list.json`
- JP/国际补充曲库：`data/maidb_jp_songs.json`
- 补充别名：`data/music_alias.json`、`data/aliases.csv`
- 自定义别名：`data/custom_aliases.json`

当前 `maimai_mcp/search.py` 会在 `load_music_data()` 中按标题合并 LXNS/CN 与 MaiDB/JP 数据，并在返回中保留：

- `source`
- `source_labels`
- `source_fields.cn`
- `source_fields.jp`
- `regions`
- `matched_charts[].source`

因此后续改造应继续基于现有多数据源合并逻辑，不应回退到旧的单源脚本。

## 执行记录要求

后续按本文档实现时，所有待做事项必须维护为 checkbox 清单。每完成一个功能、修复、验证或部署步骤，都要在同一次变更里把对应条目从 `[ ]` 改成 `[x]`；如果实现过程中拆出了新的子任务，也要先补进清单再继续做。

## 优先级 0：MCP 输出改成易读文本

当前 MCP tool result 默认把完整 JSON 直接写入 `content[0].text`。在 AstrBot / Agent 日志中会变成很长的结构化对象，不利于主 Agent 判断“查到了什么”和“下一步该怎么调用”。因此在做拟合定数功能前，应先改 MCP 输出层。

要求：

- MCP 默认返回面向 Agent 的可读摘要文本，不再默认展开完整 JSON。
- 保留原始结构化结果能力：通过 `format=json`、`include_raw=true` 或 `debug=true` 之类的显式参数返回完整 JSON，方便排查问题。
- HTTP API 可以继续保持 JSON；这次优先改 AstrBot 调用 MCP 时看到的 tool result。
- `search_maimai_songs` / `random_maimai_songs` / `list_maimai_songs_by_id` 输出应包含：
  - 命中数量、是否截断、查询条件摘要。
  - 每首歌一行主信息：`ID | 曲名 | 艺术家 | 来源 | 区服 | 版本 | BPM`。
  - 谱面信息按行列出：`难度 | 类型 | 等级 | 实际定数 | 拟合定数 | 差值 | 虚高/虚低 | Note 数 | 来源`。
  - 多源字段差异只显示重点，例如 `CN ds: 7.7/11.5/...；JP ds: 7.6/11/...`，不要默认展开完整 `source_fields`。
  - 别名默认只显示前若干个，例如前 8 个，并提示还有多少个。
- `score_counts` 输出应直接列出 `oldscore`、`oldacc`、`dxscore`、`dxacc`，不要让 Agent 从 JSON 字段里找。
- `find_score_combinations` 输出应列出筛选目标、命中组合数量、组合摘要；超限时用自然语言说明该收紧什么条件。
- `add_maimai_alias`、`list_maimai_versions` 等简单工具也应返回短文本。
- 错误结果使用简短自然语言说明，不直接抛大段 JSON。
- MCP 可读输出完成后，需要同步更新 `AGENT_MCP_GUIDE.md`：取消默认按 JSON 字段解析结果的说明，改为默认完整转述 MCP 可读文本；raw JSON 仅用于调试或用户明确要求。

建议实现：

- 在 `maimai_mcp/server.py` 增加统一 formatter，避免每个 handler 重复 `json.dumps`。
- 为不同工具提供独立格式化函数，例如：
  - `format_search_result`
  - `format_random_result`
  - `format_id_list_result`
  - `format_versions_result`
  - `format_score_counts_result`
  - `format_find_score_result`
- `result_content()` 根据参数决定返回可读文本或原始 JSON。
- Skill / Agent 指南同步说明：默认直接读 MCP 的文本结果；只有调试或需要完整字段时才请求 raw JSON。

## 新数据源

需要加入 Diving-Fish 谱面统计接口：

- URL: `https://www.diving-fish.com/api/maimaidxprober/chart_stats`
- 返回包含 `charts` 和 `diff_data`。
- `charts` 是以 `song_id` 为 key 的谱面统计字典；每首歌下按难度保存统计项。
- 关键字段：
  - `fit_diff`: 拟合定数
  - `cnt`: 样本数量
  - `diff`: 官标等级
  - `avg`: 平均达成率
  - `avg_dx`: 平均 DX SCORE
  - `std_dev`: 达成率标准差
  - `dist`: 评级分布
  - `fc_dist`: FC 分布

本地建议新增缓存文件：

- `data/divingfish_chart_stats.json`

## 拟合定数匹配方式

Diving-Fish `chart_stats` 只提供以乐曲 ID 为 key 的谱面统计数据，不提供完整曲库搜索能力。因此实现时不能用它替代当前曲库查询流程。

正确流程：

1. 先用现有多数据源曲库完成查歌、别名匹配、等级/定数/难度/谱面类型筛选。
2. 对每个命中的歌曲/谱面，取本地曲库结果里的乐曲 ID。
3. 用标准化后的乐曲 ID 去 `data/divingfish_chart_stats.json` 的 `charts` 中查对应统计项。
4. 按谱面难度顺序或可验证的谱面键匹配到具体谱面。
5. 将 `fit_diff`、`fit_delta`、`fit_label` 和可选统计字段附加到 `matched_charts[]`，再返回给 MCP/HTTP/前端。

拟合定数相关筛选也应遵循这个流程：先从当前曲库枚举候选谱面，再用乐曲 ID 附加拟合定数并进行 `fit_diff` / `fit_delta` / `fit_label` 筛选。不能只在 `chart_stats` 里查出 ID 后直接返回，因为那样缺少曲名、别名、区服、多源字段和谱面详情。

## 拟合定数字段

每个谱面结果需要补充：

- `fit_diff`: 拟合定数
- `fit_delta`: 实际定数 `ds` - 拟合定数 `fit_diff`
- `fit_label`: `虚高` / `虚低` / 可选空值

定义必须使用：

```text
fit_delta = ds - fit_diff
fit_delta > 0 => 虚高
fit_delta < 0 => 虚低
fit_delta = 0 => 不打标签或返回中性值
```

查询、推荐、随机歌曲时都要显示：

- 实际定数
- 拟合定数
- 实际定数与拟合定数的差值
- 虚高 / 虚低 标签

## 拟合定数查询规则

### 单值查询

拟合定数单值查询要按一位小数向下取整分桶。

例：

```text
用户查询 13.1
实际筛选：13.10 <= fit_diff < 13.20
```

如果用户传入 `13.17`，也应先向下取到 `13.1` 桶，再查：

```text
13.10 <= fit_diff < 13.20
```

### 范围查询

拟合定数范围查询不要取整，按用户给出的精确范围筛选。

例：

```text
fit_diff_min = 13.13
fit_diff_max = 13.27
筛选：13.13 <= fit_diff <= 13.27
```

## 需要新增的筛选能力

### 搜索

`search_maimai_songs` 需要新增：

- 拟合定数单值查询
- 拟合定数范围查询
- 实际定数范围查询继续保留现有 `ds` / `ds_min` / `ds_max`
- 按 `fit_label` 查询：`虚高` / `虚低`
- 按 `fit_delta` 差值范围查询
- 按 `fit_delta` 排序
- 按等级选出最虚高歌曲
- 按等级选出最虚低歌曲
- 区服存在 / 不存在筛选

### 随机

`random_maimai_songs` 需要新增同样的筛选能力：

- 实际定数范围
- 拟合定数单值 / 范围
- `fit_label`
- `fit_delta` 范围
- 区服存在 / 不存在

只要使用任意谱面级筛选条件，随机逻辑应进入 chart-filtered 模式。

### ID 列表

`list_maimai_songs_by_id` 如继续作为通用列表工具，也应支持：

- 拟合定数筛选
- `fit_label`
- `fit_delta`
- 区服筛选

## 区服筛选

区服字段统一使用：

- `jp`
- `intl`
- `usa`
- `cn`

需要支持中文别名：

- 日服 -> `jp`
- 国服 -> `cn`
- 国际服 -> `intl`
- 美服 -> `usa`

筛选需要支持：

- 某首歌 / 谱面在指定区服有
- 某首歌 / 谱面在指定区服没有

建议参数：

- `region_has`: 单个或多个区服，要求存在
- `region_missing`: 单个或多个区服，要求不存在

实现注意：

- `regions` 可能来自 MaiDB/JP 谱面字段。
- `regions` 也可能来自补充曲库，不一定在主曲库顶层。
- 多数据源合并后再做区服筛选，不能只看单一源。
- 如果 LXNS/CN 源没有显式 `regions.cn`，但谱面来自 `source=cn`，可以考虑在合并层推断 `cn: true`，否则国服筛选可能漏掉主曲库谱面。

## 排序与推荐

需要新增排序参数，建议：

- `sort = fit_delta_desc`: 最虚高优先
- `sort = fit_delta_asc`: 最虚低优先
- `sort = fit_diff_asc`
- `sort = fit_diff_desc`
- 保留默认排序逻辑

“某个等级里最虚高的歌曲”可转成：

```json
{
  "level": "13+",
  "sort": "fit_delta_desc",
  "limit": 1
}
```

“某个等级里最虚低的歌曲”可转成：

```json
{
  "level": "13+",
  "sort": "fit_delta_asc",
  "limit": 1
}
```

## 更新脚本

需要新增一键更新所有数据源的本地脚本。

建议新增：

- `scripts/update_all_data.py`

职责：

1. 更新 LXNS 曲库和别名。
2. 更新 MaiDB JP/INTL 补充曲库。
3. 更新 Diving-Fish chart stats。
4. 重新生成所有派生文件。

现有脚本：

- `scripts/update_music_data.py`: 更新 LXNS 曲库和别名。
- `scripts/update_maidb_jp_data.mjs`: 更新 MaiDB 曲库。

需要新增：

- `scripts/update_chart_stats.py` 或并入 `update_all_data.py`

更新策略：

- 数据源更新时，旧信息有变化的地方应以新数据为准。
- 自定义别名 `data/custom_aliases.json` 不能被覆盖。
- 更新后要自动重新生成派生文件，例如 skill 包、可能新增的索引/合并缓存。

## MCP / HTTP / Web 需要同步

需要同步更新：

- MCP tool result 可读文本 formatter 需要先做，避免后续新增拟合定数字段后输出更难读。
- `maimai_mcp/search.py`
- `maimai_mcp/server.py`
- `maimai_mcp/http_server.py`
- `web/app.js`
- `web/index.html`
- `web/styles.css` 如需要新控件样式
- `README.md`
- `AGENT_MCP_GUIDE.md`
- 旧的 packaged Skill 文件已经按前序要求移除；本轮只维护 `AGENT_MCP_GUIDE.md`。

## 已经具备的能力

当前工作区已经具备：

- 多数据源读取。
- 同标题 LXNS/CN 与 MaiDB/JP 合并。
- `source_fields.cn` / `source_fields.jp` 分源字段输出。
- `regions` 聚合输出的基础逻辑。
- 实际定数 `ds` 单值和范围筛选。
- 搜索、随机、ID 列表都可用实际定数范围。
- 单独的 LXNS 更新脚本。
- 单独的 MaiDB 更新脚本。

## 尚未实现的能力

需要实际编码实现：

- [x] MCP 默认易读文本输出。
- [x] MCP raw JSON 显式开关。
- [x] MCP 可读输出完成后同步更新 `AGENT_MCP_GUIDE.md`，移除以 JSON 字段读取为默认的说明。
- [x] 下载并缓存 Diving-Fish `chart_stats`。
- [x] 将 `fit_diff` 合并到每个谱面。
- [x] 输出 `fit_diff` / `fit_delta` / `fit_label`。
- [x] 拟合定数单值分桶查询。
- [x] 拟合定数范围查询。
- [x] `fit_delta` 范围筛选。
- [x] `fit_delta` 排序。
- [x] 虚高 / 虚低筛选。
- [x] 区服 has / missing 筛选。
- [x] 区服中文别名。
- [x] 一键更新所有数据源脚本。
- [x] 更新后自动生成派生文件。
- [x] MCP schema、HTTP API、前端图形面板、README、AGENT_MCP_GUIDE 同步。
- [x] 服务器部署。

## 测试要求

需要补充或至少手动验证：

- [x] `search_maimai_songs(query="潘")` 默认返回易读摘要，不再输出完整 JSON。
- [x] `search_maimai_songs(query="潘", format="json")` 或等效 raw 参数仍可返回完整结构化结果。
- [x] 多结果时默认显示曲名列表和关键字段，不默认展开全部别名与 `source_fields`。
- [x] 单曲结果的谱面行显示实际定数、拟合定数、差值、虚高/虚低、Note 数和来源。
- [x] `fit_diff=13.1` 实际筛选 `[13.10, 13.20)`。
- [x] `fit_diff_min=13.13, fit_diff_max=13.27` 不取整。
- [x] `fit_delta = ds - fit_diff`。
- [x] `fit_delta > 0` 输出 `虚高`。
- [x] `fit_delta < 0` 输出 `虚低`。
- [x] 搜索虚高 / 虚低。
- [x] 随机虚高 / 虚低。
- [x] 按差值排序。
- [x] 某等级最虚高 / 最虚低。
- [x] `region_has=jp`、`region_missing=jp`。
- [x] 中文区服别名：日服、国服、国际服、美服。
- [x] 多数据源合并后仍能筛选区服。
- [x] 更新脚本不会覆盖 `custom_aliases.json`。
- [x] 更新脚本会刷新派生文件。

## 风险点

- Diving-Fish `chart_stats` 的 `song_id` 与 LXNS/MaiDB ID 可能存在 DX ID / 标准化 ID 差异，需要用实际样本验证匹配策略。
- MaiDB 与 LXNS 的同曲不同标题、别名、大小写和符号差异可能导致无法合并，需要保持现有标题合并逻辑并考虑 ID 辅助匹配。
- CN 区服可能没有显式 `regions.cn`，需要决定是否根据 `source=cn` 推断。
- 如果谱面没有 `fit_diff`，筛选拟合定数时应排除；普通查询时仍可返回但字段为空。
- 一键更新脚本涉及 Node 与 Python 两种运行时，服务器 Docker 镜像需要确认 Node 是否可用。
