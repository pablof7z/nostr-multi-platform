#!/usr/bin/env python3
"""Check C header declarations against production Rust FFI exports."""

from __future__ import annotations

import re
import sys
from dataclasses import dataclass
from pathlib import Path


SCRIPT_DIR = Path(__file__).resolve().parent
REPO_ROOT = SCRIPT_DIR.parent
HEADER_REL = Path("apps/chirp/ios/Chirp/Bridge/NmpCore.h")
UPDATE_CALLBACK_HEADER_RELS = [
    HEADER_REL,
    Path("apps/nmp-gallery/ios/NmpGallery/Bridge/NmpGallery.h"),
]
FFI_ROOT_RELS = [
    Path("crates/nmp-ffi/src"),
    Path("apps/chirp/crates/nmp-app-chirp/src/ffi"),
    Path("crates/nmp-marmot/src"),
]


@dataclass(frozen=True)
class Signature:
    ret: str
    params: tuple[str, ...]


@dataclass(frozen=True)
class RustExport:
    signature: Signature
    rel_path: str


def fail(message: str) -> None:
    print(f"ffi-header-drift: FAIL - {message}", file=sys.stderr)
    raise SystemExit(1)


def strip_c_comments(text: str) -> str:
    text = re.sub(r"/\*.*?\*/", "", text, flags=re.S)
    return re.sub(r"//.*", "", text)


def first_code_line(path: Path) -> str:
    for line in path.read_text().splitlines():
        stripped = line.strip()
        if stripped and not stripped.startswith("//"):
            return stripped
    return ""


def is_test_only_file(path: Path, root: Path) -> bool:
    rel_parts = path.relative_to(root).parts
    name = path.name
    if "tests" in rel_parts or name == "tests.rs" or name.endswith("_tests.rs"):
        return True
    line = first_code_line(path)
    return line.startswith("#![cfg(") and ("test" in line or "test-support" in line)


def discover_rust_files(root: Path) -> list[Path]:
    files: list[Path] = []
    for rel in FFI_ROOT_RELS:
        source_root = root / rel
        if not source_root.is_dir():
            fail(f"FFI source root not found: {rel}")
        files.extend(sorted(source_root.rglob("*.rs")))
    return [path for path in files if not is_test_only_file(path, root)]


def split_top_level(value: str, sep: str = ",") -> list[str]:
    parts: list[str] = []
    current: list[str] = []
    angle = paren = bracket = 0
    for ch in value:
        if ch == "<":
            angle += 1
        elif ch == ">":
            angle = max(0, angle - 1)
        elif ch == "(":
            paren += 1
        elif ch == ")":
            paren = max(0, paren - 1)
        elif ch == "[":
            bracket += 1
        elif ch == "]":
            bracket = max(0, bracket - 1)
        if ch == sep and angle == paren == bracket == 0:
            parts.append("".join(current).strip())
            current = []
        else:
            current.append(ch)
    tail = "".join(current).strip()
    if tail:
        parts.append(tail)
    return parts


def normalize_spaces(value: str) -> str:
    value = re.sub(r"\s+", " ", value.strip())
    value = re.sub(r"\s*\*\s*", " *", value)
    value = re.sub(r"\s+", " ", value.strip())
    return value


def normalize_c_type(value: str, *, return_type: bool = False) -> str:
    value = normalize_spaces(value)
    if not return_type:
        value = re.sub(r"\s*\*[A-Za-z_][A-Za-z0-9_]*$", " *", value)
        value = re.sub(r"\s+[A-Za-z_][A-Za-z0-9_]*$", "", value)
        value = normalize_spaces(value)
    aliases = {
        "void": "void",
        "void *": "void *",
        "void **": "void **",
        "void * *": "void **",
        "const void *": "const void *",
        "char *": "char *",
        "const char *": "const char *",
        "const char *const *": "const char *const *",
        "const uint8_t *": "const uint8_t *",
        "uint8_t *": "uint8_t *",
        "uintptr_t": "uintptr_t",
        "uint64_t": "uint64_t",
        "uint32_t": "uint32_t",
        "uint8_t": "uint8_t",
        "int": "int",
        "bool": "bool",
        "unsigned int": "unsigned int",
        "struct NmpMirrorBytes": "struct NmpMirrorBytes",
        "NmpUpdateCallback": "NmpUpdateCallback",
        "NmpEventObserverCallback": "NmpEventObserverCallback",
        "NmpLifecycleCallback": "NmpLifecycleCallback",
        "NmpCapabilityCallback": "NmpCapabilityCallback",
        "NmpActionResultObserver": "NmpActionResultObserver",
    }
    if value in aliases:
        return aliases[value]
    fail(f"unsupported C FFI type in NmpCore.h: `{value}`")


def header_signatures(root: Path) -> dict[str, Signature]:
    header = root / HEADER_REL
    if not header.is_file():
        fail(f"header not found: {HEADER_REL}")
    text = strip_c_comments(header.read_text())
    text = "\n".join(line for line in text.splitlines() if not line.lstrip().startswith("#"))
    signatures: dict[str, Signature] = {}
    for stmt in text.split(";"):
        stmt = normalize_spaces(stmt)
        if "nmp_" not in stmt or "(*" in stmt:
            continue
        match = re.search(r"(?P<ret>.+?)\s*(?P<name>nmp_[A-Za-z0-9_]+)\s*\((?P<params>.*)\)$", stmt)
        if not match:
            continue
        name = match.group("name")
        ret = normalize_c_type(match.group("ret"), return_type=True)
        raw_params = match.group("params").strip()
        params = () if raw_params in ("", "void") else tuple(
            normalize_c_type(param) for param in split_top_level(raw_params)
        )
        if name in signatures:
            fail(f"duplicate C header declaration for {name}")
        signatures[name] = Signature(ret, params)
    return signatures


def matching_paren(text: str, open_index: int) -> int:
    depth = 0
    for index in range(open_index, len(text)):
        ch = text[index]
        if ch == "(":
            depth += 1
        elif ch == ")":
            depth -= 1
            if depth == 0:
                return index
    fail("unterminated Rust extern signature")


def rust_return_type(text: str, close_index: int) -> str:
    tail = text[close_index + 1 :]
    match = re.match(r"\s*->\s*([^{\n]+)", tail)
    return match.group(1).strip() if match else "void"


def normalize_rust_type(value: str) -> str:
    value = normalize_spaces(value)
    value = value.replace("std::ffi::", "")
    value = value.replace("std::os::raw::", "")
    value = value.replace("crate::", "")
    value = re.sub(r"\s+", " ", value)
    pointer_aliases = {
        "*mut c_void": "void *",
        "*const c_void": "const void *",
        "*mut c_char": "char *",
        "*const c_char": "const char *",
        "*const *const c_char": "const char *const *",
        "*const u8": "const uint8_t *",
        "*mut u8": "uint8_t *",
        "*mut *mut ChirpHandle": "void **",
        "*mut NmpApp": "void *",
        "*const NmpApp": "const void *",
        "*mut ChirpHandle": "void *",
        "*mut GroupFeedHandle": "void *",
        "*mut MarmotHandle": "void *",
    }
    aliases = {
        "void": "void",
        "()": "void",
        "bool": "bool",
        "u8": "uint8_t",
        "u32": "uint32_t",
        "u64": "uint64_t",
        "i32": "int",
        "usize": "uintptr_t",
        "c_int": "int",
        "c_uint": "unsigned int",
        "NmpMirrorBytes": "struct NmpMirrorBytes",
        "Option<UpdateCallback>": "NmpUpdateCallback",
        "Option<KernelEventObserverFn>": "NmpEventObserverCallback",
        "Option<LifecycleObserverFn>": "NmpLifecycleCallback",
        "Option<CapabilityCallback>": "NmpCapabilityCallback",
        "Option<NmpActionResultObserver>": "NmpActionResultObserver",
    }
    if value in pointer_aliases:
        return pointer_aliases[value]
    if value in aliases:
        return aliases[value]
    fail(f"unsupported Rust FFI type: `{value}`")


RUST_EXPORT_RE = re.compile(
    r"#\[no_mangle\](?:\s*#\[[^\]]+\])*\s*pub\s+extern\s+\"C\"\s+fn\s+"
    r"(?P<name>nmp_[A-Za-z0-9_]+)\s*\(",
    flags=re.S,
)


def rust_signatures(root: Path) -> dict[str, RustExport]:
    exports: dict[str, RustExport] = {}
    for path in discover_rust_files(root):
        text = path.read_text()
        for match in RUST_EXPORT_RE.finditer(text):
            name = match.group("name")
            open_index = match.end() - 1
            close_index = matching_paren(text, open_index)
            raw_params = text[open_index + 1 : close_index].strip()
            params: list[str] = []
            if raw_params:
                for raw in split_top_level(raw_params):
                    if ":" not in raw:
                        fail(f"could not parse Rust parameter `{raw}` in {path.relative_to(root)}")
                    params.append(normalize_rust_type(raw.split(":", 1)[1].strip()))
            ret = normalize_rust_type(rust_return_type(text, close_index))
            rel = str(path.relative_to(root))
            if name in exports:
                fail(f"duplicate Rust export for {name}: {exports[name].rel_path} and {rel}")
            exports[name] = RustExport(Signature(ret, tuple(params)), rel)
    return exports


def check_aux_update_headers(root: Path) -> None:
    expected_type = "typedef void (*NmpUpdateCallback)(void *context, const uint8_t *bytes, uintptr_t len);"
    expected_fn = "void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback callback);"
    for rel in UPDATE_CALLBACK_HEADER_RELS:
        path = root / rel
        if not path.is_file():
            fail(f"callback header not found: {rel}")
        lines = {line.strip() for line in path.read_text().splitlines()}
        if expected_type not in lines:
            fail(f"{rel} has stale NmpUpdateCallback typedef")
        if expected_fn not in lines:
            fail(f"{rel} has stale nmp_app_set_update_callback declaration")


def compare(root: Path) -> int:
    header = header_signatures(root)
    rust = rust_signatures(root)
    check_aux_update_headers(root)

    rust_only = sorted(set(rust) - set(header))
    header_only = sorted(set(header) - set(rust))
    mismatched = sorted(name for name in set(rust) & set(header) if rust[name].signature != header[name])

    if rust_only:
        print("FFI DRIFT - production Rust symbols missing from NmpCore.h:", file=sys.stderr)
        for name in rust_only:
            print(f"  - {name}    (defined in {rust[name].rel_path})", file=sys.stderr)
    if header_only:
        print("FFI DRIFT - NmpCore.h declarations not exported from Rust:", file=sys.stderr)
        for name in header_only:
            print(f"  - {name}", file=sys.stderr)
    if mismatched:
        print("FFI DRIFT - same-name Rust/header signature mismatch:", file=sys.stderr)
        for name in mismatched:
            print(f"  - {name}", file=sys.stderr)
            print(f"      Rust:   {format_sig(rust[name].signature)}", file=sys.stderr)
            print(f"      Header: {format_sig(header[name])}", file=sys.stderr)

    return 1 if rust_only or header_only or mismatched else 0


def format_sig(sig: Signature) -> str:
    return f"{sig.ret}({', '.join(sig.params) if sig.params else 'void'})"


def main() -> None:
    mode = sys.argv[1] if len(sys.argv) > 1 else "--check"
    if mode == "--self-test":
        from check_ffi_header_drift_self_test import self_test

        self_test()
        return
    if mode != "--check":
        fail(f"unknown mode `{mode}` (--check|--self-test)")
    code = compare(REPO_ROOT)
    if code:
        raise SystemExit(code)
    print(f"ffi-header-drift: OK - {len(rust_signatures(REPO_ROOT))} production nmp_* symbols in sync.")


if __name__ == "__main__":
    main()
