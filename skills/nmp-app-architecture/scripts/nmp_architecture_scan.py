#!/usr/bin/env python3
"""Static triage for common RMP/NMP architecture violations.

This script intentionally favors actionable suspicion over completeness. It
does not prove compliance; it points reviewers at places that need judgment.
"""

from __future__ import annotations

import argparse
import json
import re
import sys
from dataclasses import asdict, dataclass
from pathlib import Path
from typing import Iterable


EXCLUDED_DIRS = {
    ".git",
    ".gradle",
    ".idea",
    ".swiftpm",
    "build",
    "DerivedData",
    "docs",
    "examples",
    "fixtures",
    "node_modules",
    "target",
    "tests",
    "testdata",
    ".build",
    "dist",
    "vendor",
    "wiki",
}

TEXT_EXTENSIONS = {
    ".rs",
    ".swift",
    ".kt",
    ".kts",
    ".java",
    ".ts",
    ".tsx",
    ".js",
    ".jsx",
    ".mjs",
    ".c",
    ".cc",
    ".cpp",
    ".h",
    ".hpp",
}

NATIVE_EXTENSIONS = {".swift", ".kt", ".kts", ".java", ".ts", ".tsx", ".js", ".jsx", ".mjs"}


@dataclass
class Finding:
    severity: str
    rule: str
    path: str
    line: int
    match: str
    reason: str


def iter_files(root: Path) -> Iterable[Path]:
    for path in root.rglob("*"):
        if not path.is_file():
            continue
        if any(part in EXCLUDED_DIRS for part in path.parts):
            continue
        if path.suffix not in TEXT_EXTENSIONS:
            continue
        yield path


def should_skip_line(path: Path, line: str) -> bool:
    parts = set(path.parts)
    if {"docs", "wiki", "README.md", "CHANGELOG.md", "AGENTS.md", "CLAUDE.md"} & parts:
        return True
    if "test" in path.name.lower() or "tests" in parts:
        return True
    stripped = line.strip()
    if not stripped:
        return True
    if stripped.startswith("//") or stripped.startswith("#") or stripped.startswith("*"):
        return True
    if stripped.startswith("Text(") or stripped.startswith("Label("):
        return True
    return False


RULES = [
    (
        "error",
        "D8/no-polling",
        re.compile(
            r"\b(thread::sleep|Task\.sleep|Timer\.scheduledTimer|setInterval|setTimeout|"
            r"DispatchQueue\.[A-Za-z0-9_.]+\.asyncAfter|while\s+.*sleep|"
            r"try_recv\b.*sleep|sleep\b.*try_recv)"
        ),
        "Polling or sleep-check loops are forbidden; use blocking primitives or callbacks.",
        None,
    ),
    (
        "error",
        "D3/no-hardcoded-relay",
        re.compile(r"wss://[A-Za-z0-9._~:/?#\[\]@!$&'()*+,;=%-]+"),
        "Hardcoded relay URLs in production code usually bypass outbox routing.",
        None,
    ),
    (
        "warning",
        "D9/kernel-owns-time",
        re.compile(r"\b(SystemTime::now|Instant::now|Date\(\)|Date\.now|currentTimeMillis|NSDate\(\))\b"),
        "Reducer, replay, routing, or policy code must use an injected clock.",
        None,
    ),
    (
        "warning",
        "D6/no-ffi-errors",
        re.compile(r"(#\[uniffi::export\]|extern\s+\"C\"|@Throws|throws\b|try\s*\{|catch\s*\(|->\s*Result\s*<)"),
        "Errors must surface as state, not native exceptions or FFI Result types.",
        None,
    ),
    (
        "warning",
        "D5/bounded-snapshot",
        re.compile(r"\b(AppState|Snapshot|FullState)\b.*\b(Vec<.*Event|Vec<.*Note|event_store|history|all_events)\b", re.I),
        "Snapshots should be screen-shaped and bounded by open views, never event history.",
        None,
    ),
    (
        "warning",
        "D7/native-policy-smell",
        re.compile(
            r"\b(shouldRetry|isRecoverable|retryCount|relayUrl|relay_url|publishRelay|"
            r"decrypt|encrypt|signEvent|nostrEvent|NostrEvent|Filter|Kind|kind\s*[=:])\b"
        ),
        "Native code may be rendering or capability execution only; verify this is not policy.",
        NATIVE_EXTENSIONS,
    ),
    (
        "warning",
        "D4/native-cache-smell",
        re.compile(r"\b(cache|cached|RoomDatabase|SwiftData|UserDefaults|SharedPreferences)\b"),
        "Native caches must not mirror Rust-owned app facts.",
        NATIVE_EXTENSIONS,
    ),
    (
        "warning",
        "no-debt",
        re.compile(r"(\bTODO\b|\bFIXME\b|\bHACK\b|\btemporary\b|\bfor now\b|\bstub\b|\bworkaround\b)"),
        "Temporary hacks, TODO debt, stubs, and workaround paths require canonical tracking or removal.",
        None,
    ),
]


def scan(root: Path) -> list[Finding]:
    findings: list[Finding] = []
    for path in iter_files(root):
        try:
            text = path.read_text(encoding="utf-8")
        except UnicodeDecodeError:
            continue
        rel = str(path.relative_to(root))
        lines = text.splitlines()
        skip_from = None
        if path.suffix == ".rs":
            for idx, raw in enumerate(lines, start=1):
                if "#[cfg(test)]" in raw:
                    skip_from = idx
                    break
        for line_no, line in enumerate(lines, start=1):
            if skip_from is not None and line_no >= skip_from:
                continue
            if should_skip_line(path, line):
                continue
            for severity, rule, pattern, reason, ext_filter in RULES:
                if ext_filter is not None and path.suffix not in ext_filter:
                    continue
                if rule == "D6/no-ffi-errors" and path.suffix == ".rs":
                    window = "\n".join(lines[max(0, line_no - 6) : line_no + 1])
                    ffi_context = (
                        "uniffi::export" in window
                        or 'extern "C"' in window
                        or "ffi" in rel.lower()
                    )
                    if not ffi_context:
                        continue
                match = pattern.search(line)
                if not match:
                    continue
                if rule == "D3/no-hardcoded-relay" and (
                    "assert" in line or "relay.example" in line or "wss://x" in line
                ):
                    continue
                findings.append(
                    Finding(
                        severity=severity,
                        rule=rule,
                        path=rel,
                        line=line_no,
                        match=line.strip()[:180],
                        reason=reason,
                    )
                )
    return findings


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("root", nargs="?", default=".", help="Repository or app root to scan")
    parser.add_argument("--json", action="store_true", help="Emit JSON")
    parser.add_argument("--limit", type=int, default=200, help="Maximum findings to print, 0 for all")
    parser.add_argument(
        "--fail-on",
        choices=["never", "warning", "error"],
        default="never",
        help="Exit nonzero when findings at or above this severity exist",
    )
    args = parser.parse_args()

    root = Path(args.root).resolve()
    findings = scan(root)

    visible = findings if args.limit == 0 else findings[: args.limit]

    if args.json:
        print(json.dumps([asdict(f) for f in visible], indent=2))
    else:
        if not findings:
            print("nmp-architecture-scan: no findings")
        for f in visible:
            print(f"{f.severity.upper()} {f.rule} {f.path}:{f.line}")
            print(f"  {f.match}")
            print(f"  {f.reason}")
        if len(visible) < len(findings):
            print(f"... {len(findings) - len(visible)} more finding(s); rerun with --limit 0 to show all")

    severities = {f.severity for f in findings}
    if args.fail_on == "error" and "error" in severities:
        return 2
    if args.fail_on == "warning" and findings:
        return 2
    return 0


if __name__ == "__main__":
    sys.exit(main())
