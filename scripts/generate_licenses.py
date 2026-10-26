#!/usr/bin/env python3
"""Writes sdm-tauri/src-tauri/licenses.txt: the licenses of everything compiled into SDM.

The Rust crates come from `cargo about` (config and template in sdm-core/about.toml
and about.hbs), the web libraries from `pnpm licenses`. The result is committed
and shipped as a resource of every package; the About window shows it. Run it
after adding or updating a dependency:

    python3 scripts/generate_licenses.py

Needs `cargo install --locked cargo-about --features cli`. A dependency whose
license is not in `accepted` in sdm-core/about.toml fails the run.
"""

import json
import subprocess
import sys
from pathlib import Path

ROOT = Path(__file__).resolve().parent.parent
OUTPUT = ROOT / "sdm-tauri" / "src-tauri" / "licenses.txt"
LICENSE_FILE_NAMES = ("LICENSE", "LICENSE.md", "LICENSE.txt", "LICENCE", "license", "LICENSE-MIT")

HEADER = """SDM is open source under the MIT License (see the LICENSE file).

SDM is built with the open-source libraries below. Each is used under its own
license, listed here with the libraries that use it. The tools SDM downloads
at run time (yt-dlp, FFmpeg, QuickJS-ng) are separate programs and are not part
of this list.

"""


def rust_part() -> str:
    done = subprocess.run(
        ["cargo", "about", "generate", "--workspace", "--fail", "about.hbs"],
        cwd=ROOT / "sdm-core",
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        sys.exit(f"cargo about failed:\n{done.stderr}")
    return done.stdout


def license_text(folder: Path) -> str:
    for name in LICENSE_FILE_NAMES:
        path = folder / name
        if path.is_file():
            return path.read_text(errors="replace").strip()
    return ""


def web_part() -> str:
    done = subprocess.run(
        ["pnpm", "licenses", "list", "--prod", "--json"],
        cwd=ROOT / "sdm-tauri" / "web",
        capture_output=True,
        text=True,
    )
    if done.returncode != 0:
        sys.exit(f"pnpm licenses failed:\n{done.stderr}")
    by_text: dict[str, list[str]] = {}
    for license_name, packages in json.loads(done.stdout).items():
        for package in packages:
            text = license_text(Path(package["paths"][0])) or f"License: {license_name}"
            for version in package["versions"]:
                by_text.setdefault((license_name, text), []).append(f'{package["name"]} {version}')
    out = []
    for (license_name, text), names in sorted(by_text.items()):
        out.append("-" * 80)
        out.append(f"{license_name} (web libraries)\n")
        out.append("Used by:")
        out.extend(f"  {name}" for name in sorted(names))
        out.append("")
        out.append(text)
        out.append("")
    return "\n".join(out)


def main() -> None:
    OUTPUT.write_text(HEADER + rust_part().strip() + "\n\n" + web_part().strip() + "\n")
    print(f"wrote {OUTPUT.relative_to(ROOT)} ({OUTPUT.stat().st_size // 1024} KB)")


if __name__ == "__main__":
    main()
