# Changelog

## 2026-06-08

- Changed readable search output to include chart-specific IDs in ST/DX labels, so DX candidates render as 5-digit IDs such as `DX #10835` even though internal source merging still uses the normalized base song ID.
- Changed official resource imports to copy orphan `jackets/*.png` files even when no `music_data.json` record exists for that cover, enabling cover-only render fallbacks.
- Added optional official unpacked music resource support. `scripts/import_official_music_resources.py` converts `extracted_music_resources/` into a dxdata-like `data/official_music_data.json`, groups ordinary ST/DX by `nonDxId`, preserves utage charts by official ID, copies jacket PNGs for render use, and local search now loads this source between LXNS and dxdata.

## 2026-05-29

- Changed readable search output to show chart types as `谱面 ST/DX`; compact output now splits difficulty summaries into `ST:` and `DX:` groups so callers can tell when a song has both standard and DX charts without switching to JSON.
- Changed `image_url` to use the current dxrating cover CDN format: `https://shama.dxrating.net/images/cover/v2/{imageName}.jpg` for extensionless dxdata cover hashes.
- Added `id` / `id_min` / `id_max` song-level filter to `search_maimai_songs`. The filter compares against any integer ID present in any source (LXNS int, DivingFish string-of-digits). dxdata-only entries whose only ID is the `songId` string are excluded since they don't have a numeric ID.
- Added `bpm` / `bpm_min` / `bpm_max` song-level filter, taking the first parseable float bpm across sources by priority.
- Added `is_locked` filter that reads dxdata's per-song `isLocked` field. Songs only present in LXNS (no dxdata record) are treated as unlocked.
- Added `songId` to query matching: an exact normalized match against dxdata's `songId` (which is the song title) now ranks at the highest tier alongside title matches.
- Added dxdata's `searchAcronyms` to both the alias pool and keyword pool used by query ranking, so abbreviations like "raven" find "Raven Emperor".
- Added `image_name` and `image_url` to `serialize_music` output. `image_url` is built from `https://dxrating.net/images/cover/v2/{imageName}` whenever the dxdata source provides a cover hash.
- Added `is_locked` field to `serialize_music` output (null when dxdata has no record of the song).
- Added MCP `query_chart_history` for querying historical chart constant (定数) changes across game versions. Based on dxdata's `multiverInternalLevelValue` field (2,229 charts with multi-version data). Consecutive versions with the same ds value are automatically collapsed into one line (e.g., `Splash / Splash PLUS / UNiVERSE: 13.3`).
- Added `load_dxdata_version_order()` to build chronological version ordering from dxdata metadata's `releaseDate`.
- Added `collapse_multiver_history()` helper for grouping consecutive same-ds versions.
- Updated `AGENT_MCP_GUIDE.md` with `query_chart_history` tool description, usage rules, and examples.
- Replaced the MaiDB JP supplemental source with dxrating's curated `dxdata.json` (https://github.com/gekichumai/dxrating). The new source is fetched as plain JSON from GitHub raw instead of scraping MaiDB's hydration script, so it survives MaiDB frontend changes and ships with per-sheet `releaseDate`, `regionOverrides`, `internalId` and `multiverInternalLevelValue` fields that MaiDB lacked.
- Added `scripts/update_dxdata.py` (pure Python, no node dependency) and removed `scripts/update_maidb_jp_data.mjs` along with the cached `data/maidb_jp_songs.json`. Both `update_all_data.py` and `refresh_maimai_sources` now route the JP source through the new script.
- Changed `SOURCE_NAMES["jp"]` from `maidb` to `dxdata`; `SONG_SOURCE_LABELS` now reports `日服源 (dxrating)`. The `maidb` / `jp` / `日服` / `日版` source aliases in `refresh_maimai_sources` are kept for backward compatibility and route to `dxdata`.
- Changed `iter_maidb_charts` to `iter_dxdata_charts`. The new iterator drops MaiDB-only fields (`kanji`, `description`, `isBuddy`) which dxdata does not provide, and gains chart-level `release_date`, `is_special` (utage marker), `internal_id`, `region_overrides` and `multiver_internal_level_value` passthroughs.
- Changed `SOURCE_FIELD_MAPS["jp"]` to read song-level `release_date` and `version` from derived fields populated during load: `release_date` is the earliest `sheet.releaseDate` and `version` is the first non-empty `sheet.version`, preserving the song-level abstraction callers depend on.

## 2026-05-26

- Added local MCP stdio server exposing `search_maimai_songs`.
- Added local CLI search by ID, fuzzy title/alias, level, chart constant, difficulty, and chart type.
- Switched primary song and alias data to LXNS public APIs so IDs, constants, chart types, notes, and maintained aliases are available locally.
- Kept Yuzu aliases and legacy CSV aliases as supplemental fallback sources only.
- Added dependency-free HTTP `/search` service and Dockerfile for server deployment.
- Added MaiDB supplemental JP/INTL song cache for titles missing from LXNS, including CiRCLE PLUS songs without requiring aliases.
- Added local alias editing through HTTP `POST /alias` and MCP `add_maimai_alias`, persisted in `data/custom_aliases.json`.
- Added a local graphical web panel served by the HTTP service at `/`, with search filters, result details, and custom alias writing.
- Added MCP `random_maimai_songs` for random song picks and `list_maimai_songs_by_id` for ID-ordered song listing. Random picks use song-ID mode when no chart filter is supplied, and chart-filtered mode when `level` or `ds` is supplied.
- Added `genre` filtering to search, random picks, ID-ordered listing, HTTP search, CLI search, and the local graphical panel.
- Added `version` filtering to search, random picks, ID-ordered listing, HTTP search, CLI search, and the local graphical panel.
- Added MCP `list_maimai_versions` and HTTP `/versions` to list all local song/chart versions.
- Removed the obsolete `data/music_data.json` cache.
- Changed LXNS/MaiDB merging so matching titles return one song with `source_fields.cn` and `source_fields.jp` instead of dropping the duplicate JP record.
- Documented the LXNS numeric version-code meaning for Agent usage.
- Refined the maimai MCP Skill prompt with source-field reading rules and complete random-filter behavior.
- Expanded `search_maimai_songs` text output: `format_song` now emits release_date, is_new, genre, and available_chart_types; `format_chart` appends chart-level version, BUDDY flag, kanji, and description so callers can answer "when was this chart added" or "is this 宴谱" without switching to `format=json`.
- Translated English/Romaji prefixes in `search_maimai_songs` text output to Chinese (编号/来源/地区/版本/上线/新曲/流派/谱面/等级/定数/拟合/差值/合计/谱师/字标/说明/双人谱). Kept BPM, SD/DX, difficulty names and note types (Tap/Hold/Slide/Touch/Break) as universal music-game terms.
- Added DivingFish (`/music_data` API) as independent national-server data source with etag-based incremental refresh, accessible via `refresh_maimai_sources` with aliases `divingfish`/`水鱼`/`df`.
- Introduced source field mapping table (`SOURCE_FIELD_MAPS`) so each source maps its own key paths to unified field names; `serialize_music()` and `source_field_summary()` now use `_resolve_field()` for cross-source fallback instead of reading only from the primary source.
- `source_fields` now includes `legacy` (DivingFish) alongside `cn` and `jp`; `format_source_differences()` displays all three sources' version names and ds values.
- Added `is_new` filter to `search_maimai_songs` for querying new songs (e.g., `is_new: true` returns only new songs).
- Added `is_new_source` parameter to specify which server to check for `is_new`: `cn` for national server, `jp` for international server.
- `format_source_differences()` now shows differences for version, genre, release_date, is_new, and ds across all sources, with source labels when values differ.
- Packaged the maimai MCP Skill as `dist/maimai-mcp-skill.zip`.
- Expanded the packaged Skill with complete usage instructions for every MCP tool, parameters, result fields, source merging, and response style.
- Merged the maimai DX scoring MCP tools into the local MCP server as `score_counts` and `find_score_combinations`.
- Added direct song lookup to `find_score_combinations`: pass `query`/`song_id`/`title`, calculate only on one song and one chart, otherwise return song/chart candidates without scoring.
- Updated scoring to support `touch` as its own note type with Tap-equivalent values, and to normalize `touch_hold` aliases to Hold instead of merging Touch into Tap.
- Updated the maimai MCP Skill and Agent guide to route achievement reverse searches through the existing `find_score_combinations` tool instead of adding a separate MCP/tool.
- Replaced the old public scoring names with the current schema: `old` for old-frame/FiNALE raw score, `oldacc` for old-frame percentage, `dxscore` for DX SCORE, and `dxacc` for DX achievement.
- Removed old public scoring names from the accepted schema: use `old` instead of `total`, `dxscore` instead of `dx`, and `dxacc` instead of `achievement`.
- Added `oldacc` old-frame percentage mode to both `score_counts` and `find_score_combinations`.
- Updated `dxacc` / `oldacc` reverse search to accept either scaled integer targets such as `1004999` or percentage strings such as `100.4999%`.
- Simplified the public `score_counts` schema so normal calls only provide judgment counts and receive all main totals (`oldscore`, `oldacc`, `dxscore`, `dxacc`) in one response; `find_score_combinations` remains the mode-selected reverse-search tool.
- Renamed the old-frame raw-score field and mode from `old` to `oldscore` in public outputs and `find_score_combinations` schema.
- Collapsed equal-value non-Break judgment labels in scoring outputs: Tap/Touch/Hold/Slide use `great` and `perfect`; Break keeps high/mid/low labels because their values differ.
- Fixed Break judgment handling so `perfect_low` and `perfect_high` are not collapsed, and ambiguous exact Break counts such as `great` or `perfect` are rejected.
- Documented readable MCP tool output as the first prerequisite for the upcoming fit-difficulty and region-filtering work.
- Added a progress-tracking rule for fit-difficulty work: completed implementation and validation items must be checked off in the requirements document.
- Removed duplicate packaged Skill prompt files and the old Skill zip so `AGENT_MCP_GUIDE.md` is the only maintained Agent prompt.
- Updated `AGENT_MCP_GUIDE.md` to treat the merged maimai MCP as the only maintained tool surface, require complete MCP result forwarding, and forbid hand-calculated fallbacks after reverse-search overflow.
- Tracked the later `AGENT_MCP_GUIDE.md` JSON-reading cleanup under the readable MCP output requirements.
- Changed MCP tool results to default to readable text summaries, with `format=json`, `include_raw=true`, or `debug=true` available for raw JSON output.
- Updated `AGENT_MCP_GUIDE.md` to treat readable MCP text as the default result surface and reserve raw JSON for debugging.
- Added Diving-Fish `chart_stats` caching and attached `fit_diff`, `fit_delta`, and `fit_label` to matched song charts.
- Added fitted-constant, actual-vs-fit delta, virtual-high/virtual-low, region availability, and fit sorting filters to search, random song, ID listing, HTTP API, MCP schemas, and the local web panel.
- Added `scripts/update_chart_stats.py` and `scripts/update_all_data.py`; the one-click updater refreshes LXNS, MaiDB, and Diving-Fish data, then runs a compile check without touching `data/custom_aliases.json`.
- Updated LXNS data refresh to accept the current `songs` / `aliases` wrapped API response format.
- Fixed fit-stat attachment so UTAGE charts are not matched against normal Diving-Fish difficulty slots.
- Fixed negative `fit_delta` range parsing, including ranges such as `-0.3--0.1`.
- Updated README, Agent MCP guide, and fit-difficulty requirements progress tracking for the fitted-constant and region-filtering rollout.
- Deployed the fitted-constant update to the AstrBot-mounted MCP directory on the server and restarted AstrBot; the standalone HTTP Docker service now runs with `/opt/maimai-local-search` bind-mounted at `/app`.
- Clarified Agent MCP guide terminology so 水歌/吃分歌 maps to `虚高`, while 诈称/难歌 maps to `虚低`.
- Added LXNS year-version filtering so `version=2025`, `version=25`, `version=dx2025`, `version=dx25`, or `version=2025/25` matches all `25xxx` song and chart versions.
- Added LXNS lettered sub-version aliases such as `25-A` / `25-a` / `dx25-A` for `25000`, `25-B` for `25001`, and `25-C` for `25002`.
- Clarified Agent MCP guide terminology so `maimaiでらっくす` / DX 无印 maps to the CN/LXNS `20xxx` version segment, not a JP text version search.
- Added `find_score_combinations` shortcut constraints for all-note no miss/good searches and Break non-Critical caps such as “Break 最多 5 个 Perfect 及以下”.
- Added MCP `list_maimai_aliases` to return complete alias lists for songs matched by ID, title, or alias without relying on raw search JSON.
- Added MCP `refresh_maimai_sources` and HTTP `/source-status` / `/refresh-sources` for manual source refreshes with a default 3-day source TTL.
- Added Yuzu aliases as a refreshable source through `scripts/update_yuzu_alias_data.py` and `refresh_maimai_sources` source `yuzu`.
- Added output-level alias de-duplication for alias displays so repeated aliases from multiple sources are shown only once.
- Added MCP `batch_search_maimai_songs` so callers can run many enhanced song/chart searches in one local-search MCP call while preserving request context.
- Cached local song, alias, and chart-stat data inside the MCP process so batch searches do not repeatedly reload the same JSON files.
- Changed `refresh_maimai_sources` to refresh due sources concurrently and report per-source failures, with a shorter default per-source timeout.
