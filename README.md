# maimai-mcp-toolkit

本仓库是一组本地 stdio MCP 工具，用于 maimai 查歌、查分、群榜和绘图。

不提供日服曲目或科技传成绩功能。

绘图能力来自 [Yuri-YuzuChaN/maimaiDX](https://github.com/Yuri-YuzuChaN/maimaiDX)，本项目在其绘图代码和资源接口上接入本地 MCP 数据源。

## 项目结构

```
maimai_mcp/           本地查歌、查分、别名搜索
diving_fish_b50_mcp/  Diving-Fish B50 查询
maimaidx_render_mcp/  maimaiDX 绘图渲染（B50卡、曲目信息、分数列表）
qq_identity_mcp/      QQ 号 ↔ 水鱼用户名绑定查询
maimai_score_mcp/     单曲成绩查询
lxns_oauth.py         落雪 OAuth 授权、状态与令牌存储核心
lxns_oauth_mcp/       落雪 OAuth 绑定/状态/解绑 MCP
group_b50_mcp/        群友 B50 排行榜
player_cache/         B50 与完整成绩的本地运行时缓存后端
scripts/              数据刷新、部署安装脚本
data/                 曲库/别名/定数 JSON 快照
test/                 测试用例
docs/                 详细文档
```

## 当前公开版边界

- 不支持 日服曲库、日服牌子/进度或官服曲目资源导入。
- 曲库只使用 落雪/水鱼 数据。
- 落雪 OAuth 仅用于本地绑定、绑定状态和解绑；授权状态与令牌保存在独立的本地 SQLite 中，不查成绩、不接入 SEGA 官方成绩接口，也不包含日服功能。
- 拟合/自算 B50 的 B15 按水鱼曲库里最新的 `basic_info.from` 大版本划分；如果曲库版本名先于代码更新，可用 `MAIMAI_LOCAL_CURRENT_VERSIONS` 或 `MAIMAI_CURRENT_VERSIONS` 覆盖。
- 仓库只保留自定义“雪峰”牌子及其渲染所需的组件图片；其余绘图图片不随源码分发。
- 文档、测试和插件包中的账号/群号示例使用脱敏占位值。
- `player-cache/` 只保存部署实例运行时缓存，默认忽略且不会随源码、Docker 构建或插件包分发。

## 通用行为

- 查歌文本和曲目信息图会区分等级与定数，整数定数固定保留一位小数，例如 `13.0`。
- 标准谱面与 DX 谱面的成绩会同时校验谱面类型，兼容曲目编号不会造成跨类型串谱。
- NapCat 返回 `retcode=1200` 且发送方法等待回执超时时，插件会按“送达状态未知”处理并抑制重复补发。
- 常驻查歌进程会按文件修改时间和大小检测落雪、水鱼、别名及定数统计数据变化，并自动重新加载。

## 常用 MCP 入口

```bash
python -m maimai_mcp.server
python -m diving_fish_b50_mcp.server
python -m maimaidx_render_mcp.server
python -m qq_identity_mcp.server
python -m maimai_score_mcp.server
python -m lxns_oauth_mcp.server
```

## MCP 客户端配置示例

把路径替换成你的本地绝对路径：

```json
{
  "mcpServers": {
    "maimai-local-search": {
      "command": "python",
      "args": ["-m", "maimai_mcp.server"],
      "cwd": "/path/to/maimai"
    },
    "maimaidx-render": {
      "command": "python",
      "args": ["-m", "maimaidx_render_mcp.server"],
      "cwd": "/path/to/maimai"
    },
    "diving-fish-b50": {
      "command": "python",
      "args": ["-m", "diving_fish_b50_mcp.server"],
      "cwd": "/path/to/maimai"
    }
  }
}
```

## 落雪 OAuth 绑定

AstrBot 插件同时支持回调确认和手工提交两种绑定方式：

- 发送 `lxns bind` 生成授权链接。配置回调桥后，插件会轮询回调结果，且只允许原用户在原适配器、原会话中拍一拍当前机器人完成确认。
- 发送 `lxns bind <code>`、`lxns bind code=...` 或 `lxns bind <完整回调 URL>` 可手工完成绑定。
- `lxns status` 只返回绑定/待确认状态，`lxns unbind` 会删除对应授权状态、令牌和待确认记录。

落雪 OAuth 客户端 ID、客户端密钥与回调地址只从运行环境读取，仓库不提供实例默认值：

```dotenv
LXNS_OAUTH_CLIENT_ID=
LXNS_OAUTH_CLIENT_SECRET=
LXNS_OAUTH_REDIRECT_URI=
```

插件使用 `direct_render_oauth_module` 选择 OAuth MCP 模块。自动回调确认还需设置 `direct_render_lxns_callback_poll_url`、`direct_render_lxns_callback_poll_token` 和 `direct_render_lxns_callback_timeout_seconds`；其中共享 Token 的 UTF-8 编码长度不得少于 32 字节。插件默认把 OAuth SQLite 放在 AstrBot 运行数据目录，部署复制与 Docker 构建会排除源码树内的 OAuth 私密运行目录。回调桥提供通用环境变量、Nginx 和 systemd 部署模板，详细说明见 [docs/usage-details.md](docs/usage-details.md)。

## 数据刷新

```bash
python scripts/update_all_data.py
```

## 插件打包

修改 AstrBot 插件源码后，用固定文件白名单重建两个内容一致的可复现 ZIP：

```bash
python scripts/package_astrbot_plugin.py --force
```

## 更多说明

详细部署、参数和 AstrBot 配置见 [docs/usage-details.md](docs/usage-details.md)。

## 静态资源

公开源码仅内置自定义“雪峰”牌子及其渲染依赖的 20 张图片，其他图片已从当前目录和公开分支历史中移除。绘图所需的字体、评分图等静态资源需从 [maimaiDX](https://github.com/Yuri-YuzuChaN/maimaiDX) 下载，解压后放入 `maimaidx_render_mcp/static/`；这些本地图片默认会被 Git 忽略：

- [Cloudreve](https://cloud.yuzuchan.moe/f/34s7/Resource%20CN1.55.7z)
- [OneDrive](https://yuzuai-my.sharepoint.com/:u:/g/personal/yuzu_yuzuchan_moe/IQBGKHie6MAaTZy3rME7Q-ruAVKgXDCKROqz5e25KtMeeVY?e=53eC6a)

## 致谢

- 绘图能力基于 [Yuri-YuzuChaN/maimaiDX](https://github.com/Yuri-YuzuChaN/maimaiDX)（MIT License）
