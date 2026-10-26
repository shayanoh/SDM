"""Every file of the Chrome extension that matters is in the app bundle, and no test or hidden file is.

`tauri.conf.json` lists the extension's files one by one (a whole-folder entry
shipped `.DS_Store` and the test file in the Windows installer), so a new file
in `SDMChromeExtension/` has to be added there too. This fails until it is.

    python3 -m unittest scripts/test_bundle_resources.py
"""

import json
import unittest
from fnmatch import fnmatch
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
EXTENSION = ROOT / "SDMChromeExtension"
CONFIG = ROOT / "sdm-tauri" / "src-tauri" / "tauri.conf.json"
PREFIX = "../../SDMChromeExtension/"


def shipped_patterns():
    resources = json.loads(CONFIG.read_text())["bundle"]["resources"]
    return [source[len(PREFIX):] for source in resources if source.startswith(PREFIX)]


def extension_files():
    """What the extension needs at run time: no hidden files, no tests."""
    return sorted(
        path.relative_to(EXTENSION).as_posix()
        for path in EXTENSION.rglob("*")
        if path.is_file()
        and not any(part.startswith(".") for part in path.relative_to(EXTENSION).parts)
        and not path.name.endswith(".test.js")
    )


class BundleResources(unittest.TestCase):
    def test_every_runtime_file_is_shipped(self):
        patterns = shipped_patterns()
        missing = [f for f in extension_files() if not any(fnmatch(f, p) for p in patterns)]
        self.assertEqual(missing, [], "add these to bundle.resources in tauri.conf.json")

    def test_no_pattern_can_pick_up_a_test_or_hidden_file(self):
        for pattern in shipped_patterns():
            self.assertFalse(pattern.endswith("/"), pattern)
            self.assertNotIn(".test.", pattern)
            self.assertFalse(any(part.startswith(".") for part in pattern.split("/")), pattern)
            # A glob must name an extension, or it would take dotfiles beside the files it means.
            if "*" in pattern:
                self.assertRegex(pattern, r"\*\.[a-z]+$")


if __name__ == "__main__":
    unittest.main()
