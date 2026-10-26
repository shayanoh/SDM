"""Tests for scripts/version.py, run on a throwaway copy of the manifests.

    python3 -m unittest scripts/test_version.py
"""

import importlib.util
import json
import shutil
import tempfile
import unittest
from pathlib import Path

HERE = Path(__file__).resolve().parent
REPO = HERE.parent
spec = importlib.util.spec_from_file_location("version", HERE / "version.py")
version = importlib.util.module_from_spec(spec)
spec.loader.exec_module(version)

MANIFESTS = [
    "sdm-core/Cargo.toml",
    "sdm-core/Cargo.lock",
    "sdm-tauri/src-tauri/Cargo.toml",
    "sdm-tauri/src-tauri/tauri.conf.json",
]


def make_tree() -> Path:
    root = Path(tempfile.mkdtemp())
    for rel in MANIFESTS:
        (root / rel).parent.mkdir(parents=True, exist_ok=True)
        shutil.copy(REPO / rel, root / rel)
    for crate in (REPO / "sdm-core/crates").iterdir():
        manifest = crate / "Cargo.toml"
        if manifest.is_file():
            target = root / "sdm-core/crates" / crate.name / "Cargo.toml"
            target.parent.mkdir(parents=True, exist_ok=True)
            shutil.copy(manifest, target)
    return root


class VersionTests(unittest.TestCase):
    def setUp(self):
        self.root = make_tree()
        self.addCleanup(shutil.rmtree, self.root, ignore_errors=True)

    def test_reading_finds_one_version_everywhere_in_the_repo(self):
        report = version.read_all(REPO)
        self.assertTrue(report.in_sync, report.problems)
        self.assertRegex(report.version, r"^\d+\.\d+\.\d+$")

    def test_every_workspace_crate_is_listed_and_inherits_the_version(self):
        names = {c.name for c in version.read_all(REPO).crates}
        self.assertTrue({"sdm-app", "sdm-browser", "sdm-resolve", "sdm-tauri"} <= names)
        self.assertTrue(all(c.inherits for c in version.read_all(REPO).crates))

    def test_setting_changes_every_source(self):
        version.set_version(self.root, "7.8.9", update_lock=False)
        report = version.read_all(self.root)
        self.assertEqual(report.version, "7.8.9")
        conf = json.loads((self.root / "sdm-tauri/src-tauri/tauri.conf.json").read_text())
        self.assertEqual(conf["version"], "7.8.9")
        workspace = (self.root / "sdm-core/Cargo.toml").read_text()
        self.assertIn('version = "7.8.9"', workspace)
        # Path dependencies carry a version requirement too: they must follow.
        app = (self.root / "sdm-core/crates/sdm-app/Cargo.toml").read_text()
        self.assertIn('sdm-browser = { version = "7.8.9"', app)
        self.assertNotIn('version = "0.1.0"', app.replace("serde", ""))
        self.assertEqual(report.stale_dependency_versions, [])

    def test_setting_touches_only_version_lines(self):
        before = (self.root / "sdm-tauri/src-tauri/tauri.conf.json").read_text()
        version.set_version(self.root, "7.8.9", update_lock=False)
        after = (self.root / "sdm-tauri/src-tauri/tauri.conf.json").read_text()
        self.assertEqual(before.replace(version.read_all(REPO).version, "7.8.9", 1), after)

    def test_a_crate_that_pins_its_own_version_is_reported_out_of_sync(self):
        manifest = self.root / "sdm-core/crates/sdm-core/Cargo.toml"
        manifest.write_text(manifest.read_text().replace("version.workspace = true", 'version = "9.9.9"'))
        report = version.read_all(self.root)
        self.assertFalse(report.in_sync)
        self.assertTrue(any("sdm-core" in p for p in report.problems))

    def test_a_tauri_config_that_disagrees_is_reported_out_of_sync(self):
        conf = self.root / "sdm-tauri/src-tauri/tauri.conf.json"
        conf.write_text(conf.read_text().replace(version.read_all(REPO).version, "3.3.3", 1))
        report = version.read_all(self.root)
        self.assertFalse(report.in_sync)
        self.assertTrue(any("tauri.conf.json" in p for p in report.problems))

    def test_setting_repairs_an_out_of_sync_tree(self):
        conf = self.root / "sdm-tauri/src-tauri/tauri.conf.json"
        conf.write_text(conf.read_text().replace(version.read_all(REPO).version, "3.3.3", 1))
        version.set_version(self.root, "4.0.0", update_lock=False)
        self.assertTrue(version.read_all(self.root).in_sync)

    def test_bad_versions_are_refused_and_nothing_changes(self):
        before = (self.root / "sdm-core/Cargo.toml").read_text()
        for bad in ["", "1", "1.2", "1.2.3.4", "v1.2.3", "1.2.x", "1.2.3-beta", " 1.2.3", "01.2.3"]:
            with self.assertRaises(ValueError, msg=bad):
                version.set_version(self.root, bad, update_lock=False)
        self.assertEqual(before, (self.root / "sdm-core/Cargo.toml").read_text())

    def test_the_lock_file_is_reported_when_it_lags_behind(self):
        version.set_version(self.root, "7.8.9", update_lock=False)
        report = version.read_all(self.root)
        self.assertTrue(report.lock_stale)
        self.assertTrue(report.in_sync, "a stale lock is a note, not a mismatch")


if __name__ == "__main__":
    unittest.main()
