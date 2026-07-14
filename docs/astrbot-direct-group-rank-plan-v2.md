# AstrBot Direct MCP 排名直连适配计划 v2

本文是 `rank` / `musicrank` / `divingfishrank` 的新版直连计划。v2 的核心目标是把排名命令的玩家目标规则和现有 Direct Render 绘图命令统一，避免旧计划里 QQ、username、QQ 名字前后都能放导致解析歧义。

## 设计结论

排名直连继续要求先通过 AstrBot 唤醒条件：当前适配器唤醒词、`@bot`、或 AstrBot 能识别的引用/回复 bot 消息。`rank` / `musicrank` 只在群聊直连；`divingfishrank` 群聊和私聊都可以按 Direct Render 唤醒规则直连。

玩家目标统一成三条硬规则。除 `divingfishrank` 外，群排行榜不支持任意 Diving-Fish username、QQ 名字、群名片或其它文本目标；群榜玩家只能用真实 `@用户` 或 QQ。

| 目标类型 | 规则 | 传给 MCP |
|---|---|---|
| 真实 `@用户` | 可以放在整条消息任意位置；必须来自 AstrBot 消息链 At 组件；`@bot` 只唤醒不作为目标 | 群榜和全体榜都传 `qq` |
| QQ | 只允许整条命令最前面，例如 `1000000001 rank`、`1000000001 musicrank 系ぎて`、`1000000001 divingfishrank`；`rank` 不再支持后置 QQ | 传 `qq` |
| Diving-Fish username | 只允许 `divingfishrank` 后置，例如 `divingfishrank sample_user`、`divingfishrank 123456` | 全体榜传 `username` |

同一条直连命令里最多只能有一个玩家目标。出现多个玩家目标时，直连层返回语法错误，不静默选择第一个，也不回退给 Agent。示例：`[At:A] [At:B] rank`、`[At:A] 1000000001 rank`、`1000000001 rank [At:A]`、`musicrank 系ぎて [At:A] [At:B]` 都应报错。

`<N>` 是群榜输出人数，也是个人名次模式和榜单模式的分界。`rank` / `musicrank` 不带 `<N>` 时只查个人名次：有真实 At 或前置 QQ 就查该玩家，否则查发送者本人。带 `<N>` 时只查群榜前/后 N 人，不再附加发送者或目标玩家的个人名次；榜单模式不能同时携带玩家目标。

`myrank` 不再作为直连命令实现，统一并入 `rank`。`rank` 默认只输出发送者自己的正序 B50 排名和倒序 B50 排名；`rank <N>` 才输出群 B50/rating 榜。

`mymusicrank` 不再作为直连命令实现，统一并入 `musicrank`。`musicrank <曲名/idxxx>` 默认只输出发送者自己在该曲的正序排名和倒序排名；`musicrank <曲名/idxxx> <N>` 才输出该曲群内前 N 名。

## 和普通绘图命令的差异

普通绘图命令里的后置 `username` 是水鱼 username，直接传给绘图 MCP：

```json
{"username": "sample_user"}
```

群排行榜里的玩家目标只允许传 QQ：

```json
{"qq": "1000000001"}
```

直连层不把 QQ 名字、群名片、普通文本或水鱼 username 传给群榜 MCP 做身份解析。`rank` / `musicrank` 在个人名次模式下要查别人时，必须使用真实 `@用户` 或前置 QQ。`rank` / `musicrank` 没有玩家目标时默认查发送者本人。

真实 At 的处理顺序固定为：先从整条消息链里收集并剥离玩家 At，再解析剩余文本。At 可以出现在命令前、命令后、命令字中间、曲名中间或参数中间；剥离后相邻文本直接拼接。手打的 `@xxx` 文本不是消息链 At，不会被当作玩家目标。

`divingfishrank` 不是群榜，没有群身份缓存解析。它后置的字符串就是 Diving-Fish 公开排行榜 username；纯数字也按 username 处理，不按 QQ 处理。查 QQ 必须写前置 QQ 或真实 `@用户`。

## MCP 支持边界

排名直连范围内，本计划按 MCP 真实能力划分玩家输入：

| 工具范围 | QQ / 真实 At | 后置 username | 说明 |
|---|---|---|---|
| `divingfishrank` | 间接支持 | 支持 | username 直接查公开榜；QQ/At 需要先通过 B50 查询拿到 username 再查公开榜 |
| 群 B50/rating 个人名次 | 支持前置 QQ 和真实 At | 不支持 | 个人名次只传 `qq`，不传普通文本目标 |
| 群单曲成绩个人名次 | 支持前置 QQ 和真实 At | 不支持 | 个人名次只传 `qq`；曲名之外的普通文本不作为玩家目标 |

因此，`rank sample_user` 不是玩家查询，应返回语法错误；`musicrank 系ぎて sample_user` 里的 `sample_user` 不作为玩家目标，仍属于曲目查询文本。群排行榜要查玩家只能使用 QQ 或真实 `@用户`。

## 命令解析顺序

按更长命令优先：

1. `musicrank`
2. `rank`
3. `divingfishrank`

命令字大小写不敏感。真实 At 组件先被剥离，不参与命令文本判断；因此 `[At:A]rank`、`ra[At:A]nk`、`rank[At:A]` 都等价于带目标的 `rank`。排名命令允许第一个参数贴着命令字：`rank10` 等价于 `rank 10`，`rank倒序10` 等价于 `rank 倒序 10`，`musicrank系ぎて` 等价于 `musicrank 系ぎて`。`myrank` / `mymusicrank` 不直连，旧写法交给 Agent 或帮助文案提示改用 `rank` / `musicrank <曲名/idxxx>`。

## `rank`

群 B50/rating 排名。只在群聊直连，必须能拿到当前 `groupId`。`rank` 合并原 `myrank` 的功能：无 `<N>` 时进入个人名次模式，无玩家目标默认查发送者；有真实 At 或前置 QQ 时查该 QQ。带 `<N>` 时进入榜单模式，只输出群榜，不查询或追加任何个人名次。

| 写法 | 工具 | 参数 |
|---|---|---|
| `rank` | `group_b50_member_rank` | `qq=<sender_qq>`, `outputMode="rating"`, `contextSize=3` |
| `rank 倒序` | `group_b50_member_rank` | 同上；个人输出仍同时包含正序/倒序名次，`倒序` 不触发榜单模式 |
| `[At:A] rank` / `rank [At:A]` / `ra[At:A]nk` | `group_b50_member_rank` | `qq=<A>`, `outputMode="rating"`, `contextSize=3` |
| `1000000001 rank` | `group_b50_member_rank` | `qq="1000000001"`, `outputMode="rating"`, `contextSize=3` |
| `rank <N>` / `rank<N>` | `group_b50_report` | `sortOrder="desc"`, `outputMode="rating"`, `outputLimit=N` |
| `rank 倒序 <N>` / `rank倒序<N>` | `group_b50_report` | `sortOrder="asc"`, `outputMode="rating"`, `outputLimit=N` |
| `rank <起始>-<结束>` / `rank<起始>-<结束>` | `group_b50_report` | `sortOrder="desc"`, `outputMode="rating"`, `startRank=<起始>`, `endRank=<结束>` |
| `rank 倒序 <起始>-<结束>` / `rank倒序<起始>-<结束>` | `group_b50_report` | `sortOrder="asc"`, `outputMode="rating"`, `startRank=<起始>`, `endRank=<结束>` |

`rank` 后置纯数字只允许 `1..30`，表示榜单输出人数；`rank 1000000001` 不再按 QQ 处理，应返回语法错误。`rank` 不支持后置纯数字 username。
`rank <普通文本>` 不直连为玩家查询，也不按 QQ 名字查人，应返回语法错误。`rank <N>` / `rank 倒序 <N>` 是榜单模式，不能同时带真实 At 或前置 QQ；出现 `1000000001 rank 10`、`rank @A 10` 这类玩家目标 + `<N>` 组合时返回语法错误。

个人名次模式只输出个人 B50 排名块，至少包含该玩家按高分在前的倒序 B50 排名，以及按低分在前的正序 B50 排名。当前底层 `group_b50_member_rank` 已返回 `rankDesc` / `rankAsc`，直连层可以复用一次 member-rank 调用完成。榜单模式只输出群榜列表，不追加个人名次。

## `musicrank`

群内单曲成绩榜。只在群聊直连，必须能拿到当前 `groupId`。直连层只剥离真实 At、前置 QQ、倒序和输出人数；如果剩余曲目文本是明确 ID 写法（如 `id11451` / `id 11451`，或没有输出人数歧义的纯数字 ID），直接归一成 `musicId=11451`，否则把剩余曲目文本传给 group-rank MCP 的 `songQuery`。

`songQuery` 支持曲名、别名、拼音别名和数字 ID。底层 `group_song_score_report` / `group_song_score_member_rank` 会自行调用 `maimai-local-search` 解析曲目；该搜索会加载本地别名、Yuzu 别名、dxrating 别名、自定义别名和拼音别名。直连层只做硬 ID 归一，不额外先查一次曲目。

不写难度时默认查该曲已有谱面的最高难度。直连层会把 `musicrank` 后、曲名前的第一个 `绿黄红紫白` 解析为 `levelIndex`，无论它是否和曲名之间有空格；例如 `musicrank紫白系` 和 `musicrank 紫白系` 都表示紫谱歌曲 `白系`，`musicrank 白系` 表示白谱歌曲 `系`。后置难度仍不解析，`musicrank <曲名> 紫` 交给 Agent。若目标曲只有一个难度，底层 group-rank MCP 可改用那一个难度。

`musicrank` 合并原 `mymusicrank` 的功能：无 `<N>` 时进入个人名次模式，无玩家目标默认查发送者；有真实 At 或前置 QQ 时查该 QQ。带 `<N>` 时进入榜单模式，只输出该曲群榜，不查询或追加任何个人名次。普通文本不作为玩家目标。

| 写法 | 工具 | 参数 |
|---|---|---|
| `musicrank <曲名/idxxx>` | `group_song_score_member_rank` | 曲名/别名传 `songQuery`，`idxxx` 传 `musicId`，不传 `levelIndex` 时底层自动选最高难度，`qq=<sender_qq>`, `contextSize=3` |
| `musicrank<难度><曲名>` / `musicrank <难度><曲名>` / `musicrank <难度> <曲名>` | `group_song_score_member_rank` | `绿=0`, `黄=1`, `红=2`, `紫=3`, `白=4`；难度前缀后的文本为歌曲 |
| `musicrank 倒序 <曲名/idxxx>` | `group_song_score_member_rank` | 同上；个人输出仍同时包含正序/倒序名次，`倒序` 不触发榜单模式 |
| `[At:A] musicrank <曲名>` / `musicrank [At:A] <曲名>` / `musicrank <曲名> [At:A]` / `musicrank 系[At:A]ぎて` | `group_song_score_member_rank` | 曲名/别名传 `songQuery`，`idxxx` 传 `musicId`，`qq=<A>`, `contextSize=3` |
| `1000000001 musicrank <曲名>` | `group_song_score_member_rank` | 曲名/别名传 `songQuery`，`idxxx` 传 `musicId`，`qq="1000000001"`, `contextSize=3` |
| `musicrank <曲名/idxxx> <N>` / `musicrank<曲名/idxxx> <N>` | `group_song_score_report` | 曲名/别名传 `songQuery`，`idxxx` 传 `musicId`，`sortOrder="desc"`, `outputLimit=N` |
| `musicrank 倒序 <曲名/idxxx> <N>` | `group_song_score_report` | 曲名/别名传 `songQuery`，`idxxx` 传 `musicId`，`sortOrder="asc"`, `outputLimit=N` |
| `musicrank <曲名/idxxx> <起始>-<结束>` / `musicrank<曲名/idxxx> <起始>-<结束>` | `group_song_score_report` | 曲名/别名传 `songQuery`，`idxxx` 传 `musicId`，`sortOrder="desc"`, `startRank=<起始>`, `endRank=<结束>` |
| `musicrank 倒序 <曲名/idxxx> <起始>-<结束>` | `group_song_score_report` | 曲名/别名传 `songQuery`，`idxxx` 传 `musicId`，`sortOrder="asc"`, `startRank=<起始>`, `endRank=<结束>` |

`musicrank` 不支持后置 username、QQ 名字、群名片或文本玩家目标。除真实 At、前置 QQ、前置难度、倒序和输出人数外，命令后的普通文本全部属于曲目查询。

直连层建议限制 `N` 为 `1..30`，范围单次最多 30 人。只有末尾独立纯数字在 `1..30` 范围内时才作为输出人数，只有末尾独立 `<起始>-<结束>` / `<起始>..<结束>` 才作为范围；其它数字继续保留在曲目查询文本里或由底层查歌返回未命中。只允许省略 `musicrank` 后的第一个空格，曲名和人数/范围之间必须有空格，因此 `musicrank系ぎて31-60` 不拆范围。`musicrank <曲名> 1000000001` 不按 QQ 处理，也不按纯数字 username 处理；如果用户要查某个 QQ 在某曲的群内排名，必须写成 `1000000001 musicrank <曲名>` 或使用真实 `@用户`。

个人名次模式只输出个人排名块和该用户前后各 3 人的一张附近排名表。工具结构里仍保留 `rankInfo.rankDesc` / `rankInfo.rankAsc`，但聊天文本不再同时输出正序、倒序两张附近表。榜单模式只输出群单曲榜列表，不追加个人名次。`musicrank <曲名> <N>` / `musicrank 倒序 <曲名> <N>` 的榜单模式不能同时带真实 At 或前置 QQ；出现 `1000000001 musicrank 系ぎて 10`、`musicrank @A 系ぎて 10` 这类玩家目标 + `<N>` 组合时返回语法错误。

## `divingfishrank`

Diving-Fish 全体公开 rating 排名。群聊和私聊都可以直连。不带参数默认查发送者自己的公开排名；排名段一次最多 30 人。

| 写法 | 工具 | 参数 |
|---|---|---|
| `divingfishrank` | `render_maimai_rating_ranking` | `qq=<发送者QQ>` |
| `divingfishrank 31-60` | `render_maimai_rating_ranking` | `startRank=31`, `endRank=60` |
| `divingfishrank 31..60` | `render_maimai_rating_ranking` | 同上 |
| `divingfishrank 第123名` | `render_maimai_rating_ranking` | `startRank=123`, `endRank=123` |
| `divingfishrank 第101到130名` | `render_maimai_rating_ranking` | `startRank=101`, `endRank=130` |
| `[At:A] divingfishrank` / `divingfishrank [At:A]` / `diving[At:A]fishrank` | `render_maimai_rating_ranking` | `qq=<A>` |
| `1000000001 divingfishrank` | `render_maimai_rating_ranking` | `qq="1000000001"` |
| `divingfishrank sample_user` | `render_maimai_rating_ranking` | `username="sample_user"` |
| `divingfishrank 123456` | `render_maimai_rating_ranking` | `username="123456"` |

`divingfishrank` 不带参数时查发送者自己的公开排名。公开榜前 30 需要写 `divingfishrank 1-30`。`divingfishrank <QQ号>` 不按 QQ 处理。查 QQ 必须用前置 QQ 或真实 At。`<username> divingfishrank` 不直连，因为 username 只允许后置。

## 工具路由

现有 Direct Render 的非 search 工具默认走 `maimaidx-render`。排名接入时需要给 `DirectCommand` 增加工具服务器字段，避免把群榜工具误发给 render MCP。

建议路由：

| server | 工具 |
|---|---|
| `render` | `render_maimai_rating_ranking` |
| `group` | `group_b50_report`、`group_b50_member_rank`、`group_song_score_report`、`group_song_score_member_rank` |

`handle_direct_command` 对 group 工具应优先返回 `extract_mcp_text(result)`，不要按图片路径逻辑吞掉文本。`render_maimai_rating_ranking` 仍按现有图片路径逻辑处理。

fallback 子进程客户端也要扩展 `group_module`，默认指向 `group_b50_mcp.server`；AstrBot 已注册 MCP 工具优先复用时，只要工具名能取到，就不依赖 server 名。

## 事件上下文

当前 `TargetContext` 只有 `sender_qq`、`mention_qqs`、`self_qq`。排名接入需要扩展：

| 字段 | 用途 |
|---|---|
| `group_id` | `rank` / `musicrank` 必填 |
| `is_private` | 私聊禁用群榜命令，允许 `divingfishrank` |

`group_id` 从 AstrBot event 获取，优先使用显式 getter 或 message object 字段；拿不到时可从 unified message origin 解析 `GroupMessage:<groupId>`。拿不到群号时，群榜命令不应回退 Agent，应返回明确错误：当前会话无法识别群号，不能直连群排行榜。

## 多目标和硬错误

v2 必须先实现统一目标收集，再进入具体命令 parser：

1. 从整条消息链收集真实 At 目标，过滤 bot 自己和 `all`；收集后先剥离 At，再拼接剩余文本进入命令 parser。
2. 收集前置 QQ 目标；`rank` 不再收集后置 QQ。
3. 只对 `divingfishrank` 收集后置 username。
4. 如果玩家目标数量大于 1，直接返回语法错误；如果 `rank` / `musicrank` 同时出现玩家目标和 `<N>`，也直接返回语法错误。
5. `rank` / `musicrank` 没有玩家目标时使用发送者 QQ。

错误文案建议：

```text
直连语法错误：同一条排名命令只能指定一个玩家目标。
QQ 必须放在最前面，例如 `1000000001 rank`、`1000000001 musicrank 系ぎて`；真实 @ 可以放在消息任意位置；水鱼 username 只用于 `divingfishrank sample_user`；群排行榜查人请使用 QQ 或真实 @。`<N>` 是群榜输出人数，带 `<N>` 时不能同时指定玩家目标。
```

## 实现步骤

1. 扩展 `TargetContext`：增加 `group_id`、`is_private`，并在 AstrBot 插件入口填充。
2. 抽统一排名目标解析器：支持整条消息任意位置的真实 At、前置 QQ、`divingfishrank` 后置 username，多目标报错；`rank` 后置 QQ 不再支持。
3. 给 `DirectCommand` 增加 `server` 字段，扩展 `DirectMcpClient` 的 group MCP 路由。
4. 实现 `rank`：无 `<N>` 调 `group_b50_member_rank` 只查发送者或显式目标；有 `<N>` 调 `group_b50_report` 只输出群榜，不追加个人名次；`myrank` 标为已合并到 `rank`。
5. 实现 `musicrank`：明确 `idxxx` / `id xxx` 先归一成 `musicId`，曲名/别名/拼音别名作为 `songQuery` 交给 group-rank MCP 解析；无 `<N>` 调 `group_song_score_member_rank` 只查发送者或显式目标，有 `<N>` 调 `group_song_score_report` 只输出群榜，不追加个人名次。
6. 扩展 `group_song_score_member_rank`，返回倒序个人名次，或增加 `sortOrder` 供直连层查询倒序个人名次。
7. 实现 `divingfishrank`，支持空参数查发送者、排名段、单名次、QQ、At、username。
8. 补全测试矩阵：群聊/私聊、At 在命令前/后/命令字中间/曲名中间、前置 QQ、`rank` 后置 QQ 报错、`rank` 无 `<N>` 只查发送者/目标个人排名、`rank <N>` 只查群榜、`musicrank` 曲名/别名/拼音别名/id 查询、`musicrank` 无 `<N>` 只查发送者/目标个人排名、`musicrank <N>` 只查群榜、玩家目标 + `<N>` 报错、`divingfishrank` 后置 username/纯数字 username、多目标错误、输出上限、排名段上限。
9. 更新 `docs/astrbot-direct-group-rank-triggers.md` 为用户帮助页，删除旧 v1 中和 v2 冲突的前置 username、QQ 名字目标、后置 QQ 写法，并把 `myrank` / `mymusicrank` 标为已合并到 `rank` / `musicrank`。

## 不在 v2 范围内

- 不新增自然语言触发，例如“群里谁最高”“这首歌谁第一”仍交给 Agent。
- 不新增刷新/status 直连命令，例如 `rank 刷新`、`rank status` 继续交给 Agent。
- 不在直连层做群榜缓存；缓存和后台刷新继续由 group-rank MCP 负责。
- 不在直连层解析后置难度、谱面类型、DX/ST；`musicrank <曲名> 紫` 继续交给 Agent。
