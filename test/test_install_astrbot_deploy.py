from __future__ import annotations

import tempfile
import unittest
from pathlib import Path

from scripts.install_astrbot_deploy import copy_project


PROJECT_ROOT = Path(__file__).resolve().parents[1]


class InstallAstrBotDeployTests(unittest.TestCase):
    def test_copy_project_excludes_only_private_oauth_runtime_directories(self) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            project = root / "project"
            target = root / "target"

            ordinary_files = {
                "service.py": "print('ready')\n",
                "data/music_data.json": "{}\n",
                "data/catalog-cache.sqlite3": "ordinary cache\n",
            }
            private_files = {
                "data/.lxns-oauth/oauth.sqlite3": "private database\n",
                "data/.lxns-oauth/oauth.sqlite3-wal": "private wal\n",
                "data/.lxns-oauth/oauth.sqlite3-shm": "private shm\n",
                "data/.lxns-oauth/oauth.sqlite3-journal": "private journal\n",
                "data/.lxns-oauth/nested/session.json": "private session\n",
                "lxns-oauth-callback/callbacks.sqlite3": "private callback database\n",
                "lxns-oauth-callback/callbacks.sqlite3-wal": "private callback wal\n",
                "lxns-oauth-callback/callbacks.sqlite3-shm": "private callback shm\n",
                "lxns-oauth-callback/callbacks.sqlite3-journal": "private callback journal\n",
            }
            for relative_path, content in (ordinary_files | private_files).items():
                source = project / relative_path
                source.parent.mkdir(parents=True, exist_ok=True)
                source.write_text(content, encoding="utf-8")

            copy_project(project, target)

            for relative_path in ordinary_files:
                self.assertTrue((target / relative_path).is_file(), relative_path)
            for relative_path in private_files:
                self.assertFalse((target / relative_path).exists(), relative_path)
            self.assertFalse((target / "data/.lxns-oauth").exists())
            self.assertFalse((target / "lxns-oauth-callback").exists())

    def test_copy_project_does_not_follow_private_runtime_directory_symlink(
        self,
    ) -> None:
        with tempfile.TemporaryDirectory() as temp_dir:
            root = Path(temp_dir)
            project = root / "project"
            private_source = root / "private-source"
            target = root / "target"
            (project / "data").mkdir(parents=True)
            private_source.mkdir()
            (private_source / "oauth.sqlite3").write_text(
                "private database\n",
                encoding="utf-8",
            )
            (project / "data/.lxns-oauth").symlink_to(
                private_source,
                target_is_directory=True,
            )

            copy_project(project, target)

            self.assertFalse((target / "data/.lxns-oauth").exists())

    def test_dockerignore_scopes_oauth_runtime_exclusions(self) -> None:
        rules = {
            line.strip()
            for line in (PROJECT_ROOT / ".dockerignore")
            .read_text(encoding="utf-8")
            .splitlines()
            if line.strip() and not line.lstrip().startswith("#")
        }

        self.assertIn("data/.lxns-oauth/", rules)
        self.assertIn("lxns-oauth-callback/", rules)
        self.assertNotIn("data/", rules)
        self.assertFalse(
            any(rule in rules for rule in {"*.db", "*.sqlite", "*.sqlite3"})
        )


if __name__ == "__main__":
    unittest.main()
