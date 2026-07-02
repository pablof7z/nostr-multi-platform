# Native Binding Surface Reference

> **Reviewed:** 2026-07-02 after #2763 (`nmp-uniffi` deleted).
>
> **Current public native target:** UniFFI for iOS, Android, and desktop native
> hosts. There is no stock, consumable UniFFI facade crate. Every native app
> owns exactly one UniFFI facade crate composed over `nmp-native-runtime` and
> `nmp-uniffi-support` (shared bridge mechanics: dispatch, update-sink wiring,
> panic containment, quiescence, clamps). `apps/nmp-gallery/crates/nmp-app-gallery`
> (`GalleryApp`) is the in-repo reference facade; the `nmp init` scaffold
> (`crates/nmp-cli/templates/facade_lib.rs.tmpl`) is the reference starting
> point for new apps.
>
> **Browser target:** `wasm-bindgen` through `nmp-browser-runtime`; browser/wasm
> is not part of the native ABI collapse.
>
> **Payload target:** FlatBuffers remain the hot action/update payload bytes.
> UniFFI carries `Vec<u8>` / `ByteArray` frames; it does not replace `NMPD`
> dispatch envelopes or `NMPU` update frames with UniFFI records.

This document describes the maintained binding direction after the M14 raw
native ABI deletion work. It is not a compatibility catalog for deleted
framework C symbols. Deleted framework native C symbols are not current public
API and must not be reintroduced unless #2125 records a measured internal
exception behind the UniFFI API.

## Public Native Surface

Native app shells use generated UniFFI bindings. There is no stock, consumable
framework binding crate — `nmp-uniffi` was deleted in #2763 after shipping with
zero real consumers (every shipped native app — gallery, 29er, Chirp,
podcast-player — built its own facade over `nmp-uniffi-support` instead). The
canonical *mechanics* (dispatch, update-sink wiring, capability callbacks,
panic containment, quiescence, feed-session open/close/reopen, account-change
observation, clamp policy) live in `nmp-uniffi-support` (+ `nmp-native-runtime`).
The canonical *surface* is per-app: each native app owns exactly one UniFFI
facade crate that calls `uniffi::setup_scaffolding!()` once and exports its own
records/objects/callback interfaces into its own generated Swift/Kotlin
namespace. `apps/nmp-gallery/crates/nmp-app-gallery` (`GalleryApp`) is the
in-repo reference facade; the `nmp init` scaffold's generated facade crate
(`crates/nmp-cli/templates/facade_lib.rs.tmpl`) is the reference starting point
for new apps. Each app-owned facade's generated Swift/Kotlin bindings are
guarded per-app by its own `ci/check-uniffi-bindings.sh` (emitted by `nmp
init`), not by a shared framework-level drift gate.

This is the native binding lane for public onboarding. Starter native apps
should construct and configure the UniFFI object model over
`nmp-native-runtime` inside their own facade; retained low-level native
symbols, where they still exist during #2125 migration, are
internal/transitional/test-support and are not starter requirements. Browser
starter apps use the separate `nmp-browser-runtime` wasm-bindgen lane.

The public native app object is the facade's own `uniffi::Object` (e.g.
`GalleryApp`, or the scaffold's `{{facade_struct}}`), which owns
`nmp-native-runtime::NmpApp` by value inside its own `Arc<Facade>`. Its core
lifecycle is:

1. construct the facade object (`GalleryApp::new()` / `{{facade_struct}}::new()`);
2. apply pre-start configuration and feature/capability wiring exposed by the
   facade's own UniFFI methods;
3. register an update sink;
4. start the runtime;
5. dispatch actions/open sessions/render emitted state;
6. stop/reset/shutdown through the facade's UniFFI lifecycle methods.

The native shell renders and executes raw capabilities. Rust/NMP owns protocol
policy, relay routing, signing policy, durable state, retries, publish status,
session teardown, and cache truth.

## App-Owned UniFFI Facades

When an app needs app-specific protocol verbs that are not reusable framework
verbs, it may own a single app facade crate, for example `TwentyNinerApp` in
29er. That crate calls `uniffi::setup_scaffolding!()` and defines the
facade-local UniFFI object, records, and callback interfaces for the generated
Swift/Kotlin namespace.

The app facade must not copy NMP lifecycle, update-sink, capability, dispatch,
or quiescence mechanics. Those reusable mechanics live below the generated
surface in `nmp-native-runtime` and `nmp-uniffi-support`; the app facade adapts
its local UniFFI traits/records into those helpers. This keeps one native ABI
family per app while avoiding a second framework ABI or duplicated runtime
bridge policy.

App facades are for app-owned verbs and composition roots, not for bypassing
NMP ownership. Reusable Nostr protocol mechanisms still belong in NMP protocol
crates and the native runtime; native shells still render state and execute raw
capabilities only.

### Shared facade helper mechanics

`nmp-uniffi-support` shares the Rust mechanics a facade reuses below its
generated namespace. The stateless bridge mechanics (dispatch, update sink,
capability callback, action-result/lifecycle observers, clamp + start/configure
policy) shipped with #2494/#2498. The session/account-change mechanics (#2516)
close the remaining app-owned-facade gaps:

| Facade need | Reuse, never copy | Notes |
|---|---|---|
| Open/close a feed | `open_feed`, `close_feed` | Decode + validate + open through `NmpApp::open_feed`; idempotent close (D6). Every app-owned facade's own feed methods delegate to these. |
| Rebuild a feed after a perspective change | `reopen_feed` | Idempotent close of the prior handle + open from the retained declaration. For account-pinned feeds only — `ActiveUserFollows` feeds re-seed in place (see below). |
| React to an active-account change | `register_account_change_sink`, `unregister_account_change_sink`, `account_change_observer_from_sink` | Arc-sink + panic-contained wrapper over `nmp-native-runtime::NmpApp::register_identity_change_observer`. The sink receives only the new identity; it never captures the runtime. |

These helpers are bridge mechanics below ADR-0076's feed-shaped app helper.
Product facades should expose generated or app-owned feed methods, not
compiler selection, raw interest JSON, observer wiring, pull-controller wiring,
or teardown recipes.

Account-change handling has two layers and a facade picks the lighter one:

- Account-**reactive** feeds (`FeedSourceExpr::ActiveUserFollows` and friends) re-seed
  **in place** — the native runtime's identity-change wiring clears and
  repopulates the live session. No reopen, no facade glue.
- Account-**pinned** app-specific feeds (e.g. a NIP-29 joined-groups view
  bound to the active account) observe the change through
  `register_account_change_sink` and rebuild via `reopen_feed` from a
  facade method, where the facade already holds `&self.inner`.

### Safe runtime ownership (no raw `*mut NmpApp`)

There is deliberately **no** "owned runtime handle" helper. A facade owns its
`nmp-native-runtime::NmpApp` by value inside its own `Arc<Facade>` UniFFI object
and passes `&self.inner` to every support helper; the helpers borrow `&NmpApp`
and deliver callbacks through `Arc`-held sinks. No facade path needs to capture
a `*mut NmpApp`. The legacy raw address-capture pattern belonged to the deleted
raw native builder lane; the UniFFI-facade ownership model removes it structurally,
mirroring how the runtime's own account-change observers capture granular `Arc`
handles rather than the whole-app pointer. New facade code that reaches for a
raw runtime pointer is a smell — use the borrow + `Arc`-sink shape instead.

## FlatBuffers Through UniFFI

FlatBuffers are still the wire payload for the hot paths:

- `NMPD` dispatch envelopes enter through UniFFI `NmpApp::dispatch_action`.
- `NMPU` update frames leave through the UniFFI `UpdateSink::on_update`
  callback.
- feed/search/ref/mirror helpers may return or accept FlatBuffers bytes where
  the owned runtime contract is byte-shaped.

The automatic performance signal for this lane is
`ffi-transport-bench --standard --fail-on-gate`, wired through
`.github/workflows/perf-gates.yml`. It is intentionally narrower than the
legacy reactivity/firehose gates: it measures the current UniFFI byte transport
budget and fails only when the pre-registered 60fps-derived collapse threshold
is no longer met.

The UniFFI layer owns object lifetimes, callbacks, typed records/errors, and
host-language generation. It does not own the action/update schemas. Schema and
payload evolution remain owned by the FlatBuffers/codegen crates.

## `nmp-uniffi-support` Areas

`nmp-uniffi-support` does not call `uniffi::setup_scaffolding!()` and exports no
UniFFI records/objects/callback interfaces itself; it only shares the Rust-side
bridge mechanics every facade calls into. It is organized by mechanic:

| Module | Shared mechanics |
|---|---|
| `lib.rs` | Runtime bridge mechanics: `start_runtime`/`configure_runtime` (clamp policy via `clamp_visible`/`clamp_emit_hz`), `dispatch_action`/`dispatch_action_vec`, `set_update_sink`, `set_capability_callback`/`dispatch_capability_json`, `register_action_result_observer`/`clear_action_result_observer`, `set_lifecycle_callback`, and the shared `is_hex_pubkey` input guard. |
| `account.rs` | Active-account-change observation (`register_account_change_sink`, `unregister_account_change_sink`) — `Arc`-sink + panic containment over `NmpApp::register_identity_change_observer`. |
| `sessions.rs` | Feed open/close/reopen/page mechanics (`open_feed`, `close_feed`, `reopen_feed`, `load_older_feed`) over `NmpApp::open_feed`/`close_feed`/`load_older_feed_status`. |
| `ownership.rs` | Compiled crate-ownership descriptor for crate-ownership reports. |

Everything a public UniFFI surface actually exports — app lifecycle object,
action doorway, stateless helpers (NIP-19/21/content/intent), identity/signer/
relay verbs, reference resolution, capability/action/publish control,
feed/search/URI sessions, runtime config/diagnostics, mirror pull — is
facade-local: each app-owned facade composes these from `nmp-native-runtime`
and NMP protocol crates and exports them in its own generated namespace,
delegating the bridge mechanics above to `nmp-uniffi-support` rather than
reimplementing them. `apps/nmp-gallery/crates/nmp-app-gallery` (`facade.rs`,
`composition.rs`, `event_ref.rs`, `registry.rs`, `showcase.rs`) is the in-repo
reference for how one facade organizes these areas; the `nmp init` scaffold
(`facade_lib.rs.tmpl`) is the reference starting point for a new one.

Browser hosts use the separate `nmp-browser-runtime` `wasm-bindgen` surface. Do
not route browser guidance through UniFFI and do not use browser/wasm as a reason
to retain legacy native symbols.

## Deleted Legacy Native Symbols

#2403 completed the migrated raw native ABI deletion tracker, and #2463 deleted
the migrated runtime config/diagnostics native ABI. The following families are not
current framework public API:

- app-loop lifecycle/update callback/action doorway C symbols;
- stateless helper C exports migrated to app-owned UniFFI facades;
- lifecycle observer/signal C exports migrated to app-owned UniFFI facades;
- feed/search/URI session C exports migrated to app-owned UniFFI facades;
- mirror pull C exports migrated to app-owned UniFFI facades;
- capability/action/publish control C exports migrated to app-owned UniFFI facades;
- runtime config, input-intent dispatch, and diagnostics C exports migrated to
  app-owned UniFFI facades.

Historical references may name these symbols only as deleted history or test
evidence. New native guidance must name the UniFFI method, generated binding, or
Rust runtime seam that replaced the symbol.

## Remaining Raw Surfaces

Raw `extern "C"` functions may remain in app-owned delivery glue. They are not a
second framework native public API.

| Surface | Current status | Owner |
|---|---|---|
| Gallery app/native and Android bridge shims | App-owned delivery glue for `apps/nmp-gallery`, not reusable NMP framework ABI. | app owner |
| Marmot native surface under `crates/nmp-marmot` | Deleted. Current Marmot installation is Rust explicit composition through `nmp_marmot::install`; native shells use generic action dispatch plus pushed typed projections. | #2232 / #2495 |

Any proposal to keep a raw native byte lane after its UniFFI replacement exists
must meet ADR-0072's exception gate: measured production budget failure through
UniFFI bytes, an internal wrapper behind the UniFFI API, named owner,
thresholds, retest date, and delete trigger.

## Boundary Rules

- Native public documentation names UniFFI first.
- Browser public documentation names `wasm-bindgen`.
- FlatBuffers remain action/update payload bytes across both binding families.
- `nmp-native-runtime` owns runtime lifecycle; each app-owned UniFFI facade
  exposes it to its own generated native namespace, delegating bridge
  mechanics to `nmp-uniffi-support`. There is no shared framework facade
  crate.
- App-owned UniFFI facades may expose app-specific verbs, but exported UniFFI
  records/callback traits live in the owning facade namespace and shared NMP
  bridge mechanics live in `nmp-uniffi-support`.
- Native shells do not choose relays, mutate protocol tags, infer publish
  success, own retries, or cache product truth.
- Deleted legacy native symbols are not compatibility requirements.

## Verification Pointers

- `rg -n "pub extern \"C\" fn" apps crates` shows any remaining app-owned raw
  delivery glue.
- `rg -n "uniffi::export|uniffi::Object|callback_interface" crates/nmp-uniffi-support/src`
  shows the shared bridge mechanics (no `setup_scaffolding!()`, no exported
  records/objects — confirms the crate is mechanics-only).
- `rg -n "uniffi::export|uniffi::Object|callback_interface" apps/nmp-gallery/crates/nmp-app-gallery/src`
  shows the in-repo reference facade's exported native surface.
- App facade proofs should run `uniffi-bindgen generate --library` for Swift and
  Kotlin against the app cdylib, because Rust compile alone does not prove the
  generated native namespace.
- `bash ci/check-uniffi-bindings.sh` (per-app, emitted by `nmp init`) verifies
  generated native binding drift for that app's own facade when its UniFFI
  interfaces change.
- `cargo run -p nmp-testing --bin ffi-transport-bench --release -- --standard --fail-on-gate`
  verifies the UniFFI update-byte transport performance signal.
