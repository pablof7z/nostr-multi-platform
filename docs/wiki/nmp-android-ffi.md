---
title: NMP Android FFI — Crate, Linkage, and Kernel Integration
slug: nmp-android-ffi
summary: nmp-android-ffi is a standalone cdylib workspace crate that depends on nmp-core via Rust-path imports (not extern C declarations)
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-28
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:e2d58641-a6c3-4f43-94c0-b018c8fbb893
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:c8c2902c-43a6-4b1c-8215-1732dc266895
  - session:200932fb-5a92-44e0-8d42-2184d2e69094
  - session:54fc9b94-b995-46c6-8372-59c4abe0f95a
  - session:f26050da-6d8a-4128-9179-4088a9df94b9
  - session:56d215c4-1aee-47cc-95c2-fd17269b92b6
  - session:d98be997-81df-4738-8846-8323d40ab9ff
---

# NMP Android FFI — Crate, Linkage, and Kernel Integration

## Overview

nmp-android-ffi is a standalone cdylib workspace crate that depends on nmp-core via Rust-path imports (not extern C declarations). cargo-ndk 4.1.2 is used for cross-compiling the Rust FFI library to aarch64-linux-android and x86_64-linux-android targets.

The `nmp-app-gallery` Rust crate provides JNI wrapper symbols behind an `android-ffi` feature flag, mirroring the pattern used by `nmp-android-ffi` for the Chirp app.

The `KernelBridge.kt` exposes an `openAuthor(pubkey: String)` method backed by a `nativeOpenAuthor` JNI call.

UniFFI is M14 PLANNED — raw C FFI is the current live production surface.

<!-- citations: [^e2d58-9] [^c8c29-1] [^c8c29-2] [^56d21-3] -->
## Rust-path FFI over extern C

nmp-android-ffi must call kernel functions through Rust paths (nmp_core::nmp_app_new etc.) rather than extern C declarations, because extern C is opaque to Rust's CGU compilation and leaves symbols undefined in the .so. nmp-core has an android-ffi feature that re-exports all FFI symbols via pub use at the crate root.

The C-ABI nmp_app_set_update_callback signature uses (*context, *const u8, usize) rather than (*context, *const c_char). The NmpUpdateCallback FFI ABI signature is `typedef void (*NmpUpdateCallback)(void *context, const uint8_t *bytes, uintptr_t len)`, passing borrowed FlatBuffers bytes that are valid only for the callback duration.

CI enforces that the Rust UpdateCallback type must be `extern "C" fn(*mut c_void, *const u8, usize)` and that nmp_app_set_update_callback's signature matches the expected declaration across all three FFI headers (Chirp NmpCore.h, Notes NmpCore.h, NmpGallery NmpGallery.h).

<!-- citations: [^e2d58-10] [^20093-15] [^54fc9-10] -->
## Android PIC cdylib linking constraints

The #[used] KernelSymbolTable struct trick does not work for Android PIC cdylib builds because function pointers in DATA sections create GOT dynamic relocations, not static archive-pulling relocations. The -Wl,-u linker flags alone cannot resolve undefined kernel symbols because nmp-core's rlib is never passed to lld as a linker input archive. [^e2d58-11]

## FFI module re-export structure

The nmp-core ffi module's identity, timeline, and wallet sub-modules are private and require intermediate pub use re-exports in ffi/mod.rs before crate-level re-export. [^e2d58-12]


iOS hard-coded timeline window limits of 80 (default) and 500 (max) are exposed as #define constants (NMP_CHIRP_DEFAULT_WINDOW_LIMIT and NMP_CHIRP_MAX_WINDOW_LIMIT) in NmpCore.h to prevent drift from Rust. [^20093-17]
## Git tracking and artifact safety

The `nmp-android-ffi/` directory and `android/` Kotlin app must be tracked in git or deleted to prevent accidental loss via `git clean -fd`. [^57528-18]

## Documentation staleness

`ffi-surface.md` is stale with 5+ undocumented production symbols, and `NmpCore.h` is hand-maintained and likely missing entries. The NmpCore.h FFI header must be regenerated whenever Rust FFI exports change; the ffi-header-drift CI gate enforces this. The FFI header drift CI check must be confirmed to catch signature changes, not just new symbol names. ci/check-ffi-header-drift.sh must not reference the deleted path apps/notes/ios/Notes/Bridge/NmpCore.h.

<!-- citations: [^57528-19] [^20093-16] [^f2605-16] [^d98be-5] -->
## Safety hazards

`nmp_app_free` has a double-free use-after-free risk with no runtime guard. [^57528-20]
## See Also

