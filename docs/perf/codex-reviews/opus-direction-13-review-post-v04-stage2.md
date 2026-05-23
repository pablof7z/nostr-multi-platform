# NMP Direction Review #13 — 2026-05-24 (post-V-04 Stage 2 / post-Notes spike)

Reviewer: Opus.
Tree: master + uncommitted edits on signer/REPL/iOS settings as of `28cf348d`.
Reading scope (uncovered files only — see brief): `aim.md`, `plan.md`, `WIP.md`,
`apps/notes/` (Rust + Swift), `crates/nmp-signers/src/signers/nip46/mod.rs` +
`handle.rs`, `crates/nmp-signer-broker/src/relay_client.rs` + `broker.rs`,
`crates/nmp-repl/src/{lib,main,fanout}.rs`,
`crates/nmp-core/src/actor/relay_mgmt.rs`,
`crates/nmp-core/src/kernel/{perf_tests,raw_event_observer}.rs`,
`crates/nmp-core/src/actor/commands/raw_event_observer.rs`,
`crates/nmp-core/src/relay_worker/no_polling_tests.rs`.

Reviews #1–#12 covered: WASM stub (now Stages 2/3c merged), bespoke FFI debt
(48 → calendar), 4Hz snapshot model (now instrumented), stateful-app spike
(PR #377 closed PD-033-A), codegen F-05, V-04 dual subscription systems.
This review deliberately does not rehash any of those.

## What the last reviews missed

Reviews #6 and #11 looked at the framework thesis from the **DX angle** and
the **codegen angle**, but neither read the `apps/notes/` artifact itself in
detail. The artifact does not actually use the framework's defining feature
(D3 outbox routing) and pushes a third of the work into Swift — formatting,
JSON parsing, list ordering — which the RMP bible explicitly enumerates as
the very anti-patterns the framework is meant to make impossible (`aim.md`
§2 lines 62–69). The "second-app spike confirmed" claim in `plan.md` and
PD-033-A is therefore proven against an artifact that violates the things
the framework was built to prevent.

Separately, reviews focused on `nmp-core/src/relay_worker` (correctly
celebrated as readiness-driven, no polling) never looked at the *parallel*
relay client the broker spawned alongside it. That client reimplements the
same seam and reintroduces the exact polling pattern the project has a
dedicated CI test to ban (`relay_worker/no_polling_tests.rs`).

## Highest-signal finding — the Notes spike is not a framework proof

`apps/notes/nmp-app-notes/src/lib.rs:74` registers an empty
`nmp_app_notes_init`. All real work happens in 96 LOC of
`NotesBridge.swift`. Inspect what that Swift code actually does:

- **`NotesBridge.swift:74`** —
  `nmp_app_register_raw_event_observer(raw, ctx, cb, "[1]")` registers a
  **kind filter only**. The implementation
  (`crates/nmp-core/src/actor/commands/raw_event_observer.rs:92–116`,
  `kernel/raw_event_observer.rs:55–76`) is a *tap*: every kind-matching
  signed event the kernel ingests is fanned out regardless of author. There
  is no per-author NIP-65 outbox routing applied to what the observer
  receives. Notes therefore consumes whatever the kernel happens to
  subscribe to globally — D3 ("outbox routing automatic", `aim.md` §6.5) is
  bypassed entirely.
- **`NoteModel.swift:14`** parses the NIP-01 event JSON in Swift
  (`JSONSerialization` → `[String: Any]`). The bible's first
  anti-pattern is "Duplicated formatting logic across platforms — Rust
  pre-formats into strings, native renders them" (`aim.md` §2 line 64).
  The codegen pilot F-05 was explicitly written to make this impossible.
- **`NotesBridge.swift:84`** does the timeline ordering in Swift
  (`notes.insert(note, at: 0)` keyed off the JSON arrival order, not the
  event's `created_at`). The kernel owns no timeline view for this app.
- **`TimelineView.swift:30,36–38`** formats relative timestamps and
  shortens pubkeys to display form. Bible anti-pattern #1 again.
- **`NotesBridge.swift:36–37`** —
  `func signInBunker(_ uri:){ uri.withCString{ nmp_app_signin_bunker(...) }; isSignedIn = true }`.
  `isSignedIn` is set synchronously, with no await on the bunker handshake
  succeeding and no failure path. A bunker URI that fails to handshake
  leaves the UI in the "signed in" state forever.

PD-033-A was closed on PR #377 with "299 LOC Swift, 0 new C-ABI symbols."
The LOC count is real; the proof is not. The framework's distinguishing
properties — kernel-owned views, kernel-owned formatting, outbox-routed
reads, lifecycle states that gate on real success — are absent from the
artifact that allegedly demonstrates them. Notes is "299 LOC Swift on a
generic kind-tap," not a framework app.

**Action:** rewrite Notes to (a) register a `LogicalInterest` for kind:1
from the active user's follow set (forcing real outbox routing through the
planner), (b) consume a kernel-owned timeline projection (no JSON in
Swift, no list mutation in Swift), and (c) gate `isSignedIn` on a real
handshake-success callback. If that requires new framework affordances,
those are the *real* v1-A gap and the lack of them is what PD-033-A should
have surfaced. If it does not, then v1 ships without a single honest
example of how someone is supposed to build on this thing.

## Other findings

### 1. The broker reimplements the relay seam and reintroduces the polling antipattern the project has a CI test against

`crates/nmp-signer-broker/src/relay_client.rs:103` calls
`set_read_timeout(&mut socket, Duration::from_millis(100))`, and the
worker loop at `:154–217` interleaves `cmd_rx.try_recv()` with a
short-timeout `socket.read()`. That is precisely the pattern banned by
`crates/nmp-core/src/relay_worker/no_polling_tests.rs:1–35`, which fails
the production build if any of `set_read_timeout`, `Duration::from_millis(50)`,
or `.try_recv()` appears in `relay_worker/{mod,io_ready,socket_io}.rs`. The
banned-token test does not extend to the broker because the broker is a
different crate — but the doctrine (`feedback_no_polling`, AGENTS.md §No
polling — ever) is project-wide.

The broker explains the parallel client at `relay_client.rs:6–15` with "the
broker's needs are simpler … hard to do without leaking NIP-46 specifics
into the kernel (D0)." That reasoning is real, but the consequence is
that the project paid the D0 tax twice: once to keep `nmp-core` clean of
NIP-46, and again to reinvent a worse version of the readiness-driven
socket loop the kernel already solved. The right shape is a sibling crate
that exposes a generic `RelayConnection` trait — readiness-driven, owned
by the broker, sharing the `relay_protocol.rs` primitives that PR #375
already extracted into always-compiled `nmp-core`.

**Action:** lift `BrowserRelayDriver`-style readiness loop semantics into
a `nmp-relay-conn` crate (or extend `relay_protocol`); have both the
native relay worker and the broker depend on it. Delete the polling worker
in `TungsteniteRelayClient::run_worker`.

### 2. Bunker has no reconnect — first relay flap kills the session silently

`relay_client.rs` exposes only `send` + `shutdown`; `broker.rs:114` only
exposes `cancel`. There is no path in either file that reopens the socket
after a relay-side `Close`, a TCP reset, or a transient TLS error.
`run_worker` returns on any read or write failure (`relay_client.rs:159,
194, 213`); when that thread dies, every subsequent `signer.sign()` call
times out after `REMOTE_SIGN_TIMEOUT` (5s) and surfaces as a generic
backend error.

NIP-46 is listed in `aim.md` §4.6 as a first-class signer alongside nsec.
For any user who signs in via bunker (which is the default flow Notes
demonstrates and the path most non-developer users will take), an
intermittent relay drop turns the app into a brick that requires
re-signing in. There is no UI surface for "bunker connection lost,
reconnecting" because the broker has no state for it. V-06 / V-08 push
this to post-v1, but those tickets cover *NIP-42 AUTH compatibility* and
*DM decryption* — not basic transport resilience. The transport gap is
unticketed.

**Action:** before v1-A ships, the broker needs (a) an explicit
`reconnect` path with the same backoff/jitter constants the native
`relay_worker` uses (now in `relay_protocol.rs`), and (b) a
`BunkerHandshakeProgress::TransportLost { reconnect_in_ms }` event so the
UI can render a non-silent state. Either that, or `aim.md` and the v1
copy stop listing NIP-46 as a v1 sign-in method.

### 3. The snapshot perf gate measures the wrong workload

`crates/nmp-core/src/kernel/perf_tests.rs:128` is what review #5 asked
for — but the test runs against `Kernel::new()` with **zero registered
projections** and a 1k-event firehose. Production Chirp registers a
dozen-plus projections (zaps, follows, dm_inbox, profiles, wallet,
NIP-65 mailboxes, etc.) and a real user has 10k+ events in the
working set after 30 minutes of scrolling. `make_update` iterates every
registered projection on every tick; the cost grows with both event
count and projection count, and the gate measures neither.

The 10× headroom (`MAX_MAKE_UPDATE_US = 250_000`) compounds the
problem: the gate cannot catch a regression that adds 5× cost in
projection iteration when the test has no projections to iterate. A
real iOS user hitting a 4Hz tick budget will see jank long before the
gate fires.

**Action:** parameterize the perf test to register a representative
projection set (or reuse `nmp-app-chirp::register`) and scale events to
10k. Set ceilings off a re-measured baseline at that workload. Treat the
existing 1k-events-zero-projections test as a smoke test, not the v1
exit criterion.

### 4. `Nip46Rpc::encrypted_payload` is a footgun the type system permits

`crates/nmp-signers/src/signers/nip46/mod.rs:230–240` constructs a
`Nip46Rpc` with `encrypted_payload: body_json` — the same plaintext used
for `body_json`. The comment says "kept as plain JSON here so unit tests
can inspect what would have been sent; the production transport is
responsible for performing the encryption per its policy contract."
That's a contract the field name actively undermines: any future
transport implementer who reads `encrypted_payload` and forwards it
unchanged will leak NIP-46 RPC bodies. The compiler will not stop them.

The footgun is mitigated today because the broker's
`BrokerTransport::send_rpc` re-encrypts before the wire, but D7
("capabilities report, never decide policy") says the capability shouldn't
need to second-guess what its inputs mean.

**Action:** rename the field to `body_json_to_encrypt` (or wrap in a
`Plaintext<T>` newtype), or move the encryption into the signer so
`encrypted_payload` is genuinely encrypted by the time it leaves
`nmp-signers`.

### 5. `nmp-repl`'s "read-only" copy is stale; it has scope-crept into a dev MLS client

`nmp-repl/src/main.rs:266` greets the user with "nmp-repl v0.1 —
diagnostic REPL (read-only). type 'help' or 'quit'." The verb list at
`:21–53` includes `create-account`, `load-key`, `mls-init`, `mls-create`,
`mls-invite`, `mls-accept`, `mls-send`. Those are writes. Either the
read-only framing is obsolete and the REPL is now a development MLS
client (which would explain `commands/mls_*.rs` totaling ~hundreds of
LOC), or the MLS commands are unmaintained behind a feature flag nobody
runs. The 3,662 LOC of `nmp-repl` source is non-trivial for a tool whose
scope is undocumented.

**Action:** decide. If the REPL is the MLS dev surface, update the
greeting and add it to the v1 exit criteria as a supported developer
tool. If it's diagnostic, delete the MLS commands and remove the `mls`
feature plus the `marmot` dep. The current ambiguity costs maintenance
each time MLS or the broker churns.

### 6. The "Nostr-aware UI component registry" in BACKLOG §5 should be moved to "never until X"

The line item depends on (a) stable snapshot projection contracts and (b)
a target-platform decision (SwiftUI vs. UniFFI vs. wasm). Neither
prerequisite exists; the project's own bespoke FFI calendar (PD-039)
shows projections still in flux. Putting it on the post-v1 list as
"deferred" suggests it's coming; in practice it's blocked on architectural
decisions that haven't been scheduled. List it as "blocked on F-05
complete + UniFFI shipped" so future agents don't pick it up speculatively.

### 7. There is no business-model conversation anywhere in the docs

`aim.md` ends at §9 with "What this document is not"; `plan.md` ends at
v1 exit. There is no document in `docs/` that discusses who pays for
framework development after v1, what the licensing model is, or what the
upgrade contract looks like for downstream consumers. The framework is
being built as a public library (the doctrine "make it nearly impossible
to build a broken Nostr application," `aim.md` §1) but no plan exists for
sustaining it. For a project explicitly modeled on RMP (Rust
Multiplatform), which is open-source-by-default with no commercial
backstop, this is a structural risk worth at least one ADR.

## What to kill

- **`TungsteniteRelayClient::run_worker`** (`relay_client.rs:145–217`).
  Polling worker that the project already solved better in
  `relay_worker/io_ready.rs`. Replace with a shared readiness-driven
  client.
- **`nmp-repl` MLS verbs** (commands/mls_*.rs) — unless the REPL is
  formally adopted as the MLS dev tool, these are dead weight inside a
  binary that claims to be read-only.
- **PD-033-A "confirmed"** in `plan.md:25` and BACKLOG.md:309 — re-open
  it. The artifact doesn't prove what the closure claims (see Highest-
  signal finding).
- **"Second non-social app (shipped product)"** in BACKLOG §5 — kill the
  ambiguity. If the v1 spike is a thesis test (which it is), say so in
  the table and remove the "as a product" footnote that implies a future
  ship.
- **"Nostr-aware UI component registry"** in BACKLOG §5 — move to a
  blocked-on list, not the post-v1 queue (see finding #6).

## 30-day call

Rewrite `apps/notes/` so it actually uses the framework — kernel-owned
timeline projection driven by an outbox-routed `LogicalInterest` over the
user's follow set, with zero JSON parsing or list ordering in Swift, and
`isSignedIn` gated on a real bunker handshake-success event. The
rewrite either succeeds (in which case Notes becomes the first honest
example for would-be consumers and `nmp-repl` MLS verbs become moot) or
it surfaces concrete framework gaps (in which case those gaps are the
real v1-A backlog, not F-01/F-02/F-04/F-05). Either outcome is more
valuable than continuing to call PD-033-A confirmed against an artifact
that violates the architectural bible the framework's identity rests on.
