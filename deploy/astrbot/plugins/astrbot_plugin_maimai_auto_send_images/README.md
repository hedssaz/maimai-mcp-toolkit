# maimai MCP 图片自动发送

这个 AstrBot 插件提供两条 maimai 图片发送路径：

1. 监听 maimai 绘图 MCP 的工具结果并自动发图。
2. 监听高确定性聊天触发词，直接调用当前魔改 MCP 绘图工具，不走 LLM。

直连命中后会在 AstrBot 系统日志里记录工具名、参数、MCP 调用耗时、发图耗时和总耗时；未命中的普通消息默认不记录，避免刷屏。

工具结果自动发图会监听：

- `render_maimai_*`

当工具结果里包含可访问的 `imagePath` 或 `images[].imagePath` 时，插件会立即把图片发送到当前 AstrBot 会话。发送成功后，最终回复里的同一路径和重复图片组件会被删除；如果插件没有安装、没有启用或发送失败，MCP 仍按原样返回路径。

直连触发词默认要求群聊先通过 AstrBot 判定的唤醒条件触发，即各适配器自己的唤醒词、`@bot`，以及 AstrBot 能识别到的“引用/回复 bot 消息”；私聊默认可直接触发。插件读取 AstrBot 已处理过的 `message_str`，并用消息链里的 At 组件补回 `@用户` 的相对位置，不自行硬编码各适配器的预设触发词。`@bot` 只用于唤醒；真实 `@用户` 组件写在命令前、中、后都可以作为查询目标，和命令之间可以没有空格。用户手打的普通文本 `@xxx` 不作为查询目标，会保留给歌名、别名或 query。`曲师查歌`、`谱师查歌`、`bpm查歌` 会直接调用 `maimai-local-search` 返回文本结果，不进入绘图流程。

管理员可以在群聊或私聊中使用 `whitelist add 适配器名 群号` 把当前接收适配器下的群加入直连白名单，例如 `whitelist add napcat2 <群号>`；用 `whitelist del 适配器名 群号` 删除，用 `whitelist list [适配器名]` 查看。这个管理指令必须先用当前适配器的唤醒词或 `@bot` 唤醒，且发送人必须是当前消息实际路由配置文件里的 `admins_id`；它不要求目标群已经在白名单里。进入白名单后的同一 `适配器 ID + 群号` 可以不带唤醒词触发 b50、minfo、ginfo、rank 等直连 MCP 命令，但不会让普通聊天进入 Agent 链路。

命令说明见 [docs/astrbot-direct-render-triggers.md](../../../../docs/astrbot-direct-render-triggers.md)。用户在触发机器人后发送 help / 帮助 / 命令说明，插件会自动回复纯文本分组说明。

常用命令包括 b50、拟合b50、minfo 歌名、ginfo紫 歌名、歌名是什么歌、id296、随个dx紫13、今日舞萌、13+定数表、桃极完成表、13+sss未完成表 2、14+分数列表、我要上分、rank10、musicrank紫白系、divingfishrank 1-30。`拟合b50` 会用拟合定数重算单曲 rating 后重排 B50；`我要上分` 默认使用旧版 `ds - fit_diff` 拟合定数分桶随机算法；MCP 工具显式传 `algorithm:"expected"` 时才启用实验期望收益算法。

成绩导入直连命令只由本插件处理，不会交给 AstrBot Agent 工具管理器：`mai bind <水鱼成绩导入token>` 会把 token 绑定到发送者 QQ；`mai update <二维码解析内容>` 会调用本仓库的 SDGB155 raw dump 脚本登录、拉取成绩、登出，再调用转换脚本生成水鱼 `/player/update_records` payload 并上传。`mai update` 支持在二维码内容后追加 `--keyship <keyship>`、`--logoutid <1或2>`、`--title-ver <标题服务器版本>`。

玩家目标优先级是：真实 @用户 > 前置纯数字 QQ > 后置水鱼 username > 发送者 QQ。QQ 只支持前置；后置纯数字按页码、名次、曲目 ID 或 username 理解，不按 QQ 处理。

需要 AstrBot `>= 4.23.1`，因为插件依赖 `on_llm_tool_respond` 工具响应钩子；直连聊天触发还需要 AstrBot 提供消息事件过滤器。

标准部署脚本会把这个目录复制到 AstrBot `data/plugins/` 下。重启 AstrBot 后，如果全局 `plugin_set` 包含 `*`，插件会自动加载；否则需要在 WebUI 或配置中启用 `astrbot_plugin_maimai_auto_send_images`。

可选配置：

- `direct_render_enabled`: 默认启用，监听聊天触发词并直连 MCP。
- `direct_render_require_wake`: 默认启用，群聊需要先唤醒机器人。
- `direct_render_prefixes`: 额外直连前缀，每行一个，例如 `/mai`。
- `direct_render_group_whitelist_file`: 直连群白名单文件；留空时写入 `direct_render_data_dir/maimai-config/direct-render-group-whitelist.json`。
- `direct_render_project_cwd`: maimai MCP 项目目录，默认 `/AstrBot/data/maimai-mcp`。
- `direct_render_python`: 调用 MCP 使用的 Python 命令。
- `direct_render_use_astrbot_mcp`: 默认启用，优先复用 AstrBot 已连接 MCP，找不到工具时才回退到子进程调用。
- `direct_render_group_module`: 群榜直连回退子进程 MCP 模块，默认 `group_b50_mcp.server`。
- `direct_render_upload_module`: 成绩导入直连回退子进程 MCP 模块，默认 `maimai_update_mcp.server`；插件会强制绕过 AstrBot Agent 工具管理器。
- `direct_render_upload_timeout_seconds`: 成绩导入直连超时时间，默认 `300` 秒。
- `direct_render_import_token_bindings_file`: 水鱼 Import-Token 绑定文件，默认 `direct_render_data_dir/maimai-config/.maimai-import-token-bindings.json`。
- `direct_render_update_records_output_dir`: 成绩导入中间文件目录，默认 `direct_render_data_dir/maimai-record-imports`。
- `direct_render_today_offset`: 今日舞萌偏移值，默认 `0` 按原项目结果；改成其他整数会加到日期项上，让本 Bot 的今日人品和推荐歌区别于其他 Bot。
- `direct_render_log_timing`: 默认启用，记录直连绘图分段耗时和自动发图结果。
- `direct_render_log_unmatched`: 默认关闭，临时开启后记录已唤醒但未匹配直连命令的消息。
- `send_direct_mcp_tools`: 默认启用，监听直接 MCP 绘图工具。
- `send_maimai_subagent_results`: 默认开启，从 `transfer_to_maimai` / `handoff_to_maimai` 返回文本里兜底提取图片路径；用于副 Agent 内部 MCP 调用没有暴露给插件钩子的场景。
- `suppress_tool_result_paths`: 默认开启，插件发送图片后把工具结果中的本地路径替换为“已由插件发送”提示，避免 Agent 再调用 `send_message_to_user`。
- `suppress_duplicate_send_message_calls`: 默认开启，插件已经发送 maimai 图片后，拦截后续 `send_message_to_user` 对同一路径图片的重复发送。
- `send_subagent_results`: 默认关闭，从所有副 Agent 返回文本里兜底提取图片路径。
- `path_prefix_mappings`: 路径前缀映射，每行 `old=>new`。
- `suppress_sent_paths`: 默认启用，隐藏已自动发送的路径。
