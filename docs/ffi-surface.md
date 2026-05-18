# Canonical FFI Surface Reference

> **Status:** reviewed 2026-05-18 — M10.5 re-scoped exit gate D1.
> Pinned to `origin/master` @ `91d76b9`.

This document enumerates every exported C symbol, boundary-crossing type,
ownership invariant, and validation gate of the raw `nmp_app_*` C ABI. Line
citations are `<file>:LINE` against the **committed** tree at the pinned SHA and
were verified by reading those files.

> **M10.5 finding (correctness).** The FFI surface was a single
> `crates/nmp-core/src/ffi.rs` until commit `7f4953d`
> (*feat(pulse): wire T66a identity/publish/account FFI surface*) split it into a
> module: `ffi/mod.rs` (lifecycle + read-side) and `ffi/identity.rs`
> (identity / publish / multi-account / relay-edit). `ffi.rs` no longer exists.
> An earlier draft of this doc described the pre-split single file and was
> **stale + incomplete** (missed the 12 `ffi/identity.rs` symbols); it is
> superseded by this revision. A third file, `ffi/capability.rs`, is **in-flight
> in a concurrent session and NOT yet on `origin/master`** at review time — its
> three symbols are listed in §2b and tagged *pending; re-verify when landed*.

The surface is a flat `extern "C"` raw C ABI regardless of the Rust module
split (`ffi/identity.rs:6-7`). Committed total: **28 `#[no_mangle]` functions —
26 production** symbols Swift/C sees in shipping builds, plus **2
test-support-only** symbols compiled out of production. The opaque handle is
`*mut NmpApp` (`ffi/mod.rs:24-29`), wrapping a command `Sender` plus the actor
and update-listener join handles.

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
typed module enums in the `ffi` module today. It is the hand-written kernel ABI
that exists *before* the generated surface, used to prove kernel invariants. The
generated UniFFI surface is future work; this doc makes no claim it is present.

## 1. Production ABI — lifecycle + read-side (`ffi/mod.rs`, 14 symbols)

Every production function early-returns silently on null/invalid input and
never panics or returns a `Result`/error across FFI — this is the per-symbol
**D6** ("errors never cross FFI") evidence. Shared guards: `app_ref`
(`ffi/mod.rs:409`, null-check + `&*app`), `c_string_argument` (`ffi/mod.rs:418`,
null + UTF-8 + trim + reject-empty), `c_optional_string_argument`
(`ffi/mod.rs:438`, null/empty → `None` for genuinely-optional args),
`is_hex_pubkey` / `is_hex_id` (`crate::kernel`, imported `ffi/mod.rs:8`). The
fire-and-forget `let _ = app.tx.send(...)` discards the send result, so a dead
actor channel is also a silent no-op.

| Symbol | mod.rs | Args | Validation | Ownership / threading | D6 silent-no-op |
|---|---|---|---|---|---|
| `nmp_app_new` | `:51` | none | n/a | Allocates via `Box::into_raw` (`:70`); spawns actor thread (`:57`) + update-listener thread (`:58`). **Caller owns the `*mut NmpApp`; must `nmp_app_free` exactly once.** | Returns a pointer; cannot error across FFI |
| `nmp_app_free` | `:84` | `app: *mut NmpApp` | null-checked (`:85`) | Reclaims via `Box::from_raw` (`:88`); `Drop` (`:31`) sends `Shutdown` and **joins both threads — synchronous teardown**. Idempotent on null; **NOT idempotent on double-free** (UB). | `()`; null is a no-op |
| `nmp_app_set_update_callback` | `:94` | `app`, `context: *mut c_void`, `callback: Option<UpdateCallback>` | `app_ref` (`:99`); mutex-guarded (`:102`) | Stores `{context as usize, callback}` (`:105`); `None` clears. Callback later fires on the **update-listener thread** (`:58-68`), never the caller thread. | Null app / poisoned lock → early return |
| `nmp_app_start` | `:112` | `app`, `_events_per_second: c_uint` *(unused legacy ABI param)*, `visible_limit`, `emit_hz` | `app_ref` (`:118`); `clamp_visible`/`clamp_emit_hz` | Sends `ActorCommand::Start` (`:122`). | Null app → early return |
| `nmp_app_configure` | `:129` | `app`, `_events_per_second` *(unused legacy)*, `visible_limit`, `emit_hz` | `app_ref` (`:135`); clamp helpers | Sends `ActorCommand::Configure` (`:139`). | Null app → early return |
| `nmp_app_stop` | `:146` | `app` | `app_ref` (`:147`) | Sends `ActorCommand::Stop` (`:150`). | Null app → early return |
| `nmp_app_reset` | `:154` | `app` | `app_ref` (`:155`) | Sends `ActorCommand::Reset` (`:158`). | Null app → early return |
| `nmp_app_open_author` | `:162` | `app`, `pubkey: *const c_char` | `app_ref` (`:163`); `c_string_argument` (`:166`); `is_hex_pubkey` (`:169`) | Sends `ActorCommand::OpenAuthor` (`:173`); `pubkey` owned by actor. | Null/non-UTF8/empty/non-hex → early return |
| `nmp_app_open_thread` | `:177` | `app`, `event_id: *const c_char` | `app_ref` (`:178`); `c_string_argument` (`:181`); `is_hex_id` (`:184`) | Sends `ActorCommand::OpenThread` (`:188`). | Null/non-UTF8/empty/non-hex → early return |
| `nmp_app_open_firehose_tag` | `:192` | `app`, `tag: *const c_char` | `app_ref` (`:193`); `c_string_argument` (`:196`) — no hex check (free-form tag) | Sends `ActorCommand::OpenFirehoseTag` (`:200`). | Null/non-UTF8/empty → early return |
| `nmp_app_claim_profile` | `:204` | `app`, `pubkey`, `consumer_id: *const c_char` | `app_ref` (`:209`); `c_string_argument` ×2 (`:212`,`:215`); `is_hex_pubkey` (`:218`) | Sends `ActorCommand::ClaimProfile` (`:222`). | Any invalid arg → early return |
| `nmp_app_release_profile` | `:229` | `app`, `pubkey`, `consumer_id` | `app_ref` (`:234`); `c_string_argument` ×2 (`:237`,`:240`); `is_hex_pubkey` (`:243`) | Sends `ActorCommand::ReleaseProfile` (`:247`). | Any invalid arg → early return |
| `nmp_app_close_author` | `:254` | `app`, `pubkey` | `app_ref` (`:255`); `c_string_argument` (`:258`); `is_hex_pubkey` (`:261`) | Sends `ActorCommand::CloseAuthor` (`:265`). | Any invalid arg → early return |
| `nmp_app_close_thread` | `:269` | `app`, `event_id` | `app_ref` (`:270`); `c_string_argument` (`:273`); `is_hex_id` (`:276`) | Sends `ActorCommand::CloseThread` (`:280`). | Any invalid arg → early return |

## 1b. Production ABI — identity / publish / account / relay (`ffi/identity.rs`, 12 symbols)

Added by `7f4953d` (T66a). Same silent-no-op contract; reuses the parent
module's `app_ref` / `c_string_argument` / `c_optional_string_argument`
(`ffi/identity.rs:9`). All 12 are unconditional production symbols.

| Symbol | identity.rs | Args | Validation | Actor command | D6 silent-no-op |
|---|---|---|---|---|---|
| `nmp_app_signin_nsec` | `:15` | `app`, `secret: *const c_char` | `app_ref` (`:16`); `c_string_argument` (`:19`) | `SignInNsec` (`:22`) | Invalid → early return |
| `nmp_app_signin_bunker` | `:26` | `app`, `uri: *const c_char` | `app_ref` (`:27`); `c_string_argument` (`:30`) | `SignInBunker` (`:33`) | Invalid → early return |
| `nmp_app_create_new_account` | `:37` | `app` | `app_ref` (`:38`) | `CreateAccount` (`:41`) | Null app → early return |
| `nmp_app_switch_active` | `:45` | `app`, `identity_id: *const c_char` | `app_ref` (`:46`); `c_string_argument` (`:49`) | `SwitchActive` (`:52`) | Invalid → early return |
| `nmp_app_remove_account` | `:56` | `app`, `identity_id` | `app_ref` (`:57`); `c_string_argument` (`:60`) | `RemoveAccount` (`:63`) | Invalid → early return |
| `nmp_app_publish_note` | `:67` | `app`, `content`, `reply_to_id_or_null: *const c_char` | `app_ref` (`:72`); `c_string_argument` (`:75`); `c_optional_string_argument` (`:78`, null reply ⇒ top-level) | `PublishNote` (`:79`) | Null/empty content → early return; null reply is *valid* (top-level) |
| `nmp_app_react` | `:86` | `app`, `target_event_id`, `reaction` | `app_ref` (`:91`); `c_string_argument` (`:94`); `is_hex_id` (`:97`); reaction defaults `"+"` (`:100`) | `React` (`:101`) | Invalid/non-hex target → early return |
| `nmp_app_follow` | `:108` | `app`, `pubkey` | `app_ref` (`:109`); `c_string_argument` (`:112`); `is_hex_pubkey` (`:115`) | `Follow` (`:118`) | Invalid/non-hex → early return |
| `nmp_app_unfollow` | `:122` | `app`, `pubkey` | `app_ref` (`:123`); `c_string_argument` (`:126`); `is_hex_pubkey` (`:129`) | `Unfollow` (`:132`) | Invalid/non-hex → early return |
| `nmp_app_add_relay` | `:136` | `app`, `url`, `role` | `app_ref` (`:141`); `c_string_argument` (`:144`); role defaults `"both"` (`:147`) | `AddRelay` (`:148`) | Null/empty url → early return |
| `nmp_app_remove_relay` | `:152` | `app`, `url` | `app_ref` (`:153`); `c_string_argument` (`:156`) | `RemoveRelay` (`:159`) | Invalid → early return |
| `nmp_app_open_timeline` | `:163` | `app` | `app_ref` (`:164`) | `OpenTimeline` (`:167`) | Null app → early return |

## 2. Test-support-only ABI (`ffi/mod.rs`, 2 symbols — NEVER in production ABI)

Both gated on `#[cfg(any(test, feature = "test-support"))]` (`ffi/mod.rs:298`,
`:369`) and therefore **excluded from the production FFI surface — shipping
Swift/C never sees them**. This is the **D0** gate (`doctrine.md` D0: test-facing
surface gated behind `test-support` so production exports no actor internals).
The capability boundary is the `VerifiedEvent` type: **production constructs a
`VerifiedEvent` only via `try_from_raw` (full Schnorr + id-hash)**;
`from_raw_unchecked` is the legacy harness fast path reachable only through these
cfg-gated symbols.

| Symbol | mod.rs | Args | VerifiedEvent path | Notes |
|---|---|---|---|---|
| `nmp_app_inject_pre_verified_events` | `:301` (cfg `:298`) | `app`, `base_id_prefix`, `base_created_at: u64`, `count: u32` | `from_raw_unchecked` (`:353`) — **bypasses Schnorr** (placeholder 128-zero sig, `:351`) | Legacy perf-harness only. `app_ref` silent-no-op (`:307`). Null prefix → `"stress"` (`:310`). |
| `nmp_app_inject_signed_events` | `:372` (cfg `:369`) | `app`, `base_created_at: u64`, `count: u32` | `try_from_raw` (`:402`) — **full Schnorr** via `Keys::generate` + `EventBuilder::text_note` + `sign_with_keys` (`:384-391`) | Preferred for new harnesses (S3/S4/S5). `app_ref` silent-no-op (`:379`). |

## 2b. In-flight: `ffi/capability.rs` (NOT on origin/master @ 91d76b9)

These three symbols exist only in a concurrent session's working tree
(uncommitted at review time). Listed for completeness; **pending — re-verify
line citations and contract when the orchestrator lands the file on master.**
Signatures observed in the working-tree file (line numbers provisional):

| Symbol | Provisional | Purpose (provisional) |
|---|---|---|
| `nmp_app_set_capability_callback` | `capability.rs:~40` | Register a capability-event callback (mirror of `set_update_callback` for the capability channel) |
| `nmp_app_dispatch_capability` | `capability.rs:~66` | Dispatch a capability action into the actor |
| `nmp_app_free_string` | `capability.rs:~84` | Free a Rust-allocated `*mut c_char` handed out across FFI (ownership-return symbol) |

`nmp_app_free_string` is doctrinally notable: it is the first symbol where Rust
**hands an allocation to the caller and reclaims it**, unlike every §1/§1b
symbol (caller owns all `*const c_char` inputs; Rust copies and never frees
them). Its ownership contract must be pinned in this doc once committed.

## 3. Boundary-crossing types

`UpdateCallback = extern "C" fn(*mut c_void, *const c_char)` (`ffi/mod.rs:16`).

| Type | Role | Allocates | Frees | Thread-affinity | Nullability |
|---|---|---|---|---|---|
| `*mut NmpApp` | Opaque handle (`mod.rs:24-29`) | Rust, `Box::into_raw` in `nmp_app_new` (`:70`) | Rust, `Box::from_raw` in `nmp_app_free` (`:88`); `Drop` joins threads (`:31-47`) | Created on caller thread; actor/listener run on own spawned OS threads (`:57-58`) | Created non-null; all production fns null-check via `app_ref` (`:409`) |
| `*const c_char` | C string args (pubkey, event_id, tag, consumer_id, secret, uri, content, url, …) | Caller (C/Swift) | Caller; Rust copies into an owned `String`, never frees the C buffer | Read synchronously on the calling thread inside the FFI fn | Nullable; `c_string_argument` (`:418`) / `c_optional_string_argument` (`:438`) → `None` on null |
| `*mut c_void` | Callback context (`set_update_callback`) | Caller; Rust stores as `usize` (`:106`), never derefs it | Caller-owned; Rust never frees | Stored on caller thread; passed back on the update-listener thread (`:65`) | Opaque; passed through verbatim, no null check |
| `c_uint` | Config scalars (`_events_per_second`, `visible_limit`, `emit_hz`) | By value | n/a | Calling thread | Not a pointer; `0` = "use default" sentinel in clamp helpers |
| `u64` / `u32` | Test-support scalars (`base_created_at`, `count`) | By value | n/a | Calling thread | Not a pointer |
| `UpdateCallback` | `extern "C" fn(*mut c_void, *const c_char)` (`:16`) | Caller supplies the fn pointer | n/a | **Invoked on the update-listener thread (`:58-68`), NOT the caller thread.** The `*const c_char` payload is a Rust-owned `CString` valid only for the call's duration (`:60-65`) — callee must copy before returning. | `Option<UpdateCallback>`; `None` clears (`:105`) |

## 4. Capability / validation boundary

| Helper | mod.rs | Source | Behavior |
|---|---|---|---|
| `is_hex_pubkey` | imported `:8` | `crate::kernel` | Rejects non-hex-pubkey args → silent early return (`open_author`, `claim_profile`, `release_profile`, `close_author`, `follow`, `unfollow`) |
| `is_hex_id` | imported `:8` | `crate::kernel` | Rejects non-hex-id args → silent early return (`open_thread`, `close_thread`, `react`) |
| `c_string_argument` | `:418` | local | null → `None`; non-UTF-8 → `None`; trims + rejects empty (`:428-429`); owned `String`. For **required** args (caller drops the call). |
| `c_optional_string_argument` | `:438` | local | null/empty → `None`, else `Some(value)`. For **genuinely optional** args (e.g. `reply_to_id` null = top-level note), not "drop the call". |
| `clamp_visible` | `:451` | local | `0` → `DEFAULT_VISIBLE_LIMIT` (`crate::relay`, `:9`); else `clamp(1, 500)` → `usize` (`:455`) |
| `clamp_emit_hz` | `:459` | local | `0` → `DEFAULT_EMIT_HZ` (`crate::relay`, `:9`); else `clamp(1, 12)` (`:463`) |

## 5. Cross-reference: doctrine & RMP bible

- **D0** (`docs/product-spec/doctrine.md` §D0) — kernel + extension modules; test-facing surface cfg-gated out of production. The two `inject_*` symbols (§2) are the concrete instance: `#[cfg(any(test, feature = "test-support"))]` excludes them from the production ABI.
- **D6** (`doctrine.md` §D6, "Errors never cross FFI as exceptions") — every production fn (§1, §1b) early-returns on invalid input; no `Result`/exception/error type crosses the boundary. Mirrors `docs/aim.md` §2 bible invariant "Errors do not cross FFI" and the fire-and-forget `dispatch()` rule (`let _ = app.tx.send(...)`).
- **D7** (`doctrine.md` §D7, "Capabilities report; never decide policy") — `nmp_app_free` is idempotent on null (`mod.rs:85`) but **not** on double-free; the callback bridge stores only an opaque context + fn pointer and decides nothing. The in-flight capability surface (§2b) must be re-reviewed against D7 when it lands.
- **ADR-0010** — see §"Relationship to ADR-0010": this raw C ABI is the kernel-only pre-codegen surface; the generated per-app UniFFI crate is the eventual canonical surface and is not implemented here.
