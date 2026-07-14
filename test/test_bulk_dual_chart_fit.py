"""Bulk verification: for 50+ songs with both SD & DX charts,
ensure fit data is not cross-contaminated between chart types.
"""
import json, sys, unicodedata
from collections import defaultdict

from maimai_mcp.search import (
    load_music_data,
    load_chart_stats,
    all_charts,
    attach_fit_stats,
    serialize_music,
    aliases_for_music,
    load_alias_map,
    unique_preserve_order,
    music_title_values,
    normalize_text,
)


def collect_songs():
    data = load_music_data()
    alias_map = load_alias_map(data)
    chart_stats = load_chart_stats()

    # Find songs that have both SD and DX charts
    dual = []
    for music in data:
        charts = attach_fit_stats(music, all_charts(music), chart_stats)
        types = {c["chart_type"] for c in charts}
        if "standard" in types and "dx" in types:
            aliases = aliases_for_music(alias_map, music)
            serialized = serialize_music(music, charts, aliases)
            dual.append(serialized)
    return dual


def title(music):
    return music.get("title", "?")


def main():
    songs = collect_songs()
    print(f"Found {len(songs)} songs with both SD & DX charts\n")

    if len(songs) < 50:
        print(f"ERROR: only {len(songs)} songs found, need >= 50")
        sys.exit(1)

    passed = 0
    failed = []

    for song in songs:
        charts = song.get("matched_charts", [])
        # Group by difficulty_index + source, compare SD vs DX
        for idx in range(5):  # Basic..Re:Master
            sd = [c for c in charts
                  if c.get("chart_type") == "standard"
                  and c.get("difficulty_index") == idx
                  and c.get("source") == "cn"]
            dx = [c for c in charts
                  if c.get("chart_type") == "dx"
                  and c.get("difficulty_index") == idx
                  and c.get("source") in ("cn", "divingfish")]
            if not sd or not dx:
                continue

            sd_fit = sd[0].get("fit_diff")
            dx_fit = dx[0].get("fit_diff")
            sd_lvl = sd[0].get("level")
            dx_lvl = dx[0].get("level")
            sd_ds = sd[0].get("ds")
            dx_ds = dx[0].get("ds")

            # If both have fit_diff, they must be DIFFERENT (SD != DX)
            if sd_fit is not None and dx_fit is not None and sd_fit == dx_fit:
                failed.append(
                    f"{title(song)} idx={idx}: "
                    f"SD Lv{sd_lvl} ds={sd_ds} fit={sd_fit:.4f} == "
                    f"DX Lv{dx_lvl} ds={dx_ds} fit={dx_fit:.4f} — SAME!"
                )

            # If SD has fit but DX doesn't, that's OK
            # If DX has fit and SD doesn't, also OK (but unusual)
            # If both have fit and they're different → OK

        # Also verify: no chart has fit from a different level
        for chart in charts:
            fit = chart.get("fit_diff")
            if fit is None:
                continue
            lvl = chart.get("level")
            ds = chart.get("ds")
            if lvl is None or ds is None:
                continue

            # Basic sanity: ds and fit should be in same ballpark (±2)
            try:
                ds_val = float(ds)
            except (TypeError, ValueError):
                continue
            if abs(ds_val - fit) > 3.0:
                failed.append(
                    f"{title(song)} {chart.get('chart_type')} {chart.get('difficulty')}: "
                    f"ds={ds} fit={fit:.4f} — diff {abs(ds_val - fit):.2f} > 3!"
                )

    if failed:
        print(f"FAILURES ({len(failed)}):")
        for f in failed:
            print(f"  {f}")
        sys.exit(1)

    # Sample output
    print(f"All {len(songs)} songs verified OK.\n")
    for song in songs[:5]:
        charts = song.get("matched_charts", [])
        print(f"--- {title(song)} ---")
        for c in sorted(charts, key=lambda x: (x.get("difficulty_index", 0), x.get("chart_type", ""))):
            src = c.get("source", "?")
            ctype = c.get("chart_type", "?")
            lvl = c.get("level", "?")
            ds = c.get("ds", "?")
            fit = f"{c['fit_diff']:.2f}" if c.get("fit_diff") is not None else "None"
            print(f"  {src:>4s} {ctype:>8s} Lv{lvl:>4s} ds={str(ds):>5s} fit={fit}")
        print()


if __name__ == "__main__":
    main()
