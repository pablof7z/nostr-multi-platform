#!/usr/bin/env python3
"""Verify nmp-gallery registry copies match the canonical component registry."""

from __future__ import annotations

import argparse
import difflib
import pathlib
import re
import sys
import tomllib


REPO_ROOT = pathlib.Path(__file__).resolve().parents[1]
REGISTRY_ROOT = REPO_ROOT / "crates/nmp-cli/registry"
IOS_GALLERY_ROOT = REPO_ROOT / "apps/nmp-gallery/ios/NmpGallery/Registry"
ANDROID_GALLERY_ROOT = (
    REPO_ROOT / "apps/nmp-gallery/android/app/src/main/kotlin/org/nmp/gallery/registry"
)
ANDROID_GALLERY_PACKAGE = "org.nmp.gallery.registry"
PLATFORMS = {
    "swiftui": ("registry.swiftui.toml", IOS_GALLERY_ROOT),
    "compose": ("registry.compose.toml", ANDROID_GALLERY_ROOT),
}
PACKAGE_RE = re.compile(r"^package\s+[\w.]+$", re.MULTILINE)
GALLERY_LOCAL_IMPORT_RE = re.compile(r"^import (?:nmp\.content|org\.nmp\.registry)\.[\w.*]+\n", re.MULTILINE)


def registry_sources(platform: str) -> list[pathlib.Path]:
    manifest_name, _ = PLATFORMS[platform]
    manifest = tomllib.loads((REGISTRY_ROOT / manifest_name).read_text())
    sources: list[pathlib.Path] = []
    for component in manifest.get("components", []):
        for file_entry in component.get("files", []):
            if file_entry.get("role") != "source":
                continue
            source = pathlib.Path(file_entry["source"])
            if source.parts and source.parts[0] == platform:
                sources.append(source)
    return sources


def canonical_for_gallery(platform: str, source: pathlib.Path) -> str:
    content = (REGISTRY_ROOT / source).read_text()
    if platform != "compose":
        return content
    replaced, count = PACKAGE_RE.subn(f"package {ANDROID_GALLERY_PACKAGE}", content, count=1)
    if count != 1:
        raise ValueError(f"{source}: expected one Kotlin package declaration")
    return GALLERY_LOCAL_IMPORT_RE.sub("", replaced)


def gallery_path(platform: str, source: pathlib.Path) -> pathlib.Path:
    _, gallery_root = PLATFORMS[platform]
    return gallery_root / source.name


def write_if_needed(path: pathlib.Path, content: str) -> bool:
    if path.exists() and path.read_text() == content:
        return False
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(content)
    return True


def diff(expected: str, actual: str, expected_path: pathlib.Path, actual_path: pathlib.Path) -> str:
    return "".join(
        difflib.unified_diff(
            expected.splitlines(keepends=True),
            actual.splitlines(keepends=True),
            fromfile=str(expected_path.relative_to(REPO_ROOT)),
            tofile=str(actual_path.relative_to(REPO_ROOT)),
        )
    )


def main() -> int:
    parser = argparse.ArgumentParser()
    parser.add_argument(
        "--fix",
        action="store_true",
        help="rewrite gallery registry copies from the canonical registry",
    )
    args = parser.parse_args()

    failures: list[str] = []
    changed: list[pathlib.Path] = []
    checked = 0
    for platform in PLATFORMS:
        for source in registry_sources(platform):
            expected = canonical_for_gallery(platform, source)
            target = gallery_path(platform, source)
            checked += 1
            if args.fix:
                if write_if_needed(target, expected):
                    changed.append(target.relative_to(REPO_ROOT))
                continue
            if not target.exists():
                failures.append(f"missing gallery registry copy: {target.relative_to(REPO_ROOT)}")
                continue
            actual = target.read_text()
            if actual != expected:
                failures.append(diff(expected, actual, REGISTRY_ROOT / source, target))

    if args.fix:
        for path in changed:
            print(f"gallery-registry-drift: wrote {path}")
        print(f"gallery-registry-drift: synced {checked} registry source copy/copies")
        return 0

    if failures:
        print(
            "gallery-registry-drift: nmp-gallery registry copies drifted from "
            "crates/nmp-cli/registry. Run `python3 ci/check-gallery-registry-drift.py --fix`.",
            file=sys.stderr,
        )
        print("\n".join(failures), file=sys.stderr)
        return 1

    print(f"gallery-registry-drift: OK ({checked} registry source copy/copies)")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
