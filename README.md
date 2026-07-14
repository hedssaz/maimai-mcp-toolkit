# maimai-mcp-toolkit

本仓库是一组本地 stdio MCP 工具，用于 maimai 查歌、查分、群榜、绘图和水鱼成绩上传工作流。

没有日服曲目和科技传成绩功能

绘图能力来自 [Yuri-YuzuChaN/maimaiDX](https://github.com/Yuri-YuzuChaN/maimaiDX)，本项目在其绘图代码和资源接口上接入本地 MCP 数据源。

## 项目结构

```
maimai_mcp/           本地查歌、查分、别名搜索
diving_fish_b50_mcp/  Diving-Fish B50 查询
maimaidx_render_mcp/  maimaiDX 绘图渲染（B50卡、曲目信息、分数列表）
qq_identity_mcp/      QQ 号 ↔ 水鱼用户名绑定查询
maimai_score_mcp/     单曲成绩查询
maimai_update_mcp/    官服 raw 成绩 → 水鱼上传
group_b50_mcp/        群友 B50 排行榜
scripts/              数据刷新、部署安装脚本
data/                 曲库/别名/定数 JSON 快照
test/                 测试用例
docs/                 详细文档
```

## 当前公开版边界

- 保留官服 raw 成绩导出转换并上传到 Diving-Fish `/player/update_records` 的工作流。
- 不支持 日服曲库、日服牌子/进度或官服曲目资源导入。
- 曲库只使用 落雪/水鱼 数据。
- 拟合/自算 B50 的 B15 按水鱼曲库里最新的 `basic_info.from` 大版本划分；如果曲库版本名先于代码更新，可用 `MAIMAI_LOCAL_CURRENT_VERSIONS` 或 `MAIMAI_CURRENT_VERSIONS` 覆盖。
- 仓库只保留自定义“雪峰”牌子及其渲染所需的组件图片；其余绘图图片不随源码分发。
- 文档、测试和插件包中的账号/群号示例使用脱敏占位值。

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
```

官服成绩上传直连 fallback 入口：

```bash
python -m maimai_update_mcp.server
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

## 数据刷新

```bash
python scripts/update_all_data.py
```

## 更多说明

详细部署、参数和 AstrBot 配置见 [docs/usage-details.md](docs/usage-details.md)。

## 静态资源

公开源码仅内置自定义“雪峰”牌子及其渲染依赖的 20 张图片，其他图片已从当前目录和公开分支历史中移除。绘图所需的字体、评分图等静态资源需从 [maimaiDX](https://github.com/Yuri-YuzuChaN/maimaiDX) 下载，解压后放入 `maimaidx_render_mcp/static/`；这些本地图片默认会被 Git 忽略：

- [Cloudreve](https://cloud.yuzuchan.moe/f/34s7/Resource%20CN1.55.7z)
- [OneDrive](https://yuzuai-my.sharepoint.com/:u:/g/personal/yuzu_yuzuchan_moe/IQBGKHie6MAaTZy3rME7Q-ruAVKgXDCKROqz5e25KtMeeVY?e=53eC6a)

## 致谢

- 绘图能力基于 [Yuri-YuzuChaN/maimaiDX](https://github.com/Yuri-YuzuChaN/maimaiDX)（MIT License）
