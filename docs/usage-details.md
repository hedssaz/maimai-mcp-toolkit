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
