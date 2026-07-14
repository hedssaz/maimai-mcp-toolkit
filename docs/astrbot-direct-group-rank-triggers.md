# AstrBot Direct MCP 群排行触发词

本文按当前 `astrbot_plugin_maimai_auto_send_images/direct_render.py` 实现整理。直连命中后由插件直接调用 MCP，不走 LLM；未命中时继续交给 Agent。

直连仍要求先通过 AstrBot 唤醒条件：当前适配器唤醒词、`@bot`、或 AstrBot 能识别的引用/回复 bot 消息。`rank` / `musicrank` 只在群聊直连；`divingfishrank` 群聊和私聊都可以直连。

## 总览

| 命令 | 范围 | 默认行为 | 带 `<N>` 行为 |
|---|---|---|---|
| `rank` | 当前群 B50/rating | 只查发送者或目标的个人群内正倒排名 | 只输出群榜前/后 N 人，不追加个人名次 |
| `musicrank` | 当前群单曲成绩 | 只查发送者或目标在该曲的个人群内正倒排名 | 只输出该曲群榜前/后 N 人，不追加个人名次 |
| `divingfishrank` | Diving-Fish 公开 rating 榜 | 默认定位发送者自己的公开排名；也可查 username/QQ 或指定公开榜排名段 | 排名段一次最多 30 人 |

`N` 取值 `1..30`。

## 玩家目标

| 目标 | `rank` / `musicrank` | `divingfishrank` |
|---|---|---|
| 真实 `@用户` | 可以放在消息任意位置；传 `qq` | 可以放在消息任意位置；传 `qq` |
| 前置 QQ | 支持，例如 `1000000001 rank`、`1000000001 musicrank 系ぎて` | 支持，例如 `1000000001 divingfishrank` |
| 后置 QQ | 不支持；`rank 1000000001` 报错，`musicrank 系ぎて 1000000001` 仍属于曲目查询 | 不作为 QQ；`divingfishrank 123456` 按 username |
| username / 昵称 / 群名片 | 不支持 | 只支持后置水鱼 username，例如 `divingfishrank sample_user` |

同一条命令最多只能有一个玩家目标。出现多个真实 At、At + 前置 QQ、或玩家目标 + `<N>` 时，直连层返回语法错误，不回退给 Agent。

## `rank`

| 写法 | 工具 | 参数 |
|---|---|---|
| `rank` / `rank 倒序` | `group_b50_member_rank` | `groupId=<当前群>`, `qq=<发送者QQ>`, `outputMode="rating"`, `contextSize=3` |
| `@某人 rank` / `rank @某人` / `ra@某人nk` | `group_b50_member_rank` | `qq=<被@用户QQ>` |
| `<QQ号> rank` | `group_b50_member_rank` | `qq=<QQ号>` |
| `rank <N>` / `rank<N>` | `group_b50_report` | `sortOrder="desc"`, `outputMode="rating"`, `outputLimit=N` |
| `rank 倒序 <N>` / `rank倒序<N>` | `group_b50_report` | `sortOrder="asc"`, `outputMode="rating"`, `outputLimit=N` |
| `rank <起始>-<结束>` / `rank<起始>-<结束>` | `group_b50_report` | `sortOrder="desc"`, `outputMode="rating"`, `startRank=<起始>`, `endRank=<结束>` |
| `rank 倒序 <起始>-<结束>` / `rank倒序<起始>-<结束>` | `group_b50_report` | `sortOrder="asc"`, `outputMode="rating"`, `startRank=<起始>`, `endRank=<结束>` |

不支持：

| 写法 | 结果 |
|---|---|
| `rank <QQ号>` | 报错；QQ 只能前置 |
| `<QQ号> rank <N>` / `rank @某人 <N>` / `<QQ号> rank 31-60` | 报错；榜单模式不能同时指定玩家目标 |
| `myrank` | 不作为直连命令；使用 `rank` |

## `musicrank`

曲名、别名、拼音别名交给底层 group-rank MCP 的 `songQuery` 解析；`id11451` / `id 11451` / 无输出人数歧义的纯数字 ID 会直传 `musicId`。不写难度时默认查该曲已有谱面的最高难度；命令后第一个字如果是 `绿黄红紫白`，会先当作难度，其后文本才是歌曲，例如 `musicrank 白系` 表示白谱歌曲 `系`。若目标曲只有一个难度，底层会自动改用那一个难度。直连层不解析后置难度、谱面类型或 DX/ST。

| 写法 | 工具 | 参数 |
|---|---|---|
| `musicrank <曲名/别名/idxxx>` / `musicrank<曲名>` | `group_song_score_member_rank` | `groupId=<当前群>`, `qq=<发送者QQ>`, `contextSize=3`, 加 `songQuery` 或 `musicId`；底层自动选最高难度 |
| `musicrank<难度><曲名>` / `musicrank <难度><曲名>` / `musicrank <难度> <曲名>` | `group_song_score_member_rank` | `绿=0`, `黄=1`, `红=2`, `紫=3`, `白=4`；难度前缀后的文本为歌曲 |
| `@某人 musicrank <曲名>` / `musicrank @某人 <曲名>` / `musicrank <曲名> @某人` / `musicrank 系@某人ぎて` | `group_song_score_member_rank` | `qq=<被@用户QQ>`, 同曲目 |
| `<QQ号> musicrank <曲名>` | `group_song_score_member_rank` | `qq=<QQ号>`, 同曲目 |
| `musicrank <曲名/别名/idxxx> <N>` / `musicrank<曲名> <N>` | `group_song_score_report` | `sortOrder="desc"`, `outputLimit=N`, 加 `songQuery` 或 `musicId` |
| `musicrank 倒序 <曲名/别名/idxxx> <N>` | `group_song_score_report` | `sortOrder="asc"`, `outputLimit=N`, 加 `songQuery` 或 `musicId` |
| `musicrank <曲名/别名/idxxx> <起始>-<结束>` / `musicrank<曲名> <起始>-<结束>` | `group_song_score_report` | `sortOrder="desc"`, `startRank=<起始>`, `endRank=<结束>`, 加 `songQuery` 或 `musicId` |
| `musicrank 倒序 <曲名/别名/idxxx> <起始>-<结束>` | `group_song_score_report` | `sortOrder="asc"`, `startRank=<起始>`, `endRank=<结束>`, 加 `songQuery` 或 `musicId` |

不支持：

| 写法 | 结果 |
|---|---|
| `musicrank<曲名><起始>-<结束>` | 不拆范围；曲名和范围之间必须有空格 |
| `musicrank <曲名> <QQ号>` | 后置 QQ 不作为目标，仍属于曲目查询文本 |
| `<QQ号> musicrank <曲名> <N>` / `musicrank @某人 <曲名> <N>` | 报错；榜单模式不能同时指定玩家目标 |
| `musicrank <曲名> 紫` | 不按后置难度直连；难度必须放在 `musicrank` 后、歌曲前 |
| `musicrank <曲名> dx` | 不直连；交给 Agent 判断谱面类型 |
| `mymusicrank` | 不作为直连命令；使用 `musicrank <曲名>` |

## `divingfishrank`

| 写法 | 工具 | 参数 |
|---|---|---|
| `divingfishrank` | `render_maimai_rating_ranking` | `qq=<发送者QQ>` |
| `divingfishrank <起始>-<结束>` | `render_maimai_rating_ranking` | `startRank=<起始>`, `endRank=<结束>` |
| `divingfishrank <起始>..<结束>` | `render_maimai_rating_ranking` | 同上 |
| `divingfishrank 第<名次>名` | `render_maimai_rating_ranking` | `startRank=<名次>`, `endRank=<名次>` |
| `divingfishrank 第<起始>到<结束>名` | `render_maimai_rating_ranking` | `startRank=<起始>`, `endRank=<结束>` |
| `@某人 divingfishrank` / `divingfishrank @某人` / `diving@某人fishrank` | `render_maimai_rating_ranking` | `qq=<被@用户QQ>` |
| `<QQ号> divingfishrank` | `render_maimai_rating_ranking` | `qq=<QQ号>` |
| `divingfishrank <username>` | `render_maimai_rating_ranking` | `username=<水鱼username>` |

排名段单次最多 30 人。`divingfishrank 31-61` / `divingfishrank 1-100` 返回语法错误。`divingfishrank` 不带参数时查发送者自己；公开榜前 30 需要写 `divingfishrank 1-30`。`divingfishrank 123456` 按水鱼 username 处理，不按 QQ；查 QQ 必须写 `<QQ号> divingfishrank` 或使用真实 At。

## 不直连

| 写法 | 说明 |
|---|---|
| 私聊 `rank` / `musicrank` | 群排行榜必须能识别当前群号 |
| `rank 刷新` / `rank status` / `刷新群榜` | 刷新和状态类自然语言继续交给 Agent |
| `群榜` / `排行榜` / `这个群谁最高` / `这首歌群里谁第一` | 只保留本文列出的精确英文命令直连 |
