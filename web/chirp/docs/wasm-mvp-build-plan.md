# Chirp Web — wasm MVP build plan

> Status: plan (2026-06-12). Ordered PR sequence that turns the MVP gap list in
> [`product-spec.md`](product-spec.md) §5.1 (G1–G7 + the missing worker tick
> pump) into small, independently-landable PRs. Every gap claim below was
> re-verified against the code on this date; §6 lists where the product spec
> was wrong or incomplete. Scope: MVP = spec §2.1 M1–M8 + §3.1 D1–D5,
> **non-persistent preview** — no OPFS/IndexedDB store (#1007 / spec V11 is
> post-MVP).

Doctrine constraints carried throughout: the web shell renders snapshots and
dispatches actions, never computes protocol state; every gap is closed by
exposing or owning the capability in `nmp-core` / `nmp-wasm` (or a Rust
composition crate), never in TypeScript. Honest degraded modes only.

---

## 0. The headline verification result

The product spec's G1/G2 are **cheaper than specified**, and it **missed one
prerequisite**:

1. **`Kernel::make_update` already builds everything.**
   `crates/nmp-core/src/kernel/update.rs:212` (`pub(crate) fn make_update(&mut
   self, running: bool) -> UpdateFrameBytes`) encodes the FULL Tier-3 envelope
   (relay statuses incl. auth/negentropy/counters from
   `kernel/status.rs:23-88,107-138`, wire subscriptions, metrics, error
   fields) **and** merges the kernel-owned Tier-2 typed-projection sidecar
   (`kernel/typed_projections/mod.rs:282-370`: `configured_relays`,
   `relay_role_options`, `settings_hub`, the publish cluster
   `publish_queue`/`publish_outbox`/`outbox_summary`, the identity cluster
   `accounts`/`active_account`/`profile`, the profile/event cluster
   `mention_profiles`/`claimed_profiles`/`claimed_events`/`resolved_profiles`,
   and the diagnostics cluster `action_results`/`signed_events`/
   `action_stages`/`action_lifecycle`/`relay_diagnostics`) with host-registered
   Tier-1 entries (`snapshot_registry.rs:354,373`). It compiles **without**
   the `native` feature (`nmp-wasm` already depends on `nmp-core` with
   `default-features = false`, `crates/nmp-wasm/Cargo.toml`). So G1 plus the
   *emission* half of G2 collapse into one mechanical `KernelReducer`
   forwarding, and the bespoke wasm envelope builder
   (`crates/nmp-wasm/src/snapshot.rs:95-126`) gets **deleted**, not extended.

2. **Missed prerequisite — the wasm kernel never learns its relay set.**
   `Kernel::set_configured_relays`
   (`crates/nmp-core/src/kernel/identity_state.rs:334-395`) is what populates
   the kernel's `configured_relays`, the planner's routing lanes
   (`set_indexer_relays` / `set_app_relays` / `set_bootstrap_content_relays` /
   `set_bootstrap_indexer_relays`), the write-relay slot, and the
   `nmp.relay.configured_relays` Tier-2 projection. On native it is fed from
   `ActorCommand::Start { initial_relays }`. The wasm `Start`
   (`crates/nmp-wasm/src/runtime.rs:225-277`) never calls it — it only parks
   the bootstrap in `RuntimeMeta` and spawns drivers. Consequences today:
   snapshot relay rows would carry empty URLs
   (`kernel/mod.rs:2263` `bootstrap_urls_for_role` reads `configured_relays`),
   the planner's lifecycle compile has **no relay targets** (so G3's interest
   REQs would silently emit nothing), and the `no_configured_relays`
   diagnostic misfires once an account is set. PR-1 fixes this.

Everything else in the gap list verified as claimed — citations inline below.

---

## 1. Ordered PR sequence

### PR-1 — Kernel-authored snapshot frames + configured-relay hand-off (G1 + G2-emission)

**What it builds**

- `crates/nmp-core/src/kernel_reducer.rs` — two mechanical public
  forwardings on `KernelReducer`:
  - `pub fn make_update_frame(&mut self, running: bool) -> UpdateFrameBytes`
    → delegates to `Kernel::make_update` (`kernel/update.rs:212`). Same
    doc-comment discipline as the existing `publish_signed_event` /
    `claim_*` forwardings in the same file.
  - `pub fn set_configured_relays(&mut self, rows: Vec<AppRelay>)`
    → delegates to `Kernel::set_configured_relays`
    (`identity_state.rs:334`). `AppRelay` is already `pub use`d from
    `kernel/mod.rs:440`.
- `crates/nmp-wasm/src/runtime.rs` — `start()` maps the validated
  `RelayBootstrapEntry` list into `AppRelay` rows and calls
  `reducer.set_configured_relays(...)` before spawning drivers. Role-string
  mapping must mirror the native `has_role` convention
  (`identity_state.rs:347,378` — `"both"` ⇒ read+write; `"indexer"` stays
  `"indexer"`), matching what `NmpAppBuilder::with_relays` produces on native
  so routing behaves identically.
- `crates/nmp-wasm/src/snapshot.rs` — `build_snapshot_bytes` stops ignoring
  `_reducer` (line 95) and stops hand-building an envelope with hardcoded
  `connection: "configured"` (lines 102-126) and an empty sidecar (line 96):
  it becomes `reducer.borrow_mut().make_update_frame(meta.started)`. The
  bespoke `build_snapshot_envelope` is deleted; `RuntimeMeta` keeps only
  `started` / `database_name` (start-handshake echo) — `rev` becomes
  kernel-authored (`make_update` bumps `Kernel::rev`, `update.rs:222`), so
  the monotonic-rev guard on the host keeps working unchanged. Note
  `push_snapshot_if_callback` / call sites take `&mut` borrow now
  (`make_update` mutates: rev bump + drain-on-emit captures).
- `web/chirp/README.md:77-80` — fix the stale "nmp-core does not yet compile
  to browser wasm" claim (rider; spec already flags it).

**Acceptance check** (native, `cargo test -p nmp-wasm -p nmp-core`)

- New protocol-conformance test in `crates/nmp-wasm/tests/protocol.rs`:
  `Start` with two relays → drive `handle_relay_connected` + one synthetic
  NIP-01 `EVENT` text frame through the runtime's reducer → take a snapshot
  frame → `nmp_core::decode_snapshot_envelope` asserts: a relay row with the
  real bootstrap URL and `connection == "connected"`, `events_rx > 0`,
  non-zero `last_tick_ms`; `decode_snapshot_typed_projections` asserts the
  sidecar contains `nmp.relay.configured_relays` (decoding to the bootstrap
  rows) and the unconditional profile cluster keys.
- A `kernel_reducer.rs` unit test pinning that `make_update_frame` bumps rev
  monotonically and never panics on a fresh reducer (D6).

**What it unblocks (user-facing)** — the single biggest light-up:
D2 live relay table (real connection/auth/reconnect/events_rx/NOTICE), M8
error toasts (`last_error_toast`/`category` now ride the envelope), D9
store/cache metrics, D10 negentropy badge, D6 wire-sub inspector data, and —
because the Tier-2 sidecar now ships — M4 resolved-profile read-back
(`nmp.profile.resolved` / `claimed_profiles` are unconditional builtins), M7 +
D4 publish queue/outbox/summary with per-relay verdicts (the
`publish_path.rs:244-247` comment already promises "per-relay terminal
verdicts arrive via the `action_results` projection on the next snapshot
push" — this PR is what makes that true), D7 claim registry, D11 signer panel
data (`signed_events`).

**Dependencies / degraded mode** — no dependencies; this is the lead PR.
Absent: the snapshot stays the current honest-but-empty
`connection:"configured"` echo and nothing downstream can render.

**Layer ownership** — snapshot truth is `nmp-core`'s (`Kernel::make_update`
is the single producer on every platform); `nmp-wasm` only owns the
JS-callback transport. Deleting the wasm-side envelope builder removes a
divergence risk, it doesn't add one.

**Collision risk** — touches `nmp-core` but only `kernel_reducer.rs`
(additive methods; not a lint-sweep target) + `nmp-wasm`. **Low.**

---

### PR-2 — Worker tick driver (G6) + lifecycle-drain pump

**What it builds**

- `crates/nmp-core/src/kernel_reducer.rs` — extend `KernelReducer::tick()`
  (`kernel_reducer.rs:186-189`; it already exists, contrary to a literal
  reading of G6 — what's missing is the **call site**) to also drain the
  subscription lifecycle: append `Kernel::drain_lifecycle_outbound()`
  (`kernel/lifecycle_drain.rs:91`) before `partition_auth_paused`. Rationale:
  the native actor drains lifecycle on its idle tick; the reducer today
  drains it only inside `handle_relay_connected` (`kernel_reducer.rs:155`),
  so a `CompileTrigger` enqueued *while relays are already connected* (every
  PR-3 interest open, every kind:3-driven `FollowListChanged` recompile from
  `ingest/contacts.rs:299-320`) has no pump. One pump, byte-aligned with the
  native idle tick.
- `crates/nmp-wasm/src/runtime.rs` (+ small `tick.rs` module if the 500-LOC
  ceiling demands) — a timer started on `Start`, cancelled on `Stop`:
  every interval (1 Hz; retry deadlines are seconds-scale per the
  `tick()` doc, `kernel_reducer.rs:177-185`) call `reducer.tick()`, fan the
  outbound through `publish_path::fan_out_outbound`
  (`publish_path.rs:100-110`), and push a snapshot **iff outbound was
  non-empty** (no 1 Hz snapshot spam; D8 — the host supplies the timer, the
  kernel owns what happens per tick). Timer primitive: `gloo-timers`
  `Interval` (already used by `BrowserRelayDriver`'s ping cadence via
  `nmp-network`) or `setInterval` via `js-sys`; wasm32-only, native shim
  no-op.
- Optional rider (decide in review): move the per-inbound-frame snapshot push
  to a dirty-flag + tick-coalesced cadence (kernel already tracks
  `changed_since_emit`) — `make_update` per inbound frame runs every
  projection closure, which on a busy relay is hotter than native's 4 Hz emit
  loop. Not required for correctness; flag if frame rates hurt.

**Acceptance check** (native)

- `kernel_reducer.rs` test: register an interest (use `KernelAction::OpenUri`
  or — after PR-3 — `open_interest`), connect a relay, then assert `tick()`
  returns the compiled REQ frames (today it returns `[]` because only the
  publish engine is pumped). Plus a regression of the existing
  `tick_on_fresh_reducer_is_empty` (`kernel_reducer.rs:504`).
- `nmp-wasm` protocol test: publish through `publish_signed_event` to a
  relay, mark it failed, assert `tick()` re-emits after backoff (mirrors the
  native `tick_publish_engine_for_now` contract).

**What it unblocks** — M5/M7 reliability (publish retries actually fire in
the browser instead of waiting for an unrelated inbound frame) and is a hard
prerequisite for M3 (the follow-feed REQ emission rides
`FollowListChanged` triggers drained here).

**Dependencies / degraded mode** — independent of PR-1 (orders second only
because PR-1 is the bigger light-up). Absent: publishes that fail
transiently never retry; interests opened mid-session never compile until the
next relay reconnect — both silent liveness bugs, which is why this lands
before PR-3.

**Layer ownership** — cadence *policy* (what a tick does) is `nmp-core`'s;
`nmp-wasm` owns only the timer, mirroring "the kernel decides cadence policy;
the host only supplies the timer".

**Collision risk** — `nmp-core/src/kernel_reducer.rs` (one method body) +
`nmp-wasm`. **Low.**

---

### PR-3 — Interest + contact-feed dispatch verbs, viewer-pubkey hand-off (G3)

**What it builds**

- `crates/nmp-core` (the real work is moving shared logic *down*, not
  duplicating it):
  - Move `build_open_interest(filter_json, consumer_id, scope)` out of
    `actor/dispatch.rs:511-549` — the `dispatch` module is
    `#[cfg(feature = "native")]` (`actor/mod.rs:35`) so it is unreachable
    from the wasm build — into an always-compiled home (e.g.
    `subs/` next to `SubIdentity`, or a `kernel/interest.rs` sibling). The
    actor arm (`actor/dispatch.rs:1727-1760`) calls the moved fn; one source
    of truth for the filter→`(SubIdentity, LogicalInterest)` derivation.
  - `KernelReducer::open_interest(filter_json, consumer_id, scope) ->
    Vec<OutboundMessage>` / `close_interest(...)` — parse via the moved
    helper, call `Kernel::open_interest_sub` / `close_interest_sub`
    (`kernel/mod.rs:2714,2751` — stay `pub(crate)`; the reducer is in-crate),
    then drain inline (`drain_lifecycle_outbound` + `pending_view_requests` +
    `partition_auth_paused`, same pattern as `handle_relay_connected`,
    `kernel_reducer.rs:138-157`) so an open against already-connected relays
    emits its REQ immediately rather than waiting for the next tick.
  - `KernelReducer::set_follow_feed_kinds(kinds: BTreeSet<u32>) ->
    Vec<OutboundMessage>` — forwards to `Kernel::set_follow_feed_kinds`
    (`kernel/ingest/contacts.rs:293-297`), then the same inline drain. This
    is the reducer twin of the 10-line actor wrappers
    `open_contact_feed`/`close_contact_feed`
    (`actor/commands/publish.rs:698-731`), minus the `IdentityRuntime` check
    (replaced by the kernel's own `active_account` gate — `register_follow_
    feed_for_active_account` already early-returns on `None`,
    `contacts.rs:300-302`).
  - `KernelReducer::set_active_account(pubkey_hex: String) ->
    Vec<OutboundMessage>` — the viewer-pubkey hand-off. Sets
    `Kernel::active_account` (`kernel/mod.rs:1009`), writes the shared
    `ActiveAccountSlot` (`Kernel::active_account_handle`,
    `kernel/mod.rs:1763` — PR-4's `ActiveFollowSet` reads it), invokes
    `reconcile_follow_feed_after_identity_change` (`contacts.rs`, T168) and
    returns `active_account_bootstrap_requests()` (the self
    profile/NIP-65/contacts bootstrap REQs the native sign-in path emits,
    `actor/commands/identity.rs:904`) + inline drain. Without this the
    follow feed has no account and no kind:3 to expand.
- `crates/nmp-wasm`:
  - `dispatch_routing.rs` — four new verbs:
    `nmp.kernel.open_interest` / `close_interest`
    (payload: `filter_json`, `consumer_id`, `scope`) and
    `nmp.kernel.open_contact_feed` / `close_contact_feed`
    (payload: `kinds: [u32]`), mirroring the native FFI names
    (`docs/ffi-surface.md` §5–6, ADR-0042). Same D6 parse discipline as the
    existing `ClaimDispatch` arm (`dispatch_routing.rs:62-93`).
  - `runtime.rs` — the claim/verb arm fans the returned outbound through
    `fan_out_outbound` and pushes a snapshot (existing pattern,
    `runtime.rs:388-424`); `set_signer` success additionally calls
    `reducer.set_active_account(pubkey)` + fan-out + push.

**Acceptance check** (native, `cargo test -p nmp-wasm` + `-p nmp-core`)

- Protocol-conformance test: `SetSigner(nip07, pk)` → `Start` →
  `handle_relay_connected` → dispatch `open_contact_feed {kinds:[1]}` →
  feed a kind:3 frame for `pk` with follows → assert the outbound (inline or
  next `tick()`) contains a REQ whose compiled acquisition filter carries `authors = follows, kinds = [1,6]`, while the app-owned declaration remains primary `[1]`.
- Test: dispatch the declared author-feed seam with primary `[1]`, deriving
  `{"kinds":[1,6],"authors":[pk]}` against a connected relay → REQ emitted
  inline; matching close emits CLOSE. Re-open dedup (second owner attaches, no
  second REQ) mirrors the existing kernel tests
  (`actor/dispatch.rs:2254-2300`).
- `nmp-core` test: the moved `build_open_interest` keeps its exact behavior
  (existing tests move with it).

**What it unblocks** — M3 wire half (home-timeline REQs flow), M6 read half
(thread interest), V4/V5 later (author/hashtag are the same verb). With PR-1
already landed, the events land in the kernel store and the claim/profile
projections update — but **no feed projection reaches JS yet** (that's PR-4).

**Dependencies / degraded mode** — needs PR-2's drain pump for triggers
enqueued outside the verb call (kind:3 arrival → `FollowListChanged`).
Absent: signed-in users see no home timeline; the honest mode is the current
one — the verbs simply don't exist and the shell shows its
read-only/degraded banner.

**Layer ownership** — interest registry, follow-set expansion, planner
compile: all `nmp-core` (already true). The verbs are thin `nmp-wasm`
routing. The `{1,6}` kind policy stays host-supplied data (D0), exactly as
native Chirp passes `HOME_FEED_KINDS_JSON` through the generic verb
(`apps/chirp/nmp-app-chirp/src/ffi/interest_feed.rs:272-289`).

**Collision risk** — touches `nmp-core` more broadly than PR-1/2
(`actor/dispatch.rs` move + `kernel_reducer.rs` + `contacts.rs` call paths).
Peers are running lint sweeps in `nmp-core`-adjacent crates — coordinate the
`actor/dispatch.rs` move window. **Medium.**

---

### PR-4 — Web feed composition: `nmp.feed.home` + author/thread FlatFeeds (G4)

**What it builds**

- `crates/nmp-core/src/kernel_reducer.rs` — three mechanical registration
  seams (all the underlying kernel surfaces exist and are always-compiled):
  - `register_event_observer(Arc<dyn KernelEventObserver>)` — installs/uses
    the `KernelEventObserverSlot` via `Kernel::set_event_observers_handle`
    (`kernel/event_observer.rs:37-39`; slot type + `notify_observers` are
    un-gated, `actor/mod.rs:91`).
  - `register_typed_snapshot_projection(key, closure)` — reaches
    `SnapshotRegistry::register_typed` (`kernel/snapshot_registry.rs:354`),
    whose output `make_update` already merges (`kernel/update.rs:243,266`).
  - `active_account_handle()` and an event-by-id lookup forwarding (for the
    OP-feed `event_lookup` closure; native uses `NmpApp::event_by_id`).
- **New crate `apps/chirp/nmp-app-chirp-web`** (cdylib + rlib) — the wasm
  composition root, the web twin of
  `apps/chirp/nmp-app-chirp/src/ffi/interest_feed.rs` and
  `nmp-defaults::register_op_feed_defaults`. `nmp-wasm` stays substrate-grade
  ("no app nouns", `snapshot.rs:18-22`); this crate is where Chirp nouns
  live, mirroring the native `nmp-ffi` ↔ `nmp-app-chirp` split. It depends on
  `nmp-wasm` (rlib — the `#[wasm_bindgen]` `NmpWasmRuntime` exports survive
  into the downstream cdylib), `nmp-nip01`, `nmp-nip02`, and wires:
  - **Home feed** — `nmp_nip01::register_op_feed(viewer, follow_predicate,
    event_lookup, claim_sink)` (`nmp-nip01/src/op_feed/wiring.rs:117` — it is
    deliberately `NmpApp`-free; closures only). `follow_predicate` =
    `nmp_nip02::ActiveFollowSet` over the reducer's `ActiveAccountSlot`
    (also registered as its own event observer for kind:3 ingest, mirroring
    `op_feed_defaults.rs:204-212`); engine registered as event observer +
    typed projection under `nmp.feed.home` with the existing `NOFS` op-feed
    schema (`encode_op_feed_snapshot`, no new `.fbs`).
  - **Author/thread feeds** — `nmp_nip01::FlatFeed` with
    `author_feed_predicate` / `thread_feed_predicate`, registered under
    `nmp.feed.author.<pk>` / `nmp.feed.thread.<id>` + the matching
    `open_interest`/`close_interest` (PR-3 verbs), exactly the two-halves
    pattern of `interest_feed.rs:141-260` minus the store seeding (no
    persistent store on wasm; live ingest only — honest for a non-persistent
    preview). Exposed as wasm-bindgen methods or dispatch verbs on the new
    crate (`open_author_feed(pk)`, `open_thread_feed(id)`, closes).
  - **Claim-sink re-entrancy guard** — the OP-feed claim sink fires during
    observer fan-out, i.e. while the reducer is mutably borrowed by
    `handle_relay_frame`. On native it enqueues an actor command; the wasm
    sink must likewise **queue** (RefCell `VecDeque` of pending claims)
    and drain through `KernelReducer::claim_profile` after the frame
    returns / on tick — a direct re-entrant call would panic the `RefCell`.
    This is the one genuinely subtle piece of the PR; pin it with a test.
- `web/chirp`: wasm-pack build target switches from `crates/nmp-wasm` to the
  new crate (`web/chirp/README.md:66-69`); the shell decodes the `NOFS`
  op-feed sidecar (TS decoder generated into `web/chirp/src/nmp/generated/`
  alongside the existing frame bindings) and renders `HomePanel` from
  `nmp.feed.home`.

**Acceptance check** (native, `cargo test -p nmp-app-chirp-web`)

- Composition test mirroring `interest_feed.rs:548-612`: set active account,
  feed kind:3 + kind:1/6 frames through the reducer, take
  `make_update_frame`, `decode_snapshot_typed_projections`, find
  `nmp.feed.home`, `decode_op_feed_snapshot`, assert the cards (newest-first,
  reply rolled up under its OP per the engine semantics).
- Thread/author twin tests (open → cards present; close → key gone).
- Claim-sink test: an ingested event whose author needs a profile enqueues a
  claim that drains without panicking the reducer borrow.

**What it unblocks** — M3 render half (the home timeline actually appears),
M6 thread view, M4 completes end-to-end (cards + claims + resolved-profile
projections from PR-1). This is the PR after which Chirp Web looks like a
Nostr client.

**Dependencies / degraded mode** — hard on PR-1 (sidecar emission) and PR-3
(interests, contact feed, viewer pubkey). Absent: timeline events sit in the
kernel store invisible to JS; the shell shows the diagnostics panel only —
honest, but not a product.

**Layer ownership** — feed semantics (`OpFeedEngine`, `FlatFeed`, NIP-10
attribution): `nmp-nip01`/`nmp-feed`. Follow-set: `nmp-nip02`. Composition +
the `{1,6}` policy + `nmp.feed.*` keys: the new app crate (same altitude as
`nmp-app-chirp` on native). `nmp-wasm` and TypeScript own nothing here.

**Collision risk** — `nmp-core` only via `kernel_reducer.rs` seams; the new
crate is greenfield. **Low-medium** (workspace `Cargo.toml` member addition
is a classic merge-conflict line; trivial to resolve).

---

### PR-5 — Async write path: replies (G5, MVP slice)

**What it builds**

- Verified baseline: the wasm write path is `PublishNote` kind:1 top-level
  only — variant gate `publish_path.rs:158-172`, NIP-07-only backend gate
  `:174-182`, reply fail-closed `:190-196`; correlation-id threading into the
  engine already works (`:232-235`,
  `kernel_reducer.rs:228-238`).
- `crates/nmp-core` — extract the native reply-tag builder from
  `actor/commands/publish.rs::publish_note` (it "walks the kernel's events
  read-cache for NIP-10 root/parent reply tags", per the comment at
  `publish_path.rs:185-189`) into an always-compiled shared builder (e.g.
  `publish/builders.rs`) callable with a kernel event lookup; native command
  path calls the moved code (move, don't duplicate — same rule as PR-3's
  `build_open_interest`). Expose
  `KernelReducer::build_reply_tags(reply_to_id) -> Option<Vec<Vec<String>>>`
  (or fold into a `build_unsigned_note` helper) so tag derivation never
  leaves Rust.
- `crates/nmp-wasm/src/publish_path.rs` — replace the `reply_to_id`
  fail-closed arm: resolve root/parent through the reducer **before** the
  sign `.await` (borrow discipline at `:133-142` — nothing borrowed across
  the await), build the unsigned kind:1 with NIP-10 marker tags, then the
  existing sign → `publish_signed_event` → fan-out → push flow unchanged.
  Unknown `reply_to_id` (event not in the store) stays fail-closed with a
  stable reason (`reply_target_unknown:`-prefixed) — never publish a reply
  with fabricated markers (honest degraded mode).

**Acceptance check** (native)

- Builder tests pinning byte-identical tags with the native `publish_note`
  path for: reply-to-root, reply-to-reply (root+parent markers), missing
  target.
- `publish_path.rs` reason-string tests updated: replies no longer return
  `publish_path_not_wired_for_kind`; the unknown-target reason has a stable
  prefix (the existing prefix-pinning test pattern at
  `publish_path.rs:261-277`).

**What it unblocks** — M6 write half: reply from the thread view. Completes
the MVP loop (post → thread → reply).

**Dependencies / degraded mode** — pairs with PR-4 (threads must render to
be replied to); technically independent of PR-3/4 at the code level. Absent:
replies fail closed with the current honest reason — the compose box for
replies stays disabled with the kernel's reason rendered verbatim.

**Layer ownership** — NIP-10 semantics: `nmp-core`'s shared builder (sourced
from the kernel event cache). `nmp-wasm` owns only the sign-Promise
orchestration. TypeScript never constructs tags.

**Collision risk** — `nmp-core/src/actor/commands/publish.rs` is in the zone
peers' lint sweeps touch (cf. recent `nmp-signers`/`nmp-planner` lint PRs).
**Medium** — keep the extraction commit minimal and mechanical.

---

### PR-6 — Async write path: React / Follow / Unfollow (G5 remainder; v1, immediately post-MVP)

**What it builds** — same seam as PR-5, kind by kind (the PR-boundary note at
`publish_path.rs:25-34` explicitly anticipates this): extract the native
NIP-25 `k`-tag derivation (react command) and the kind:3 follow-set merge
(**from kernel contact state** — `Kernel::seed_contacts`, never the shell)
into the shared builders; route `AppAction::React`/`Follow`/`Unfollow`
(`protocol.rs:105-115`) through `publish_app_action`. Follow/Unfollow with no
cached kind:3 fails closed (`contact_list_unknown:` prefix) rather than
publishing a destructive single-entry kind:3 — the same trap the native
command guards against.

**Acceptance** — builder parity tests vs. the native command path; protocol
tests per variant (accepted, and the two fail-closed arms). Closes issue
#1007's wasm half.

**Unblocks** — V1/V2 (like, follow buttons). Not in the MVP definition of
done; ordered here because the marginal cost after PR-5's extraction is
small.

**Collision risk** — same `nmp-core` zone as PR-5. **Medium.**

---

### PR-7 — F-TTL `force` claim parity (G7; tiny, any time after PR-1)

**What it builds** — `ClaimDispatch::ClaimProfile`/`ClaimEvent` gain an
optional `force: bool` payload field (default `false`,
`dispatch_routing.rs:40-93`); `runtime.rs:393-408` passes it through instead
of hardcoding `false`. ~30 lines, `nmp-wasm` only.

**Acceptance** — `dispatch_routing.rs` parse tests (absent ⇒ false, present
⇒ honored); a runtime test that `force: true` reaches
`KernelReducer::claim_profile`.

**Unblocks** — V4 pull-to-refresh / explicit-navigation refresh. Not
MVP-critical. **Collision: none (nmp-wasm only).**

---

## 2. Dependency graph

```
PR-1 (snapshot + configured relays)          [nmp-core: kernel_reducer.rs; nmp-wasm]
 ├──> PR-4 (feed composition)  ──────────┐
 ├──> PR-7 (force flag)                  │
 │                                       │
PR-2 (tick + lifecycle drain)            │   [nmp-core: kernel_reducer.rs; nmp-wasm]
 └──> PR-3 (verbs + viewer pubkey) ──────┤   [nmp-core: dispatch move + reducer; nmp-wasm]
                                         ▼
                                   MVP renders (M3/M4/M6-read)
PR-5 (replies)  ── independent code-path; product-depends on PR-4 (threads UI)
 └──> PR-6 (React/Follow/Unfollow)           [post-MVP fast-follow]
```

Land order: **1 → 2 → 3 → 4 → 5** (MVP), then 6, 7. PR-7 can interleave
anywhere after 1. Low-collision first: PR-1, PR-2, PR-7 touch `nmp-core` only
in `kernel_reducer.rs` (or not at all); PR-3 and PR-5 carry the
`nmp-core` move-refactors — schedule them around the peers' active lint
sweeps in that crate.

Each PR has a thin TypeScript counterpart (decode + render only) that can
ride the same PR or trail it: PR-1 → relay table / toasts / outbox panels;
PR-3/4 → home & thread views; PR-5 → reply composer. No JS counterpart
contains logic, so none blocks the Rust ordering.

## 3. MVP definition of done

When PR-1…PR-5 are merged and the shell renders them (spec §4.1, restated
against this plan):

A stranger with a NIP-07 extension can: **sign in** (M2; `SetSigner` +
viewer-pubkey hand-off, PR-3) → **read their follows' live feed** with names
and avatars (M3 = PR-2+3+4; M4 = PR-1 projections + PR-4 claims) → **post a
note** (M5; already wired) and watch "accepted by N/M relays" (M7 = PR-1
`action_results`/publish cluster + PR-2 retries) → **open a thread and
reply** (M6 = PR-3/4 read + PR-5 write) — while the diagnostics panel shows
the live relay table (D2 = PR-1), routing decisions (D3; already wired,
`lib.rs:141-144`), the publish outbox (D4 = PR-1), and the snapshot heartbeat
(D5; already wired). Without the extension, the app still browses (a JS-host-
supplied default interest through the PR-3 generic verb — configuration, not
logic) and every panel renders kernel truth or is absent. Reload loses state:
expected and honest for the non-persistent preview (V11/#1007 is post-MVP).

## 4. Test-infrastructure note

There is **no wasm-bindgen-test infra** in the workspace (pinned by the
comment at `crates/nmp-wasm/src/lib.rs:199-201`); every acceptance check
above is a native test (`crates/nmp-wasm/tests/protocol.rs` protocol-
conformance suite + in-crate unit tests), which is how the crate is tested
today. Per-PR test scope: `cargo test -p nmp-wasm -p nmp-core` (plus the new
app crate for PR-4) and always
`cargo test -p nmp-testing --test doctrine_lint_smoke`. Browser-level
verification (real WebSocket + NIP-07) stays a manual checklist per PR until
a wasm-test rig is justified — proposing that rig is out of MVP scope.

## 5. Collision-risk summary

| PR | nmp-core surface | Risk |
|---|---|---|
| PR-1 | `kernel_reducer.rs` (additive) | Low |
| PR-2 | `kernel_reducer.rs` (one body) | Low |
| PR-3 | `actor/dispatch.rs` move, `kernel_reducer.rs`, `ingest/contacts.rs` visibility | Medium |
| PR-4 | `kernel_reducer.rs` seams; new workspace member | Low-medium |
| PR-5 | `actor/commands/publish.rs` extraction | Medium |
| PR-6 | same zone as PR-5 | Medium |
| PR-7 | none (nmp-wasm only) | None |

## 6. Corrections to the product-spec gap list (verified 2026-06-12)

1. **G1+G2 emission collapse** — `Kernel::make_update`
   (`kernel/update.rs:212`) already emits the full envelope **and** the
   merged typed sidecar, no-`native`-feature clean. The spec's "needs a
   mechanical public read accessor (relay statuses, wire subscriptions,
   metrics, error fields)" over-specifies: one forwarding suffices, and the
   wasm envelope builder is deleted. G2's *remaining* substance is only the
   registration seams (Tier-1 projections + event observers) consumed by
   PR-4.
2. **Missed: `Kernel::set_configured_relays` is never called on wasm**
   (`identity_state.rs:334`; native feeds it from `ActorCommand::Start`).
   Without it the planner has no routing lanes — G3's verbs would emit no
   REQs — and snapshot relay rows have no URLs. Folded into PR-1.
3. **G6 wording** — `KernelReducer::tick()` exists
   (`kernel_reducer.rs:186`); the spec is right that nothing in `nmp-wasm`
   calls it, but the core half of the fix is that the reducer's tick pumps
   only the publish engine — lifecycle triggers (`drain_lifecycle_outbound`,
   `lifecycle_drain.rs:91`) drain only on `handle_relay_connected`. PR-2
   extends the pump; otherwise M3's follow-feed REQs would still starve.
4. **`build_open_interest` is native-gated** (`actor/mod.rs:35` gates `mod
   dispatch`) — the spec's "forward `open_interest_sub` through
   `KernelReducer`" needs the filter-parsing helper moved down first (PR-3),
   not just a forwarding.
5. **M4 needs no registration seam** — `resolved_profiles` /
   `claimed_profiles` are *unconditional Tier-2 builtins*
   (`typed_projections/mod.rs:355-360`), so profile read-back lights up with
   PR-1 alone; the spec filed it under the G2 registration work.
6. **G16/D14 partially moot on wasm** — the drain-on-emit action-lifecycle
   projections (`action_results` etc.) are drained per `make_update` call
   (`builtins_diagnostics` capture note, `typed_projections/mod.rs:361-369`);
   on wasm every drained frame is delivered through the push callback, so
   "reducer-side drain semantics" need no new core decision for the MVP's M7
   use (per-relay verdict surfacing). The animated D14 timeline remains
   post-MVP.
7. **New hazard the spec didn't flag** — the OP-feed claim sink fires inside
   kernel observer fan-out while the wasm reducer's `RefCell` is mutably
   borrowed; the wasm composition (PR-4) must queue claims and drain
   post-frame. On native the actor command queue absorbs this; the reducer
   path has no queue, so a naive port would panic.
8. Verified-as-claimed, for the record: snapshot hardcoding
   (`snapshot.rs:95-126`), encoder slot
   (`update_envelope.rs:249-310`), `open_interest_sub` visibility
   (`kernel/mod.rs:2714`), `set_follow_feed_kinds` wrapper
   (`actor/commands/publish.rs:698-731`; kernel method
   `ingest/contacts.rs:293`), FlatFeed twin (`interest_feed.rs`),
   write-path fail-closed arms (`publish_path.rs:158-196`), F-TTL `force`
   hardcoded false (`runtime.rs:398,404`), no wasm-bindgen-test infra
   (`lib.rs:199-201`), `register_op_feed` is `NmpApp`-free
   (`nmp-nip01/src/op_feed/wiring.rs:117`).
