#!/usr/bin/env python3
"""Gate native production code against hand-spelled migrated action namespaces."""

from __future__ import annotations

import re
import shutil
import sys
import tempfile
from contextlib import redirect_stderr, redirect_stdout
from io import StringIO
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
REGISTRY_REL = Path("crates/nmp-codegen/src/action_builders/registry.rs")
SCAN_ROOT_RELS = [
    Path("apps/chirp/ios/Chirp"),
    Path("apps/chirp/android/app/src/main/java/org/nmp/android"),
]


def fail(message: str) -> None:
    print(f"native-action-boundary: FAIL - {message}", file=sys.stderr)
    raise SystemExit(1)


def migrated_namespaces(root: Path) -> set[str]:
    registry = root / REGISTRY_REL
    if not registry.is_file():
        fail(f"action-builder registry not found: {REGISTRY_REL}")
    # The registry is a Rust module: the `ACTION_BUILDERS` table (and its
    # `namespace:` literals) may live in `registry.rs` itself or in any
    # `registry/<submodule>.rs` it declares via `mod ...;`. Read the whole
    # module so the gate follows the SSOT wherever a file-size split moves it.
    sources = [registry]
    registry_dir = registry.with_suffix("")
    if registry_dir.is_dir():
        sources.extend(sorted(registry_dir.rglob("*.rs")))
    text = "\n".join(src.read_text() for src in sources)
    namespaces = set(re.findall(r'namespace:\s*"([^"]+)"', text))
    publish = re.search(r'PUBLISH_NAMESPACE:\s*&str\s*=\s*"([^"]+)"', text)
    if publish:
        namespaces.add(publish.group(1))
    if not namespaces:
        fail("no migrated action namespaces discovered from action-builder registry")
    return namespaces


def source_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for rel in SCAN_ROOT_RELS:
        scan_root = root / rel
        if not scan_root.is_dir():
            fail(f"native source root not found: {rel}")
        files.extend(scan_root.rglob("*.swift"))
        files.extend(scan_root.rglob("*.kt"))
    return [path for path in sorted(files) if is_production_host_source(path)]


def is_production_host_source(path: Path) -> bool:
    parts = path.parts
    name = path.name
    if "Generated" in parts or name.endswith(".generated.swift") or name.endswith(".generated.kt"):
        return False
    if name in {"ActionBuilders.kt", "ActionBuilders.generated.swift"}:
        return False
    if "Tests" in parts or "androidTest" in parts or "test" in parts:
        return False
    return True


def host_string_literals(text: str) -> list[str]:
    literals: list[str] = []
    i = 0
    while i < len(text):
        if text.startswith("//", i):
            newline = text.find("\n", i + 2)
            i = len(text) if newline == -1 else newline + 1
            continue
        if text.startswith("/*", i):
            end = text.find("*/", i + 2)
            i = len(text) if end == -1 else end + 2
            continue
        if text.startswith('"""', i):
            end = text.find('"""', i + 3)
            if end == -1:
                break
            literals.append(text[i + 3 : end])
            i = end + 3
            continue
        if text[i] == '"':
            i += 1
            start = i
            escaped = False
            while i < len(text):
                ch = text[i]
                if escaped:
                    escaped = False
                elif ch == "\\":
                    escaped = True
                elif ch == '"':
                    literals.append(text[start:i])
                    i += 1
                    break
                i += 1
            continue
        i += 1
    return literals


def literal_occurrences(text: str, namespace: str) -> bool:
    pattern = re.compile(rf'(?<![A-Za-z0-9_.-]){re.escape(namespace)}(?![A-Za-z0-9_.-])')
    return any(pattern.search(literal) for literal in host_string_literals(text))


def check(root: Path) -> int:
    namespaces = migrated_namespaces(root)
    findings: list[tuple[str, str]] = []
    for path in source_files(root):
        text = path.read_text()
        for namespace in sorted(namespaces):
            if literal_occurrences(text, namespace):
                findings.append((str(path.relative_to(root)), namespace))
    if findings:
        print(
            "Native write-boundary drift - migrated action namespace spelled outside generated builders:",
            file=sys.stderr,
        )
        for rel, namespace in findings:
            print(f"  - {rel}: {namespace}", file=sys.stderr)
        print(
            "Use GeneratedActionBuilders or a Rust-authored intent seam; do not hand-assemble migrated action namespaces.",
            file=sys.stderr,
        )
        return 1
    print(
        f"native-action-boundary: OK - {len(namespaces)} migrated action namespace(s) hidden behind generated builders."
    )
    return 0


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def self_test() -> None:
    tmp = Path(tempfile.mkdtemp(prefix="native-action-boundary-"))
    try:
        write(
            tmp / REGISTRY_REL,
            """\
pub const ACTION_BUILDERS: &[ActionBuilder] = &[
    ActionBuilder { namespace: "nmp.follow", method: "follow", payload_file_identifier: "NF2A", payload_schema_version: 1, fields: &[], doc: "" },
];
pub const PUBLISH_NAMESPACE: &str = "nmp.publish";
""",
        )
        write(tmp / "apps/chirp/ios/Chirp/Bridge/Generated/ActionBuilders.generated.swift", '"nmp.follow"\n')
        write(tmp / "apps/chirp/android/app/src/main/java/org/nmp/android/ActionBuilders.kt", '"nmp.publish"\n')
        write(tmp / "apps/chirp/ios/Chirp/Bridge/KernelBridge.swift", "GeneratedActionBuilders.follow(...)\n")
        write(tmp / "apps/chirp/android/app/src/main/java/org/nmp/android/SocialActions.kt", "GeneratedActionBuilders.follow()\n")
        write(tmp / "apps/chirp/ios/Chirp/Features/Notes.swift", '/// "nmp.follow" in docs is not executable.\n')
        if check(tmp) != 0:
            fail("self-test valid fixture unexpectedly failed")
        print("native-action-boundary: self-test OK - generated files and comments may spell namespaces")

        offender = tmp / "apps/chirp/ios/Chirp/Features/Bad.swift"
        write(offender, 'let namespace = "nmp.follow"\n')
        stdout = StringIO()
        stderr = StringIO()
        with redirect_stdout(stdout), redirect_stderr(stderr):
            code = check(tmp)
        if code == 0:
            fail("self-test hand-spelled namespace did not trip")
        print("native-action-boundary: self-test OK - hand-spelled migrated namespace trips")
    finally:
        shutil.rmtree(tmp)


def main() -> None:
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    if mode == "--self-test":
        self_test()
        return
    if mode != "--check":
        fail(f"unknown mode `{mode}` (--check|--self-test)")
    raise SystemExit(check(REPO_ROOT))


if __name__ == "__main__":
    main()
