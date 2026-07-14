# Contributing

欢迎提交 Issue 和 PR。

## 开发环境

```bash
git clone https://github.com/hedssaz/maimai-mcp-toolkit.git
cd maimai-mcp-toolkit
python -m venv .venv
source .venv/bin/activate
pip install -e ".[dev]"
```

## 运行测试

```bash
python -m unittest discover -s test -p 'test_*.py'
```

## 代码风格

- Python 3.10+
- 使用 `pyproject.toml` 管理依赖
- 保持 MCP 服务入口一致：每个模块提供 `server.py` 作为主入口
