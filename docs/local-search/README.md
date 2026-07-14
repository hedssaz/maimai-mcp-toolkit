# maimai local search MCP

本项目提供一个本地 maimai 曲目查询服务：

- MCP stdio 工具，适合接入 MCP 客户端。
- HTTP API，适合部署到服务器后给应用调用。
- 本地 JSON 缓存，查询时不依赖外网。
- 自定义别名单独保存，更新上游数据时不会被覆盖。
- Diving-Fish 拟合定数缓存，可按拟合定数、虚高/虚低、区服筛选。

## 数据源

### 主数据源

- 曲库：`data/lxns_song_list.json`
  - 来源：落雪咖啡屋公共 API
  - URL：`https://maimai.lxns.net/api/v0/maimai/song/list?notes=true`
  - 包含曲目 ID、曲名、艺术家、分类、BPM、standard/dx/utage 谱面、等级、定数、谱师、物量。
  - LXNS 数字版本码前两位表示年份，后三位表示该年的小版本号，不等同于 JP 官方版本名。例如 `25010` 表示 2025 年小版本 `010`，`24003` 表示 2024 年小版本 `003`。
  - 更新脚本：`scripts/update_music_data.py`
- 别名：`data/lxns_alias_list.json`
  - 来源：落雪咖啡屋公共 API
  - URL：`https://maimai.lxns.net/api/v0/maimai/alias/list`
  - 作为主别名库。
  - 更新脚本：`scripts/update_music_data.py`

### 补充数据

- 日版补充曲库：`data/dxdata.json`
  - 来源：dxrating 维护的 dxdata 数据集
  - URL：`https://raw.githubusercontent.com/gekichumai/dxrating/main/packages/dxdata/dxdata.json`
  - 用途：补充落雪库中没有的日版新歌，例如 `CiRCLE PLUS` 曲目。dxdata 比之前的 MaiDB 多出谱面级 `releaseDate` / `regionOverrides` / `internalId` 等字段。
  - 更新脚本：`scripts/update_dxdata.py`
  - 合并规则：按标题合并；同一首歌如果 LXNS 和 dxdata 都有，查询结果返回一条，并在 `source_fields.cn` 与 `source_fields.jp` 下分别输出两个源的字段，避免同名字段互相覆盖。只有 dxdata 存在的曲目会作为 JP-only 曲目追加。
  - `release_date` 在 dxdata 是谱面级字段；歌曲级 `release_date` 自动取所有谱面里最早的 `sheet.releaseDate`。
- 官方解包本地曲库：`data/official_music_data.json`
  - 来源：本机从官方客户端资源解包出的 `extracted_music_resources/`，不是远程下载源。
  - 生成脚本：`scripts/import_official_music_resources.py`
  - 用途：补充官方包里的当前 CN 曲库和等级标签，优先级位于 LXNS 之后、dxdata 之前。也就是说，LXNS 仍是主源；LXNS 缺字段或缺歌时，官方本地源优先于日服 dxdata。
  - 导入规则：脚本读取 `music_data.json` / `catalogs.json`，按 `nonDxId` 合并普通 ST/DX，宴谱按官方 6 位 ID 单独保留；等级标签使用 `catalogs.json` 的 `MusicLevel`，避免用定数小数手推 `+`。`jackets/*.png` 中没有对应曲库记录的 orphan 曲绘也会按文件名 ID 复制，供绘图降级使用。
  - 这个文件是生成快照，默认被 `.gitignore` 忽略。要启用它，需要先运行导入脚本生成本地文件。
- Yuzu 别名补充：`data/music_alias.json`
  - 来源：Yuzu maimai 别名 API
  - URL：`https://www.yuzuchan.moe/api/maimaidx/maimaidxalias`
  - 用途：作为落雪别名之外的补充别名源。
  - 更新脚本：`scripts/update_yuzu_alias_data.py`
- 旧项目 CSV 别名补充：`data/aliases.csv`
  - 来源：`xszqxszq/maimai-bot`
  - URL：`https://raw.githubusercontent.com/xszqxszq/maimai-bot/master/src/main/resources/config/aliases.csv`
  - 用途：只作为兜底补充，不作为主别名源。
  - 这个文件较老，新歌别名不要指望它覆盖。
- 拟合定数：`data/divingfish_chart_stats.json`
  - 来源：Diving-Fish maimai DX 查分器公开接口
  - URL：`https://www.diving-fish.com/api/maimaidxprober/chart_stats`
  - 用途：为本地曲库谱面附加 `fit_diff`，并计算 `fit_delta = ds - fit_diff`、`虚高/虚低` 标签。
  - 更新脚本：`scripts/update_chart_stats.py`

### 计分规则来源

- 计分 MCP：合并自 `skill-https-luch4736-github-io-post/maimai-dx-mcp`
- 规则参考：`https://luch4736.github.io/post/2GwvWgY6N/`
- 用途：提供 `score_counts` 和 `find_score_combinations`，用于舞萌DX音符判定分数计算与反查判定组合。

### 自定义别名

- 文件：`data/custom_aliases.json`
- 来源：本服务的 `POST /alias` 或 MCP 工具 `add_maimai_alias`
- 用途：保存你自己添加的别名。
- 格式：

```json
{
  "1759": ["rnr"],
  "Operation☆DOTABATA!": ["蓝档案闹腾"]
}
```

`custom_aliases.json` 不会被上游更新脚本覆盖。容器部署时应挂载 `data/`，否则重建容器会丢失新增别名。

别名不是官方字段，新歌可能没有别名。没有别名时仍可用曲目 ID、官方曲名、等级、定数查询。dxdata 补充曲库中的日版新歌可能没有别名；搜索结果会标记 `source`、`source_labels`、`source_fields` 和区域可用性。

## 更新数据

本机更新：

```bash
python scripts/update_music_data.py
python scripts/update_yuzu_alias_data.py
python3 scripts/update_dxdata.py
python scripts/update_chart_stats.py
python scripts/import_official_music_resources.py /path/to/extracted_music_resources
```

一键更新：

```bash
python scripts/update_all_data.py
```

服务器更新：

```bash
cd /opt/maimai-local-search
python scripts/update_all_data.py
docker restart maimai-local-search
```

`scripts/update_music_data.py` 会刷新落雪曲库和落雪别名库。`scripts/update_yuzu_alias_data.py` 会刷新 Yuzu 别名补充。`scripts/update_dxdata.py` 会刷新 dxrating 日服补充曲库。`scripts/update_chart_stats.py` 会刷新 Diving-Fish 拟合定数。`scripts/import_official_music_resources.py` 会把本机官方解包目录转换成 `data/official_music_data.json`，并默认把 `jackets/*.png` 复制到渲染封面目录；只想生成 JSON 时加 `--no-copy-covers`。`scripts/update_all_data.py` 会依次更新所有远程数据源并运行 `compileall` 检查派生状态。所有更新脚本都不会修改 `data/custom_aliases.json`。

## 本地查询

```bash
python -m maimai_mcp.search 希腊奶
```

常用参数：

```bash
# ID 查询；10288 会按落雪规则归一到 288
python -m maimai_mcp.search 10288

# 模糊曲名或别名
python -m maimai_mcp.search 六兆年

# 等级 + 难度 + 谱面类型
python -m maimai_mcp.search --level 13+ --difficulty 紫 --type dx --limit 20

# 定数范围
python -m maimai_mcp.search --ds 13.6-13.8 --difficulty Master --type dx
```

## MCP 配置

把下面配置加到支持 MCP stdio 的客户端里：

```json
{
  "mcpServers": {
    "maimai-local-search": {
      "command": "python",
      "args": [
        "-m",
        "maimai_mcp.server"
      ],
      "cwd": "/path/to/maimai"
    }
  }
}
```

暴露工具：

- `search_maimai_songs`
- `batch_search_maimai_songs`
- `add_maimai_alias`
- `list_maimai_aliases`
- `random_maimai_songs`
- `today_maimai`
- `list_maimai_songs_by_id`
- `list_maimai_versions`
- `refresh_maimai_sources`
- `score_counts`
- `find_score_combinations`
- `query_chart_history`

参数：

- `query`：曲目 ID、旧式 DX ID、官方曲名片段或别名片段。
- `genre`：流派/分类，模糊匹配，例如 `POPS`、`niconico`、`オンゲキ`。
- `version`：版本，模糊匹配曲目版本或谱面版本，例如 `PRiSM PLUS`、`dx-prism-plus`、`25010`。LXNS 年份筛选支持 `2025`、`25`、`dx2025`、`dx25`、`2025/25`，都会匹配全部 `25xxx` 版本码；小版本字母支持 `25-A` / `25-a` / `dx25-A`，其中 `A=000`、`B=001`、`C=002`。
- `level`：等级，例如 `13`、`13+`、`14+`。
- `ds`：定数或范围，例如 `13.8`、`13.6-13.8`。
- `ds_min` / `ds_max`：定数上下界。
- `fit_diff`：拟合定数。单值按一位小数向下分桶，例如 `13.1` 或 `13.17` 都查 `13.10 <= fit_diff < 13.20`；字符串范围如 `13.13-13.27` 不取整。
- `fit_diff_min` / `fit_diff_max`：拟合定数范围上下界，不取整。
- `fit_delta`：实际定数 `ds` 减拟合定数 `fit_diff`，可传单值或范围；负数范围可写成 `-0.3--0.1`。
- `fit_delta_min` / `fit_delta_max`：差值范围上下界。
- `fit_label`：`虚高` 或 `虚低`。`fit_delta > 0` 为虚高，`fit_delta < 0` 为虚低。
- `region_has` / `region_missing`：区服筛选，支持 `jp`、`intl`、`usa`、`cn`，也支持 `日服`、`国际服`、`美服`、`国服`。
- `sort`：`fit_delta_desc`（最虚高优先）、`fit_delta_asc`（最虚低优先）、`fit_diff_asc`、`fit_diff_desc`。
- `difficulty`：`Basic`、`Advanced`、`Expert`、`Master`、`Re:MASTER`，也支持 `绿`、`黄`、`红`、`紫`、`白`。
- `song_type`：`standard`/`SD`、`dx`、`utage`/`宴`。
- `limit`：限制返回数量；不填则返回全部命中。

`batch_search_maimai_songs` 接收 `items` 数组，每一项都可以使用上面这些搜索参数，并可附带 `key`。它会在一次 MCP 调用里返回每项独立的 `ok/result/error`，适合 B50 这类需要按水鱼 `song_id` 批量补谱面拟合信息的场景。

搜索结果说明：

- `source`：数据源名称，可能是 `lxns`、`official`、`dxdata`、`cndivingfish` 或它们的 `+` 组合（按 cn/official/jp/divingfish 优先级排序）。
- `source_labels`：源标签，`cn` 表示 LXNS/国服侧缓存，`official` 表示官方解包本地源，`jp` 表示 dxdata 日服侧缓存。
- `available_chart_types`：这首歌拥有的谱面类型。可读输出里会显示为 `谱面 ST` / `谱面 DX` / `谱面 ST/DX`；compact 模式还会把难度行按 `ST #835:`、`DX #10835:` 这类谱面 ID 分组。
- `source_fields.cn` / `source_fields.official` / `source_fields.jp`：各源的字段摘要。同名字段分别放在各源标签下，例如 `title`、`artist`、`genre`、`bpm`、`version` 不会互相覆盖。
- `matched_charts[].source`：谱面来自哪个源，值为 `cn`、`official`、`jp` 或 `divingfish`。
- `matched_charts[].fit_diff`：Diving-Fish 拟合定数。
- `matched_charts[].fit_delta`：实际定数 `ds` - 拟合定数 `fit_diff`。
- `matched_charts[].fit_label`：`虚高` / `虚低`，或为空。
- `regions`：聚合后的区服可用性，字段为 `jp`、`intl`、`usa`、`cn`。CN 谱面和官方解包谱面会推断 `cn: true`；dxdata 谱面会推断 `jp: true`，并保留源里已有的 `intl` / `usa` / `cn` 信息。

`add_maimai_alias` 参数：

- `song_id`：曲目 ID。dxdata 追加曲使用 `songId` 字符串；可直接传返回结果里的 `id` 或 `source_id`。
- `title`：没有 ID 时可传精确曲名。
- `alias`：要添加的别名。

`list_maimai_aliases` 参数：

- `query`：曲目 ID、旧式 DX ID、官方曲名片段或别名片段。
- `song_id`：曲目 ID；传了以后按 `query` 使用。
- `title`：曲名或曲名片段；未传 `query` / `song_id` 时使用。
- `limit`：最多返回多少首匹配歌曲，默认 `20`，最多 `200`。

这个工具会直接返回每首匹配歌曲的完整 `aliases` 列表，文本输出不截断别名。别名来源与搜索一致，会合并 LXNS、Yuzu、旧 CSV 和本地自定义别名；重复别名会按规范化文本去重，只输出一次。

`refresh_maimai_sources` 参数：

- `sources` / `source`：要刷新的源，支持 `all`、`lxns`、`maidb`、`yuzu`、`chart_stats`，默认 `all`。
- `ttl_days`：源缓存 TTL，默认 `3` 天。未过期时默认跳过。
- `force`：强制刷新，忽略 TTL。
- `check_only`：只查看源年龄和是否过期，不下载。
- `timeout_seconds`：每个源刷新命令的超时时间，默认 `25` 秒。多个到期源会并发刷新，某个源失败只会进入 `failed_sources`，不影响其它源完成。

刷新源不会修改 `data/custom_aliases.json`。只要有源实际更新成功，工具会自动运行 Python 编译检查作为派生状态校验。

`random_maimai_songs` 参数：

- `count`：随机返回数量，默认 `1`，最多 `100`。
- `genre`：可选流派/分类筛选；不传谱面条件时仍然是按曲目 ID 随机。
- `version`：可选版本筛选；不传谱面条件时仍然是按曲目 ID 随机。
- `level`：可选等级筛选，例如 `13`、`13+`、`14+`。
- 不传 `level` / `ds` / `difficulty` / `song_type` 时是按曲目 ID 随机，只随机选歌，不随机具体等级或谱面。
- 传了 `level` 或 `ds` 后，会按谱面条件筛候选后随机；`ds` 支持单个定数或范围。
- `fit_diff` / `fit_diff_min` / `fit_diff_max` / `fit_delta` / `fit_delta_min` / `fit_delta_max` / `fit_label` / `region_has` / `region_missing`：含义同搜索工具。使用任意拟合定数或差值条件时，会进入谱面筛选随机。
- `seed`：可选随机种子，用于复现同一组随机结果。

示例：

```json
{"count": 1}
```

```json
{"count": 3, "level": "14"}
```

```json
{"count": 3, "ds": "13.6-13.8"}
```

```json
{"count": 1, "level": "15", "fit_label": "虚低"}
```

```json
{"count": 3, "level": "14", "difficulty": "紫", "song_type": "dx"}
```

`today_maimai` 参数：

- `qq`：必填，用于计算今日人品的 QQ 号。
- `botName`：可选，提醒文案里的 Bot 名称，默认 `MaiBot`。\n- `offset`：可选今日人品偏移值，默认 `0`。偏移会加到原项目 qqhash 的日期项上，即 offset=0 时完全按原逻辑；改成其他整数后，RP、宜忌和推荐歌都会与其它 Bot 错开。

固定提醒正文为 `以上内容均由程序自动生成，仅供娱乐参考`。

工具会先按原逻辑从 hash 低位依次取 11 个宜忌值，再使用已经右移 22 位后的 `h` 从本地曲库中选择今日推荐歌曲。推荐歌池只包含有数字歌曲 ID 的曲目，dxdata-only 这类只有字符串 ID 的曲目不会进入候选。

```json
{"qq": "123456789", "offset": 7}
```

`list_maimai_songs_by_id` 参数：

- `order`：`asc`/`desc`，也支持 `正序`/`倒序`、`升序`/`降序`，默认 `asc`。
- `limit`：返回前多少首，默认 `20`，最多 `2000`。
- `genre` / `version` / `level` / `ds` / `ds_min` / `ds_max` / `fit_diff` / `fit_diff_min` / `fit_diff_max` / `fit_delta` / `fit_delta_min` / `fit_delta_max` / `fit_label` / `region_has` / `region_missing` / `difficulty` / `song_type` / `sort`：可选筛选条件，含义同搜索工具。

`list_maimai_versions` 参数：

- `query`：可选版本名/版本号模糊过滤，例如 `PRiSM`、`25010`。
- `limit`：可选返回数量。

示例：

```json
{}
```

```json
{"query": "PRiSM"}
```

`query_chart_history` 参数：

- `query`：歌名、歌曲 ID 或别名（必填）。
- `difficulty`：难度过滤，例如 `Master`、`紫`。
- `song_type`：谱面类型过滤：`dx`、`standard`、`utage`。

基于日服 dxdata 的 `multiverInternalLevelValue` 数据，返回歌曲每张谱面在各版本的定数。连续相同定数的版本自动合并为一行。仅返回有历史数据的谱面。

示例：

```json
{"query": "毒占欲"}
```

```json
{"query": "はじめまして地球人さん", "difficulty": "Master"}
```

拟合定数单值分桶：

```json
{"fit_diff": 13.1, "limit": 20}
```

按差值找最虚高：

```json
{"level": "13+", "fit_label": "虚高", "sort": "fit_delta_desc", "limit": 10}
```

区服筛选：

```json
{"region_has": "日服", "region_missing": "国服", "limit": 20}
```

示例：

```json
{"order": "asc", "limit": 10}
```

```json
{"order": "desc", "limit": 20}
```

`score_counts` 参数：

- `counts`：按 `音符类型 -> 判定 -> 数量` 输入，例如 `tap`、`touch`、`hold`、`slide`、`break`。
- 不需要传 `score_mode`；工具会一次返回 `totals.oldscore`、`totals.oldacc`、`totals.dxscore`、`totals.dxacc`。
- `display_digits`：`oldacc` / `dxacc` 输出用几位小数，默认 `4`。
- `include_zero`：是否返回数量为 0 的行。

计分音符类型：`touch` 和 `tap` 同权重；`touch_hold` / `touchhold` / `thold` 作为输入别名按 `hold` 处理。

判定名：`tap`、`touch`、`hold`、`slide` 的高低判定分值相同，输入和输出优先用 `great`、`perfect`；`break` 的高低判定会影响基础分、奖励分或旧框分，必须使用 `great_low` / `great_mid` / `great_high`、`perfect_low` / `perfect_high`，不能在精确数量里写模糊的 `great` / `perfect`。

示例：

```json
{
  "counts": {
    "tap": { "critical": 100, "great": 2 },
    "break": { "perfect_high": 1, "critical": 3 }
  }
}
```

`find_score_combinations` 参数：

- `note_totals`：谱面物量，形如 `{ "tap": 100, "touch": 12, "hold": 20, "slide": 30, "break": 5 }`。
- `target_score`：精确目标分。
- `min_score` / `max_score`：目标分数段；与 `target_score` 二选一。
- `score_mode`：`base`、`break_bonus`、`oldscore`、`oldacc`、`dxscore` 或 `dxacc`，默认 `oldscore`。
- `oldscore` 查旧框 raw score；`oldacc` 查旧框百分比；`dxscore` 查 DX SCORE；`dxacc` 查 DX 达成率。
- 在 `oldacc` / `dxacc` 下，`target_score` 如果是整数就表示去掉小数点的百分比，例如 `100.4999 -> 1004999`；也可以直接传百分比字符串，如 `"100.4999"` 或 `"100.4999%"`。也可用 `target_acc`、`min_acc`、`max_acc` 传百分比。
- `allowed_judgments` / `disallowed_judgments`：每类音符允许或禁止的判定/判定组。
- `fixed_counts` / `min_counts` / `max_counts`：固定、最少、最多的判定数量。
- `no_miss_good: true`：快捷约束，所有音符禁止 `miss` 和 `good`。
- `break_max_perfect_or_below`：快捷约束，Break 中 `critical` 以下的数量最多为 N。等价于自动设置 `break.critical >= break总数 - N`。
- `max_solutions`：最多返回多少个组合，默认 `10`。

新增直传查歌功能：如果不传 `note_totals`，可以直接传 `query`、`song_id` 或 `title`，并可搭配 `level`、`difficulty`、`song_type`、`ds`、`genre`、`version` 缩小谱面范围。服务会先用本地曲库查歌：只有唯一歌曲且唯一谱面时，自动取该谱面的物量传入计算；如果匹配多首歌，会返回多个歌名候选并且不计算；如果唯一歌曲下仍有多个谱面，会返回谱面候选并且不计算。谱面中的 `touch` 会作为独立 `note_totals.touch` 传给计分 MCP，不再合并进 `tap`。

达成率反搜也使用 `find_score_combinations`，不要新配 MCP。DX 达成率传 `score_mode: "dxacc"`；旧框百分比传 `score_mode: "oldacc"`。`target_score` 可填去掉小数点后的整数，也可直接填 `"100.4999%"`。默认按四位小数和 `display_mode: "floor"` 解释，也可传 `display_mode: "half_up"` 或 `"exact"`。

示例：

```json
{
  "query": "Seize The Day",
  "difficulty": "Basic",
  "song_type": "dx",
  "target_score": 0,
  "score_mode": "oldscore",
  "max_solutions": 1
}
```

```json
{
  "note_totals": { "tap": 5, "touch": 2, "break": 1 },
  "target_score": 5050,
  "score_mode": "oldscore",
  "allowed_judgments": {
    "break": "perfect",
    "tap": "not_miss"
  },
  "max_solutions": 5
}
```

```json
{
  "query": "系ぎて",
  "difficulty": "白",
  "song_type": "dx",
  "score_mode": "dxacc",
  "target_score": 1004999,
  "display_mode": "floor",
  "max_solutions": 5
}
```

“全谱无 miss/good，Break 最多 5 个 Perfect 及以下”可以直接写：

```json
{
  "query": "系ぎて",
  "difficulty": "白",
  "song_type": "dx",
  "score_mode": "dxacc",
  "target_score": "100.4999%",
  "no_miss_good": true,
  "break_max_perfect_or_below": 5,
  "max_solutions": 5
}
```

## HTTP 服务

本项目也带一个无额外依赖的 HTTP 查询服务，便于部署到服务器。

```bash
python -m maimai_mcp.http_server --host 0.0.0.0 --port 8000
```

图形化面板：

```text
http://127.0.0.1:8000/
```

面板可以查询曲目、按等级/定数/难度/谱面类型筛选，并通过右侧详情栏写入自定义别名。新增别名仍保存到 `data/custom_aliases.json`。

接口：

```bash
curl 'http://127.0.0.1:8000/search?query=希腊奶&limit=1'
curl 'http://127.0.0.1:8000/search?ds=13.6-13.8&difficulty=Master&song_type=dx&limit=5'
curl 'http://127.0.0.1:8000/search?fit_diff=13.1&limit=5'
curl 'http://127.0.0.1:8000/search?level=13%2B&fit_label=虚高&sort=fit_delta_desc&limit=5'
curl -X POST 'http://127.0.0.1:8000/search' \
  -H 'content-type: application/json' \
  -d '{"query":"rnr","limit":1}'
curl -X POST 'http://127.0.0.1:8000/random' \
  -H 'content-type: application/json' \
  -d '{"level":"15","fit_label":"虚低","count":1}'
curl 'http://127.0.0.1:8000/songs-by-id?order=desc&limit=10'
curl 'http://127.0.0.1:8000/versions?query=PRiSM'
curl 'http://127.0.0.1:8000/source-status'
curl -X POST 'http://127.0.0.1:8000/refresh-sources' \
  -H 'content-type: application/json' \
  -d '{"force":true,"sources":"all"}'
curl -X POST 'http://127.0.0.1:8000/alias' \
  -H 'content-type: application/json' \
  -d '{"song_id":"Operation☆DOTABATA!","alias":"蓝档案闹腾"}'
```

Docker：

```bash
docker build -t maimai-local-search:latest .
docker run -d --name maimai-local-search --restart unless-stopped -p 8000:8000 \
  -v /opt/maimai-local-search/data:/app/data \
  maimai-local-search:latest
```

新增别名单独保存在 `data/custom_aliases.json`，不会修改落雪/dxdata 原始缓存。容器部署时建议挂载 `data/`，避免重建镜像后丢失新增别名。
