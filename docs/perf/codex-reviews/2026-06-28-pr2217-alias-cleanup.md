# Codex Review — PR #2217: compat alias + dead code sweep, nmp-kinds migration

**PR:** https://github.com/pablof7z/nostr-multi-platform/pull/2217  
**Date:** 2026-06-28  
**Scope:** #2198 (compat-alias + dead-code sweep) + #2200 (finish nmp-kinds migration)

---

## What changed

45 files, −549 lines / +224 lines. Net −325 LOC across aliases, dead code, and migrated kind-integer consts.

### Aliases deleted (hard-break, all callers updated)

| Old name | New name | Location |
|---|---|---|
| `DmInboxLookup` | `DmInboxRelayLookup` | nmp-core substrate |
| `OkFrame` | `AuthOk` | nmp-core kernel/auth.rs |
| `FeedCompileOutput` | `FeedSessionBuild` | nmp-ffi + test files |
| `FeedParamsError` | `PrimaryKindError` | nmp-ffi, nmp-wasm |
| `ObservedProjectionHandle` | `ObservedProjectionCommandHandle` | nmp-ffi, nmp-defaults |
| `KIND_COMMENT` | `KIND_NIP22_COMMENT` | nmp-nip22, nmp-relations, nmp-defaults |
| `KIND_LONG_FORM` | `KIND_LONG_FORM_ARTICLE` | nmp-nip50 |
| `ImageDimensions`/`ImageMeta` | `MediaDimensions`/`MediaMeta` | nmp-nip68, nmp-testing |
| nmp-nip60 six kind aliases | `KIND_NIP60_*`/`KIND_NIP61_*` | nmp-nip60/kinds.rs |

### Dead code deleted

- `KIND_MARMOT_KEY_PACKAGE_LEGACY = 443` (nmp-kinds)
- `DmInboxLookup` re-export block (substrate/protocol/capabilities.rs)
- `FeedParamsError` type alias (nmp-ffi/feed.rs)
- `ObservedProjectionHandle` type alias (nmp-ffi/observed_projection_handle.rs)
- `FeedCompileOutput` type alias (nmp-ffi/feed_session.rs)
- `shape_to_store_queries` gated to `#[cfg(test)]` (nmp-core cache_serve)
- `is_relay_url` private function with no callers (nmp-router)
- `UpdatePushListener` + `UpdateListenerSlot` (nmp-chirp-android-ffi — always None after M14-0)
- `nmp_app_dispatch_action` JSON shim re-export from nmp-ffi/lib.rs
- Marmot dispatch tests migrated to byte doorway (last caller of JSON shim)

### nmp-kinds migration (#2200)

- Added `KIND_HIGHLIGHT = 9_802` (NIP-84)
- embed_projection/mod.rs: local kind consts → `nmp_kinds::` imports
- nmp-nip60/kinds.rs: full rewrite — re-exports from nmp-kinds directly

---

## Review findings

### Correct calls

1. **DmInboxRelayLookup routing** — Adding `DmInboxRelayLookup` to `capabilities.rs` as `pub use crate::substrate::DmInboxRelayLookup` is the right layering seam: capability traits live in `protocol/capabilities.rs`, and `DmInboxRelayLookup` is a protocol-facing capability trait. The `substrate/mod.rs` original re-export from `dm_inbox_relays.rs` remains the primary source; the capabilities.rs re-export is a view alias that keeps `protocol.rs` compilation clean without importing `dm_inbox_relays` directly.

2. **`finish_dispatch` restoration** — The previous session deleted `finish_dispatch` along with `nmp_app_dispatch_action` and the JSON shim. This session restored it as `pub(super)` so both `action.rs` (test path) and `action/bytes.rs` (production byte doorway) can reach it. The function is not dead — `bytes.rs` calls it on the production `nmp_app_dispatch_action_bytes` path. The restoration is a bug-fix, not a regression.

3. **`dispatch_action_json` + `execute_action` restoration** — Both restored behind `#[cfg(any(test, feature = "test-support"))]` only; the public `nmp_app_dispatch_action` (C-ABI JSON shim) stays deleted. This is the correct boundary: the internal test helper remains; the external export is gone.

4. **Marmot test migration** — `dispatch_action_tests.rs` now uses `encode_dispatch_envelope` + `nmp_app_dispatch_action_bytes`. The 32-char `correlation_id` is derived from the first half of a freshly-generated nostr pubkey hex — a reasonable test source that satisfies the non-empty correlation_id envelope validation (S2).

5. **UpdatePushListener deletion** — Correct. M14-0 (#2129) replaced the JNI push path with `AppHandle::set_update_sink`. The `push_listener` slot was never set after M14-0; all production code in `uniffi_app_loop.rs` uses the UniFFI path. Tests that verified the slot's lock-ordering properties are removed; equivalent concurrency tests for the surviving `generic_sink` path exist in `session/tests.rs`.

### Potential improvements (post-v1 / non-blocking)

1. **nmp-nip68 `parse_imeta_tag` doc** — After renaming `ImageMeta` → `MediaMeta`, any doc comments that mention "image" metadata might be outdated. Not a correctness issue; cosmetic.

2. **`capabilities.rs` `DmInboxRelayLookup` doc comment** — The added re-export comment says "NIP-17 kind:10050 DM-inbox relay reads — substrate-generic." The "kind:10050" detail is NIP-17–specific; `DmInboxRelayLookup` is a substrate-generic trait. A more neutral comment ("DM-inbox relay lookup — implemented by nmp-nip17::DmRelayCache") avoids leaking NIP numbers into the substrate layer's vocabulary.

3. **`dispatch_action_json` in action.rs** — The function body recomputes `dispatch_now_ms` via `SystemTime::now()` on every call. This is fine for test use. Production dispatches use the kernel clock via the byte doorway. No issue.

### Risks

None identified. CI gate covers the full call graph; doctrine-lint (0 findings) + 98-test doctrine smoke suite confirm D0/D6/D8 compliance. The `#[cfg(test)]`-gated `shape_to_store_queries` is in-scope only for tests that already exercised it — no behavioral change.

---

## Verdict

**Approve.** The PR does exactly what #2198/#2200 require: all compat aliases hard-broken with callers updated in the same commit, dead code removed, kind-integer const hierarchy unified under nmp-kinds. No shims, no parallel roadmaps, no debt carried forward.
