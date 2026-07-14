# maimai-mcp-toolkit Usage Details

## MCP servers

The repository is intentionally split into several stdio MCP servers:

- `maimai_mcp.server`: local song search, aliases, scoring, random songs, today maimai, source refresh.
- `diving_fish_b50_mcp.server`: Diving-Fish B50, computed/fitted B50, public API wrapper, Developer-Token helpers.
- `maimaidx_render_mcp.server`: maimaiDX/Yuzu style image rendering for B50, plates, rating tables, progress, song info, song score, recommendations, and rankings.
- `qq_identity_mcp.server`: QQ/NapCat identity cache.
- `group_b50_mcp.server`: group B50 and group song-score ranking orchestration.
- `maimai_score_mcp.server`: song-name to player-score bridge.
- `maimai_update_mcp.server`: direct-plugin fallback for official raw score import and Diving-Fish upload.

## 落雪 OAuth 绑定边界

`lxns_oauth_mcp.server` 是独立的本地 stdio MCP，只处理落雪 OAuth 授权链接、绑定、绑定状态和解绑。授权 state、令牌和待拍一拍确认记录保存在独立的本地 SQLite 中，不依赖成绩存储。该 MCP 不查成绩、不接入 SEGA 官方成绩接口，也不包含日服数据或功能。

启动入口：

```bash
python -m lxns_oauth_mcp.server
```

客户端 ID、客户端密钥和回调地址只能通过运行环境注入，不要写入源码、插件配置或打包文件。空占位模板如下：

```dotenv
LXNS_OAUTH_CLIENT_ID=
LXNS_OAUTH_CLIENT_SECRET=
LXNS_OAUTH_REDIRECT_URI=
```

AstrBot 插件默认把 OAuth SQLite 放到 `direct_render_data_dir` 下的私密运行目录；也可通过 `LXNS_OAUTH_DB` 覆盖。覆盖路径必须位于专用目录中：新目录会以 `0700` 创建，已存在目录也必须已经是 `0700`，程序不会改写共享目录权限。数据库、WAL 与 SHM 文件会收紧为 `0600`。部署复制和 Docker 构建会排除源码树中的 OAuth 私密运行目录，避免把本地令牌带进部署包。

## 群内绑定流程

- `lxns bind` 创建授权链接。配置回调桥时，插件在授权链接确认生成后才开始轮询；收到回调后仅暂存令牌，必须由原用户在原适配器、原会话中拍一拍当前机器人才会完成绑定。
- `lxns bind <code>`、`lxns bind code=...` 与 `lxns bind <完整回调 URL>` 可以在群里手工提交授权结果。只有明确的 `lxns bind` 命令才会解析 code，普通消息中的裸 code 不会被捕获。
- `lxns status` 只显示当前是否已绑定或等待确认，不返回令牌或账号资料。
- `lxns unbind` 删除对应授权状态、令牌和待确认记录。

AstrBot 插件的 OAuth 子进程与回调配置项为：

- `direct_render_oauth_module`：OAuth MCP 模块，默认为 `lxns_oauth_mcp.server`。
- `direct_render_lxns_callback_poll_url`：回调桥轮询地址；留空时仅使用手工提交流程。
- `direct_render_lxns_callback_poll_token`：回调桥共享 Token，同时用于签名 state 和 Bearer 轮询鉴权；UTF-8 编码后至少 32 字节。
- `direct_render_lxns_callback_timeout_seconds`：整个回调等待的超时时间。
- `direct_render_lxns_callback_poll_interval_seconds`：两次轮询之间的间隔。
- `direct_render_lxns_callback_http_timeout_seconds`：单次轮询请求的超时时间。
- `direct_render_lxns_poke_confirm_timeout_seconds`：回调到达后等待原用户拍一拍确认的超时时间。

## 回调桥部署

回调桥程序为 `scripts/lxns_oauth_callback_bridge.py`，配套文件为 `deploy/lxns-oauth-callback.env.example`、`deploy/lxns-oauth-callback.nginx.conf` 和 `deploy/lxns-oauth-callback.service`；模板不包含任何实例信息。先在环境文件中设置 `LXNS_CALLBACK_TOKEN=`，其 UTF-8 编码长度至少为 32 字节，再使回调桥与插件的共享 Token 保持一致。反向代理需将授权回调和 Bot 轮询分别转发到模板中的回调路径与轮询路径；两者均关闭访问日志，轮询端点还必须使用 Bearer 鉴权。

回调桥对同一 state 只保留第一个 code，且只允许成功消费一次；已消费记录保留墓碑，重复回调不会重新激活绑定流程。

`direct_render_lxns_callback_timeout_seconds` 与回调桥的 `LXNS_CALLBACK_TTL_SECONDS` 应保持一致；默认均为 600 秒。轮询响应只返回是否就绪、授权 code 和接收时间，不回显 state。

## 插件可复现打包

插件 ZIP 只从固定白名单取文件，并统一时间戳、权限和压缩元数据。修改插件源码后执行：

```bash
python scripts/package_astrbot_plugin.py --force
```

脚本会校验成员白名单、CRC、文件字节与两个 ZIP 的一致性，只在两个临时包都校验成功后才覆盖原包。

## AstrBot layout

Recommended host layout:

```text
/opt/qqbot/data/maimai-mcp                         # this repository
/opt/qqbot/data/maimai-config                      # tokens and custom aliases
/opt/qqbot/data/maimai-yuzu-static/Resource/static # maimaiDX/Yuzu static assets
/opt/qqbot/data/maimai-images                      # maimaidx-render outputs
/opt/qqbot/data/maimai-covers                      # local cover cache
/opt/qqbot/data/player-cache                       # player B50/records cache
/opt/qqbot/data/group-b50-cache                    # group B50 cache
/opt/qqbot/data/group-song-cache                   # group song-score cache
/opt/qqbot/data/qq-identity-cache                  # QQ identity cache
/opt/qqbot/data/plugins                            # AstrBot plugins
```

Install/update the standard deployment:

```bash
python scripts/install_astrbot_deploy.py --data-dir /opt/qqbot/data
docker restart astrbot
```

Override NapCat address when needed:

```bash
python scripts/install_astrbot_deploy.py \
  --data-dir /opt/qqbot/data \
  --napcat-base-url http://napcat:3000
```

The deployment script writes MCP entries to `/opt/qqbot/data/mcp_server.json` and keeps existing unrelated MCP config.

## Computed B50 current-version split

`query_computed_b50` and fitted B50 rendering split B35/B15 by the newest ranked Diving-Fish `basic_info.from` version in `data/divingfish_song_list.json`. They do not use LXNS version codes such as `255xx`, and they no longer rely on `basic_info.is_new` when a known newer version exists.

If a future Diving-Fish version name appears before this code knows its order, set `MAIMAI_LOCAL_CURRENT_VERSIONS` or `MAIMAI_CURRENT_VERSIONS` to a comma/semicolon-separated version list to override the current B15 version set.

## Static resources

`maimaidx-render` uses maimaiDX/Yuzu static resources. If the open package does not include the resource pack, download it once:

当前公开分支的目录和可达历史中只保留自定义“雪峰”牌子完成渲染必需的 20 张图片；其余图片需在部署时按需提供，且默认不纳入 Git。

```bash
curl -L -o Resource.7z https://cloud.yuzuchan.moe/f/nXt6/Resource.7z
mkdir -p /opt/qqbot/data/maimai-yuzu-static
7z x Resource.7z -o/opt/qqbot/data/maimai-yuzu-static
```

The current branch does not import official music resources and does not download dxrating/dxdata covers.

## Official raw score import

Raw official user data can be converted to Diving-Fish `/player/update_records` payloads:

```bash
python scripts/convert_official_raw_records.py raw_full_data.json -o update_records.json --report update_records_report.json --pretty
```

The direct upload workflow is:

1. `scripts/sdgb155_full_dump_logout_tool.py` logs in by QR, dumps official raw JSON, and logs out.
2. `scripts/convert_official_raw_records.py` converts raw official records to Diving-Fish payload.
3. `scripts/maimai_update_records_workflow.py` stores QQ-bound Import-Token and uploads to Diving-Fish.
4. `maimai_update_mcp.server` exists only as direct-plugin fallback and should not be added to a global Agent prompt.

The converter deliberately uses only `data/divingfish_song_list.json` for title/type lookup. It does not use official music data or dxdata.

## Local source refresh

```bash
python scripts/update_all_data.py
```

Supported refresh sources in this branch:

- LXNS song and alias snapshots.
- Diving-Fish song list and chart stats.
- Yuzu alias data.
- CN plate whitelist.

Unsupported in this branch:

- dxdata.
- dxrating aliases/tags/covers.
- official unpacked music-data import.
- Japanese-server plate/progress/music-data rendering.
