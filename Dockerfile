FROM python:3.12-slim

ENV PYTHONDONTWRITEBYTECODE=1
ENV PYTHONUNBUFFERED=1

RUN apt-get update \
  && apt-get install -y --no-install-recommends ca-certificates \
  && rm -rf /var/lib/apt/lists/*

RUN python -m pip install --no-cache-dir "pypinyin>=0.50"

WORKDIR /app

COPY pyproject.toml README.md ./
COPY maimai_mcp ./maimai_mcp
COPY scripts ./scripts
COPY data/ ./data/

EXPOSE 8000

CMD ["python", "-m", "maimai_mcp.http_server", "--host", "0.0.0.0", "--port", "8000"]
