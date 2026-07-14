from __future__ import annotations

import subprocess
import sys
from pathlib import Path


ROOT = Path(__file__).resolve().parent.parent


def run(command: list[str]) -> None:
    print("+ " + " ".join(command), flush=True)
    subprocess.run(command, cwd=ROOT, check=True)


def regenerate_derived_files() -> None:
    # There is no generated merged cache in the current project. Compile the package
    # so syntax errors are caught immediately after data updates.
    run([sys.executable, "-m", "compileall", "-x", r"(^|/)\._", "maimai_mcp"])


def main() -> None:
    run([sys.executable, "scripts/update_music_data.py"])
    run([sys.executable, "scripts/update_yuzu_alias_data.py"])
    run([sys.executable, "scripts/update_divingfish_data.py"])
    run([sys.executable, "scripts/update_chart_stats.py"])
    run([sys.executable, "scripts/update_plate_data.py"])
    regenerate_derived_files()


if __name__ == "__main__":
    main()
