---
type: episode-card
date: 2026-05-25
session: c8c2902c-43a6-4b1c-8215-1732dc266895
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c8c2902c-43a6-4b1c-8215-1732dc266895.jsonl
salience: architecture
status: active
subjects:
  - nmp-app-gallery
  - android-jni-bridge
  - kernel-ffi
supersedes: []
related_claims: []
source_lines:
  - 510-797
captured_at: 2026-06-18T05:33:14Z
---

# Episode: Android gallery gets its own JNI shim instead of nmp-android-ffi

## Prior State

nmp-app-gallery crate exported only C-ABI nmp_app_* symbols; the Android KernelBridge.kt expected JNI-style Java_org_nmp_gallery_bridge_KernelBridge_nativeNew symbols, so the gallery app could not load the Rust kernel on Android at all (UnsatisfiedLinkError)

## Trigger

Running the nmp-gallery Android app produced UnsatisfiedLinkError: no JNI entry points existed in the .so

## Decision

Added a dedicated android.rs JNI shim module to nmp-app-gallery with 11 JNI entry points (nativeNew, nativeFree, nativeGalleryRegister, nativeOpenAuthor, nativeStart, nativeStop, nativeClaimProfile, nativeReleaseProfile, nativeNextUpdate, nativeGallerySnapshot, nativeDispatchAction), gated behind the android-ffi feature flag with jni = '0.21' optional dependency. The gallery app no longer depends on nmp-android-ffi.

## Consequences

- nmp-app-gallery now produces a self-contained libnmp_app_gallery.so with JNI symbols, mirroring how nmp-app-chirp uses nmp-android-ffi but as an inline module
- Cargo.toml added jni dep and expanded android-ffi feature to include dep:jni
- The gallery app can boot, load the Rust kernel, and drain snapshot updates on Android
- Bootstrap relay list (purplepag.es, relay.damus.io, nos.lol) is hardcoded in the JNI shim

## Open Tail

*(none)*

## Evidence

- transcript lines 510-797

