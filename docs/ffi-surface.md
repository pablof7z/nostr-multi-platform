# Canonical FFI Surface Reference

> **Status:** reviewed 2026-05-18 — M10.5 re-scoped exit gate D1.

This document enumerates every C symbol, boundary-crossing type, ownership
invariant, and validation gate exposed by `crates/nmp-core/src/ffi.rs` (441
lines). Every claim is traceable to that file or to the cross-referenced
doctrine/ADR docs. Line citations are of the form `ffi.rs:LINE` and were
verified by reading the file.

The surface is a flat `extern "C"` raw C ABI: 16 `#[no_mangle]` functions —
**14 production** symbols Swift/C sees in shipping builds, plus **2
test-support-only** symbols compiled out of production. The opaque handle is
`*mut NmpApp` (`ffi.rs:18-23`), wrapping a command `Sender` plus the actor and
update-listener join handles.

## Relationship to ADR-0010 (honest gap statement)

The current `nmp_app_*` raw C ABI is the **kernel-only / pre-codegen** FFI
surface. ADR-0010 (`docs/decisions/0010-generated-app-enum-vs-type-erased-registry.md`)
decides that the *eventual canonical* FFI surface is a per-app **generated
UniFFI crate** `nmp-app-<name>` exposing concrete per-app `AppAction` /
`AppUpdate` / `ViewSpec` / `Capability*` enums produced by `nmp gen modules`.
ADR-0010 §Consequences states this explicitly: "A per-app FFI crate becomes the
canonical FFI surface; raw `nmp-ffi` is for the kernel only." The surface
documented here is precisely that raw kernel surface. **It does not implement
ADR-0010**: there is no codegen, no per-app crate, no UniFFI scaffolding, and no
typed module enums in `ffi.rs` today. It is the hand-written kernel ABI that
exists *before* the generated surface, used to prove kernel invariants. The
generated UniFFI surface is future work; this doc makes no claim that it is
present.

## 1. Production ABI (14 symbols — seen by Swift/C in shipping builds)

Every production function early-returns silently on null/invalid input and
never panics or returns a `Result`/error across FFI — this is the per-symbol
**D6** ("errors never cross FFI") evidence. Guards: `app_ref` (`ffi.rs:403`,
null-check + `&*app`), `c_string_argument` (`ffi.rs:412`, null-check + UTF-8 +
trim + reject-empty), `is_hex_pubkey` / `is_hex_id` (`crate::kernel`,
imported `ffi.rs:2`). The fire-and-forget `app.tx.send(...)` discards the send
result (`let _ =`), so a dead actor channel is also a silent no-op.

| Symbol | ffi.rs | Args | Validation | Ownership / threading | D6 silent-no-op |
|---|---|---|---|---|---|
| `nmp_app_new` | `:45` | none | n/a | Allocates via `Box::into_raw` (`:64`); spawns actor thread (`:51`) + update-listener thread (`:52`). **Caller owns the returned `*mut NmpApp` and must call `nmp_app_free` exactly once.** | Cannot fail across FFI; returns a pointer (never errors) |
| `nmp_app_free` | `:78` | `app: *mut NmpApp` | null-checked (`:79`) | Reclaims via `Box::from_raw` (`:82`); `Drop` (`:25`) sends `Shutdown` and **joins both threads — synchronous teardown, not fire-and-forget.** Idempotent on null; **NOT idempotent on double-free** (use-after-free / double-free is UB). | Returns `()`; null is a no-op |
| `nmp_app_set_update_callback` | `:88` | `app`, `context: *mut c_void`, `callback: Option<UpdateCallback>` | `app_ref` (`:93`); mutex lock guarded (`:96`) | Stores `{context as usize, callback}` (`:99`); `None` clears registration. Callback later invoked on the **update-listener thread** (`:52-62`), never the caller thread. | Null app or poisoned lock → early return |
| `nmp_app_start` | `:106` | `app`, `_events_per_second: c_uint` (unused legacy ABI param), `visible_limit: c_uint`, `emit_hz: c_uint` | `app_ref` (`:112`); `clamp_visible`/`clamp_emit_hz` | Sends `ActorCommand::Start` (`:116`). No allocation transfer. | Null app → early return |
| `nmp_app_configure` | `:123` | `app`, `_events_per_second` (unused legacy), `visible_limit`, `emit_hz` | `app_ref` (`:129`); clamp helpers | Sends `ActorCommand::Configure` (`:133`). | Null app → early return |
| `nmp_app_stop` | `:140` | `app` | `app_ref` (`:141`) | Sends `ActorCommand::Stop` (`:144`). | Null app → early return |
| `nmp_app_reset` | `:148` | `app` | `app_ref` (`:149`) | Sends `ActorCommand::Reset` (`:152`). | Null app → early return |
| `nmp_app_open_author` | `:156` | `app`, `pubkey: *const c_char` | `app_ref` (`:157`); `c_string_argument` (`:160`); `is_hex_pubkey` (`:163`) | Sends `ActorCommand::OpenAuthor` (`:167`); `pubkey` String is owned by the actor. | Null/non-UTF8/empty/non-hex-pubkey → early return |
| `nmp_app_open_thread` | `:171` | `app`, `event_id: *const c_char` | `app_ref` (`:172`); `c_string_argument` (`:175`); `is_hex_id` (`:178`) | Sends `ActorCommand::OpenThread` (`:182`). | Null/non-UTF8/empty/non-hex-id → early return |
| `nmp_app_open_firehose_tag` | `:186` | `app`, `tag: *const c_char` | `app_ref` (`:187`); `c_string_argument` (`:190`) — no hex check (free-form tag) | Sends `ActorCommand::OpenFirehoseTag` (`:194`). | Null/non-UTF8/empty → early return |
| `nmp_app_claim_profile` | `:198` | `app`, `pubkey`, `consumer_id: *const c_char` | `app_ref` (`:203`); `c_string_argument` ×2 (`:206`,`:209`); `is_hex_pubkey` (`:212`) | Sends `ActorCommand::ClaimProfile` (`:216`). | Any invalid arg → early return |
| `nmp_app_release_profile` | `:223` | `app`, `pubkey`, `consumer_id` | `app_ref` (`:228`); `c_string_argument` ×2 (`:231`,`:234`); `is_hex_pubkey` (`:237`) | Sends `ActorCommand::ReleaseProfile` (`:241`). | Any invalid arg → early return |
| `nmp_app_close_author` | `:248` | `app`, `pubkey` | `app_ref` (`:249`); `c_string_argument` (`:252`); `is_hex_pubkey` (`:255`) | Sends `ActorCommand::CloseAuthor` (`:259`). | Any invalid arg → early return |
| `nmp_app_close_thread` | `:263` | `app`, `event_id` | `app_ref` (`:264`); `c_string_argument` (`:267`); `is_hex_id` (`:270`) | Sends `ActorCommand::CloseThread` (`:274`). | Any invalid arg → early return |

## 2. Test-support-only ABI (2 symbols — NEVER in production ABI)

Both are gated on `#[cfg(any(test, feature = "test-support"))]` (`ffi.rs:292`,
`:363`) and are therefore **excluded from the production FFI surface — Swift/C
in shipping builds never see these symbols**. This is the **D0** gate:
`docs/product-spec/doctrine.md` D0 — "Internal test-facing surface … is gated
behind `#[cfg(any(test, feature = "test-support"))]` so production builds export
no actor internals." The capability boundary is the `VerifiedEvent` type:
**production code constructs a `VerifiedEvent` only via `try_from_raw` (full
Schnorr + id-hash verification)**; `from_raw_unchecked` is the legacy harness
fast path that bypasses Schnorr verification and is reachable only through these
cfg-gated symbols.

| Symbol | ffi.rs | Args | VerifiedEvent path | Notes |
|---|---|---|---|---|
| `nmp_app_inject_pre_verified_events` | `:295` | `app`, `base_id_prefix: *const c_char`, `base_created_at: u64`, `count: u32` | `VerifiedEvent::from_raw_unchecked` (`:347`) — **bypasses Schnorr** (placeholder 128-zero sig, `:345`) | Legacy perf-harness compatibility only. Same `app_ref` silent-no-op contract (`:301`). Null prefix → `"stress"` default (`:304`). |
| `nmp_app_inject_signed_events` | `:366` | `app`, `base_created_at: u64`, `count: u32` | `VerifiedEvent::try_from_raw` (`:396`) — **full Schnorr** via `Keys::generate` + `EventBuilder::text_note` + `sign_with_keys` (`:378-385`) | Preferred for new harnesses (S3/S4/S5). `app_ref` silent-no-op (`:373`). |

## 3. Boundary-crossing types

`UpdateCallback = extern "C" fn(*mut c_void, *const c_char)` (`ffi.rs:10`).

| Type | Role | Allocates | Frees | Thread-affinity | Nullability |
|---|---|---|---|---|---|
| `*mut NmpApp` | Opaque handle (`:18-23`) | Rust, `Box::into_raw` in `nmp_app_new` (`:64`) | Rust, `Box::from_raw` in `nmp_app_free` (`:82`); `Drop` joins threads (`:25-42`) | Created on caller thread; the contained actor/listener run on their own spawned OS threads (`:51-52`) | Created non-null; all production fns null-check via `app_ref` (`:403`) |
| `*const c_char` | C string args (pubkey, event_id, tag, consumer_id, base_id_prefix) | Caller (C/Swift side) | Caller; Rust copies into an owned `String` and never frees the C buffer | Read synchronously on the calling thread inside the FFI fn | Nullable; `c_string_argument` (`:412`) returns `None` on null → silent no-op |
| `*mut c_void` | Callback context (`set_update_callback`) | Caller; Rust stores it as `usize` (`:100`), never derefs it itself | Caller-owned; Rust never frees | Stored on caller thread; passed back on the update-listener thread (`:59`) | Opaque; passed through verbatim, no null check |
| `c_uint` | Config scalars (`_events_per_second`, `visible_limit`, `emit_hz`) | By value (no allocation) | n/a | Calling thread | Not a pointer; `0` is the "use default" sentinel in clamp helpers |
| `UpdateCallback` | `extern "C" fn(*mut c_void, *const c_char)` (`:10`) | Caller supplies the fn pointer | n/a | **Invoked on the update-listener thread (`:52-62`), NOT the caller thread.** The `*const c_char` payload is a `CString` owned by Rust, valid only for the duration of the callback call (`:54-59`) — the callee must copy before returning. | Registered as `Option<UpdateCallback>`; `None` clears it (`:99`) |

## 4. Capability / validation boundary

| Helper | ffi.rs | Source | Behavior |
|---|---|---|---|
| `is_hex_pubkey` | imported `:2` | `crate::kernel` | Rejects any pubkey arg that is not a valid hex pubkey; failure → silent early return (`open_author`, `claim_profile`, `release_profile`, `close_author`) |
| `is_hex_id` | imported `:2` | `crate::kernel` | Rejects any event-id arg that is not a valid hex id; failure → silent early return (`open_thread`, `close_thread`) |
| `c_string_argument` | `:412` | local | null → `None`; non-UTF-8 → `None`; then `trim`s and rejects empty (`:422-423`); returns owned `String`. The string-sanitization half of the silent-no-op contract. |
| `clamp_visible` | `:427` | local | `0` → `DEFAULT_VISIBLE_LIMIT` (`crate::relay`, `:3`); else `clamp(1, 500)` → `usize` (`:431`) |
| `clamp_emit_hz` | `:435` | local | `0` → `DEFAULT_EMIT_HZ` (`crate::relay`, `:3`); else `clamp(1, 12)` (`:439`) |

## 5. Cross-reference: doctrine & RMP bible

- **D0** (`docs/product-spec/doctrine.md` §D0) — kernel + extension modules; test-facing surface cfg-gated out of production. The two `inject_*` symbols (§2) are the concrete instance: `#[cfg(any(test, feature = "test-support"))]` excludes them from the production ABI.
- **D6** (`doctrine.md` §D6, "Errors never cross FFI as exceptions") — every production fn (§1) early-returns on invalid input; no `Result`/exception/error type ever crosses the boundary. Mirrors `docs/aim.md` §2 bible invariant #2 ("Errors do not cross FFI") and bible rule #3 (`dispatch()` is fire-and-forget — here, `let _ = app.tx.send(...)`).
- **D7** (`doctrine.md` §D7, "Capabilities report; never decide policy") and bible invariant #7 (idempotent capability lifecycle, `aim.md` §2) — `nmp_app_free` is idempotent on null (`ffi.rs:79`) but **not** on double-free; the callback bridge stores only an opaque context + fn pointer and decides nothing.
- **ADR-0010** — see §"Relationship to ADR-0010" above: this raw C ABI is the kernel-only pre-codegen surface; the generated per-app UniFFI crate is the eventual canonical surface and is not implemented here.
