#!/usr/bin/env python3
"""Shows or sets the version of the Tauri builds.

    scripts/version.py            show the version and whether everything agrees
    scripts/version.py 1.2.3      set it everywhere
    scripts/version.py --short    print just the version (for build scripts)

One number versions the Windows exe and installer, the macOS bundle and the
Linux packages: Tauri reads it from `tauri.conf.json`. Every Rust crate takes
it from `[workspace.package]` in `sdm-core/Cargo.toml`; the path dependencies
between crates repeat it as a version requirement, and `Cargo.lock` records
it. All of those are kept equal here.

Not touched, because they version something else: the Chrome extension
(`SDMChromeExtension/manifest.json`), the SwiftUI app (`MARKETING_VERSION` in
the Xcode project) and the npm manifests.
"""

import json
import re
import subprocess
import sys
from dataclasses import dataclass, field
from pathlib import Path

WORKSPACE_MANIFEST = "sdm-core/Cargo.toml"
CRATES_DIR = "sdm-core/crates"
TAURI_MANIFEST = "sdm-tauri/src-tauri/Cargo.toml"
TAURI_CONFIG = "sdm-tauri/src-tauri/tauri.conf.json"
LOCK_FILE = "sdm-core/Cargo.lock"

VERSION = re.compile(r"^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$")


@dataclass
class Crate:
    name: str
    manifest: Path
    inherits: bool
    own_version: str | None


@dataclass
class Report:
    version: str
    crates: list[Crate]
    tauri_config: str | None
    stale_dependency_versions: list[tuple[str, str, str]] = field(default_factory=list)
    lock_stale: bool = False
    problems: list[str] = field(default_factory=list)

    @property
    def in_sync(self) -> bool:
        return not self.problems


def _manifests(root: Path) -> list[Path]:
    found = sorted((root / CRATES_DIR).glob("*/Cargo.toml"))
    found.append(root / TAURI_MANIFEST)
    return [m for m in found if m.is_file()]


def _package_section(text: str) -> str:
    match = re.search(r"^\[package\]\n(.*?)(?=^\[|\Z)", text, re.S | re.M)
    return match.group(1) if match else ""


def _crate(manifest: Path) -> Crate:
    section = _package_section(manifest.read_text())
    name = re.search(r'^name\s*=\s*"([^"]+)"', section, re.M)
    own = re.search(r'^version\s*=\s*"([^"]+)"', section, re.M)
    return Crate(
        name=name.group(1) if name else manifest.parent.name,
        manifest=manifest,
        inherits=re.search(r"^version\.workspace\s*=\s*true", section, re.M) is not None,
        own_version=own.group(1) if own else None,
    )


def _workspace_version(root: Path) -> str:
    text = (root / WORKSPACE_MANIFEST).read_text()
    section = re.search(r"^\[workspace\.package\]\n(.*?)(?=^\[|\Z)", text, re.S | re.M)
    found = re.search(r'^version\s*=\s*"([^"]+)"', section.group(1), re.M) if section else None
    if not found:
        raise ValueError(f"no [workspace.package] version in {WORKSPACE_MANIFEST}")
    return found.group(1)


def _path_dependencies(manifest: Path, names: set[str]):
    """(line match, crate name, version requirement) for each version-pinned path dependency."""
    pattern = re.compile(
        r'^(?P<name>[A-Za-z0-9_-]+)\s*=\s*\{(?P<body>[^}\n]*\bpath\s*=[^}\n]*)\}', re.M
    )
    for match in pattern.finditer(manifest.read_text()):
        if match.group("name") not in names:
            continue
        req = re.search(r'\bversion\s*=\s*"([^"]*)"', match.group("body"))
        if req:
            yield match, match.group("name"), req.group(1)


def _lock_versions(root: Path, names: set[str]) -> dict[str, str]:
    lock = root / LOCK_FILE
    if not lock.is_file():
        return {}
    found = {}
    for block in re.finditer(r'\[\[package\]\]\nname = "([^"]+)"\nversion = "([^"]+)"', lock.read_text()):
        if block.group(1) in names:
            found[block.group(1)] = block.group(2)
    return found


def read_all(root: Path) -> Report:
    root = Path(root)
    version = _workspace_version(root)
    crates = [_crate(m) for m in _manifests(root)]
    names = {c.name for c in crates}
    conf = json.loads((root / TAURI_CONFIG).read_text()).get("version")

    report = Report(version=version, crates=crates, tauri_config=conf)
    for crate in crates:
        if not crate.inherits:
            report.problems.append(
                f"{crate.name} sets its own version ({crate.own_version or 'none'}), not the workspace's"
            )
    if conf != version:
        report.problems.append(f"tauri.conf.json says {conf}, the crates say {version}")
    for crate in crates:
        for _, dep, req in _path_dependencies(crate.manifest, names):
            if req != version:
                where = str(crate.manifest.relative_to(root))
                report.stale_dependency_versions.append((where, dep, req))
                report.problems.append(f"{where} requires {dep} {req}, not {version}")
    report.lock_stale = any(v != version for v in _lock_versions(root, names).values())
    return report


def set_version(root: Path, new: str, update_lock: bool = True) -> Report:
    """Sets `new` everywhere; refuses (changing nothing) unless it is MAJOR.MINOR.PATCH."""
    if not isinstance(new, str) or not VERSION.match(new):
        raise ValueError(f"{new!r} is not a version like 1.2.3")
    root = Path(root)
    names = {_crate(m).name for m in _manifests(root)}

    workspace = root / WORKSPACE_MANIFEST
    text = workspace.read_text()
    text, count = re.subn(
        r'(^\[workspace\.package\]\n(?:(?!^\[).*\n)*?version\s*=\s*")[^"]+(")',
        rf"\g<1>{new}\g<2>",
        text,
        count=1,
        flags=re.M,
    )
    if count != 1:
        raise ValueError(f"no [workspace.package] version in {WORKSPACE_MANIFEST}")
    workspace.write_text(text)

    for manifest in _manifests(root):
        text = manifest.read_text()
        # A crate with its own version goes back to following the workspace's.
        package = _package_section(text)
        if re.search(r'^version\s*=\s*"', package, re.M):
            fixed = re.sub(r'^version\s*=\s*"[^"]*"', "version.workspace = true", package, count=1, flags=re.M)
            text = text.replace(package, fixed, 1)
        for match, _, req in reversed(list(_path_dependencies(manifest, names))):
            old = match.group(0)
            text = text.replace(old, re.sub(r'(\bversion\s*=\s*")[^"]*(")', rf"\g<1>{new}\g<2>", old, count=1), 1)
        manifest.write_text(text)

    conf = root / TAURI_CONFIG
    text, count = re.subn(r'("version"\s*:\s*")[^"]+(")', rf"\g<1>{new}\g<2>", conf.read_text(), count=1)
    if count != 1:
        raise ValueError(f"no version in {TAURI_CONFIG}")
    conf.write_text(text)

    if update_lock:
        done = subprocess.run(
            ["cargo", "update", "--workspace", "--offline"],
            cwd=root / "sdm-core",
            capture_output=True,
            text=True,
        )
        if done.returncode != 0:
            raise RuntimeError(f"cargo update failed, Cargo.lock still has the old version:\n{done.stderr}")
    return read_all(root)


def describe(report: Report) -> str:
    lines = [f"Version: {report.version}"]
    lines.append("  crates:      " + ", ".join(sorted(c.name for c in report.crates)))
    lines.append(f"  tauri.conf.json: {report.tauri_config}  (versions the Windows exe and installer, the macOS bundle and the Linux packages)")
    if report.lock_stale:
        lines.append("  Cargo.lock lags behind: the next cargo build (or `cargo update --workspace`) refreshes it")
    if report.in_sync:
        lines.append("Everything is in sync.")
    else:
        lines.append("OUT OF SYNC:")
        lines += [f"  - {p}" for p in report.problems]
        lines.append("Run `scripts/version.py <version>` to set them all.")
    return "\n".join(lines)


def main(argv: list[str]) -> int:
    root = Path(__file__).resolve().parent.parent
    if len(argv) == 0:
        report = read_all(root)
        print(describe(report))
        return 0 if report.in_sync else 1
    if argv == ["--short"]:
        report = read_all(root)
        if not report.in_sync:
            print(describe(report), file=sys.stderr)
            return 1
        print(report.version)
        return 0
    if len(argv) == 1 and not argv[0].startswith("-"):
        before = read_all(root).version
        try:
            report = set_version(root, argv[0])
        except (ValueError, RuntimeError) as error:
            print(f"error: {error}", file=sys.stderr)
            return 2
        print(f"{before} -> {report.version}\n")
        print(describe(report))
        return 0 if report.in_sync else 1
    print(__doc__, file=sys.stderr)
    return 2


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:]))
