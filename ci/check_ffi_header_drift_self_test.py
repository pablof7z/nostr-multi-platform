"""Self-tests for the FFI header drift gate."""

from __future__ import annotations

import shutil
import tempfile
from contextlib import redirect_stderr
from io import StringIO
from pathlib import Path

from check_ffi_header_drift import HEADER_REL, UPDATE_CALLBACK_HEADER_RELS, compare, fail


def write(path: Path, text: str) -> None:
    path.parent.mkdir(parents=True, exist_ok=True)
    path.write_text(text)


def expect_ok(root: Path, label: str) -> None:
    if compare(root):
        fail(f"self-test `{label}` unexpectedly failed")
    print(f"ffi-header-drift: self-test OK - {label}")


def expect_trip(root: Path, label: str) -> None:
    stderr = StringIO()
    with redirect_stderr(stderr):
        code = compare(root)
    if code == 0:
        fail(f"self-test `{label}` did not trip")
    print(f"ffi-header-drift: self-test OK - {label} trips")


def self_test() -> None:
    tmp = Path(tempfile.mkdtemp(prefix="ffi-header-drift-"))
    try:
        header_rel = tmp / HEADER_REL
        gallery_rel = tmp / UPDATE_CALLBACK_HEADER_RELS[1]
        base_header = """\
#include <stdbool.h>
#include <stdint.h>
typedef void (*NmpUpdateCallback)(void *context, const uint8_t *bytes, uintptr_t len);
void *nmp_app_new(void);
void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback callback);
void nmp_extra_ping(void);
void nmp_free_string(char *ptr);
// void nmp_app_comment_only(void);
"""
        write(header_rel, base_header)
        write(
            gallery_rel,
            """\
#include <stdint.h>
typedef void (*NmpUpdateCallback)(void *context, const uint8_t *bytes, uintptr_t len);
void nmp_app_set_update_callback(void *app, void *context, NmpUpdateCallback callback);
""",
        )
        write(
            tmp / "crates/nmp-ffi/src/lib.rs",
            """\
use std::ffi::{c_char, c_void};
pub struct NmpApp;
type UpdateCallback = extern "C" fn(*mut c_void, *const u8, usize);
#[no_mangle]
pub extern "C" fn nmp_app_new() -> *mut NmpApp { todo!() }
#[no_mangle]
pub extern "C" fn nmp_app_set_update_callback(
    app: *mut NmpApp,
    context: *mut c_void,
    callback: Option<UpdateCallback>,
) {}
#[no_mangle]
pub extern "C" fn nmp_extra_ping() {}
#[no_mangle]
pub extern "C" fn nmp_free_string(ptr: *mut c_char) {}
""",
        )
        write(tmp / "apps/chirp/nmp-app-chirp/src/ffi/mod.rs", "")
        write(tmp / "crates/nmp-marmot/src/lib.rs", "")
        write(
            tmp / "crates/nmp-ffi/src/testing.rs",
            """\
#![cfg(any(test, feature = "test-support"))]
#[no_mangle]
pub extern "C" fn nmp_app_test_only_not_in_header() {}
""",
        )

        expect_ok(tmp, "valid fixture")

        new_surface = tmp / "apps/chirp/nmp-app-chirp/src/ffi/new_surface.rs"
        write(
            new_surface,
            """\
#[no_mangle]
pub extern "C" fn nmp_app_chirp_new_surface() {}
""",
        )
        expect_trip(tmp, "auto-discovered production file missing from header")
        new_surface.unlink()

        write(header_rel, base_header + "void nmp_app_missing(void);\n")
        expect_trip(tmp, "extra header declaration")

        write(
            header_rel,
            base_header.replace(
                "void nmp_extra_ping(void);",
                "void nmp_extra_ping(uint32_t unexpected);",
            ),
        )
        expect_trip(tmp, "same-name signature drift")

        print("ffi-header-drift: self-test OK")
    finally:
        shutil.rmtree(tmp)
