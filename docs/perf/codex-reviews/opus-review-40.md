# Opus Direction Review #40

Date: 2026-05-21
Reviewer: Opus (direction review #40)
Scope: NIP-17 phasing, the kind:1059 multiplexing question, ADR-0026,
post-#119 pattern audit.

## TL;DR

1. **The kind:1059 multiplexing fear is unfounded.** Both observer slots
   fan out N-way (snapshot the `Vec`, iterate every match). A future
   `DmInboxProjection` registering a raw observer on kind:1059 coexists
   with Marmot's `MarmotIngestTap` cleanly. Not a blocker. Evidence below.
2. **Phasing verdict: build `DmInboxProjection` + a Swift DM screen
   NEXT — not a `dm.rs` seam upgrade, and pause ADR-0026.** The receive
   side is the first step that produces a user-visible behavior. Send
   without receive cannot be opened in the app to verify anything, and
   PR #122's send path is mostly undeliverable today anyway (relay
   routing TODO).
3. **PR #122 merged alone, with nothing following this cycle, is inert
   surface #N.** Reviews #33–#39 have said the consumer pipe is the
   failure mode six times. Say it a seventh: "Phase 1 complete" is not
   permission to ship and stop.
4. ADR-0026 as currently scoped (symmetric `nip44_encrypt` +
   `nip44_decrypt` on `RemoteSignerHandle`) is a speculative-symmetry
   seam. The real demand is *decrypt on receive*. Let that demand shape
   the seam; don't pre-build both halves with zero consumers.

---

## 1. The kind:1059 multiplexing question — answered concretely

**Does the kernel fan out to multiple observers, or does
last-registration win?** It fans out. Both slots.

`crates/nmp-core/src/actor/commands/event_observer.rs:267-312`
(`notify_observers`): snapshots `guard.rust` and `guard.c_abi` into
`Vec`s under the lock, releases the lock, then `for observer in
&rust_snapshot { observer.on_kernel_event(event) }`. Every registration
fires. `register_rust_observer` (line 204) does `guard.rust.push(...)` —
append, never replace.

`crates/nmp-core/src/actor/commands/raw_event_observer.rs:223`
(`register_raw_observer`): `guard.rust.push((id, kinds, observer))` —
same append semantics. `notify_raw_observers` (line 282) iterates all
registrations whose `KindFilter` matches `raw.kind`.

The only "last-registration wins" in the codebase is the *slot binding
itself* — `Kernel::set_event_observers_handle`
(`kernel/event_observer.rs:37`) replaces `self.event_observers`. That is
a one-time actor-init call, not a per-observer concern. Within a slot,
registrations are purely additive.

**How does Marmot subscribe to kind:1059?** Via the **raw** observer
tap, not the lossy `KernelEventObserver`. `MarmotIngestTap` implements
`RawEventObserver` with `TAP_KINDS = [443, 444, 445, 1059, 30443]`
(`crates/nmp-marmot/src/projection/tap.rs:59`). It needs the verbatim
`sig` because MDK requires a *signed* `nostr::Event` to unwrap a
gift-wrap. `MarmotProjection`'s `KernelEventObserver::on_kernel_event`
(`projection/state.rs:446-452`) explicitly **early-returns for
kind:1059** — the comment says so: "kind:445 / kind:1059 require a
signed event — driven by the raw signed-event tap".

**What happens if `DmInboxProjection` also subscribes to kind:1059?**
Nothing bad. It registers a second `RawEventObserver` (it also needs the
`sig` to NIP-44-unseal the gift-wrap). The kernel's
`notify_raw_observers` calls *both* taps for every accepted kind:1059
event. Marmot's tap tries MDK unwrap; on a NIP-17 gift-wrap MDK's
`process_welcome` fails and `ingest_signed_event_core` returns `Err`,
which the tap discards (D6 silent no-op — `tap.rs:96-100`). The DM tap
tries NIP-44 unseal; on a Marmot Welcome gift-wrap *that* fails and is
discarded. Each tap silently ignores the other's traffic. This is the
designed behavior — the raw tap rustdoc at
`raw_event_observer.rs:14-16` calls it a "generic verbatim-signed-event
seam" precisely so multiple protocol consumers can share it.

**Verdict: multiplexing is a non-issue. Do not let it gate
`DmInboxProjection`.** One small follow-up worth noting (not blocking):
both taps will do a redundant parse + failed-unwrap on each other's
events. At current event volumes this is free. If DM + Marmot inboxes
both grow large, a `tags`-aware pre-filter (kind:1059 alone is shared;
the gift-wrap inner kind is not visible until unwrap) could help — but
that is a perf footnote, not an architecture decision. Do not build it
now.

---

## 2. NIP-17 phasing verdict

### What has shipped / is in flight

- **PR #122 (open, CI green-ish)** — `nmp-nip17` crate (pure kind:14
  rumor builder, no crypto, no key material) + `ActorCommand::
  SendGiftWrappedDm { rumor, recipient_pubkey }` + `actor/commands/
  dm.rs` handler. Local-keys-only; bunker accounts fail explicitly.
  **No `nmp.dm.send` registration, no FFI, no UI.** The PR body says
  "infrastructure only" in plain words — honest.
- **ADR-0026 (agent building now)** — extend `RemoteSignerHandle` with
  `nip44_encrypt` / `nip44_decrypt`. No consumer.

### The discriminating question

The task framed Phase 3 as a binary: "update `dm.rs` to use the seam, or
build `DmInboxProjection` first?" That is the wrong axis. The right
axis, given six consecutive reviews diagnosing the consumer-pipe
pathology: **which step first produces a behavior a user can see in the
app?**

- `dm.rs` seam upgrade → upgrades bunker compatibility on the **send**
  side. Still no receive side, still no screen. Invisible.
- `DmInboxProjection` + Swift DM screen → the user can open Chirp, see a
  DM someone sent them, and (with PR #122's send path) reply. Visible.

Send-without-receive is unverifiable end-to-end. You cannot dogfood it.
It is exactly the "shipped-but-inert, camouflaged by green CI" pattern
review #33 named as the #1 project risk.

### The verdict

**Next PR: `DmInboxProjection`.** A `RawEventObserver` registered on
kind:1059 that, for each gift-wrap addressed to the local pubkey:
NIP-44-unseals with the local key → extracts the kind:13 seal →
unseals again → yields the kind:14 rumor → folds it into a snapshot
projection (`nmp.dm.inbox` or similar) registered via
`register_snapshot_projection`. Then a minimal Swift DM screen consuming
that projection. Ship the receive side and the screen *in the same
cycle* as PR #122's send side, so the merged result is a thing a human
can use.

**Then** decide the bunker story with real demand in hand.

### Pause ADR-0026 — or at least re-scope it

ADR-0026 as described extends `RemoteSignerHandle` with **both**
`nip44_encrypt` and `nip44_decrypt`. Two observations:

1. **The crypto already exists.** `Nip46Signer` (the `Signer`-trait
   impl) already has a working NIP-44 namespace —
   `crates/nmp-signers/src/signers/nip46/mod.rs:311-320` enqueues
   `nip44_encrypt` / `nip44_decrypt` RPCs to the bunker. `LocalKeySigner`
   (`signers/local.rs:287`) and `Nip07Signer` (`signers/nip07.rs:136`)
   also impl `Nip44`. ADR-0026 is **not** new crypto — it is plumbing
   the existing `Signer::nip44()` namespace through to the *actor-facing*
   `RemoteSignerHandle` trait (`crates/nmp-core/src/remote_signer.rs`),
   which today exposes only `sign()` + `deliver_rpc_response()`. Scope it
   honestly as "surface the existing Nip44 namespace on the actor seam,"
   not "build NIP-44 for remote signers."

2. **Symmetric encrypt+decrypt is speculative.** The receive side
   (`DmInboxProjection`) needs **decrypt**. The send side already works
   for local keys (PR #122) and needs **encrypt** only to extend to
   bunker users — a strictly-later concern. Building both halves now,
   with `DmInboxProjection` not yet written, lands ADR-0026 as another
   inert seam: a trait method nothing calls.

   **Recommendation:** either (a) pause ADR-0026 until
   `DmInboxProjection` exists and let the decrypt requirement drive the
   exact method signature, or (b) if it must proceed, scope it to
   `nip44_decrypt` only, with `nip44_encrypt` deferred to the bunker-send
   phase. Do not merge a symmetric two-method extension with zero
   consumers.

   If you've already retrieved evidence that the ADR-0026 agent is far
   along: still land it scoped-down, or land it but immediately wire
   `DmInboxProjection` as the consumer in the *same* cycle. The
   non-negotiable is no merged trait method without a caller.

### Blocking caveat on PR #122's "infrastructure" framing

`dm.rs` publishes **both** gift-wrap envelopes (recipient + self-copy)
to the actor's configured **Content relays**, not the recipient's
kind:10050 DM-relay list (`TODO(nip17-dm-relays)` in the PR diff,
module docstring lines 100-110). Per NIP-17 the recipient envelope
SHOULD go to the recipient's kind:10050 inbox. As written, a DM sent
via PR #122 lands on the *sender's* relays — the recipient's client,
subscribed to its own kind:10050 inbox, will likely never see it.

This is fine to merge as labeled infrastructure, but it means **the
send path is not load-bearing until kind:10050 routing lands.** Do not
let "Phase 1 send works" (it signs + publishes, CI green) be mistaken
for "DMs are deliverable." They are not. The honest status after PR #122
merges: *DM send compiles and signs correctly; DMs are not yet
deliverable to arbitrary recipients.* kind:10050 resolution belongs in
the same phase cluster as `DmInboxProjection`, because the inbox screen
is what proves delivery actually closed the loop.

---

## 3. Pattern audit — last few PRs

### Positives (constraint satisfaction worth naming)

- **PR #122 honors ADR-0025.** ADR-0025 explicitly says "NIP-17 DMs must
  NOT copy [the Marmot bespoke-FFI] pattern" and "must be mediated by a
  new `ActorCommand::SendGiftWrappedDm`." PR #122 does exactly that —
  `ActorCommand` variant, no `nmp_app_chirp_dm_*` cluster. Good
  discipline; the ADR did its job as a forcing function.
- **PR #123 deleted `HttpCapability`.** Closes the review #34 concern
  about a synchronous-socket capability that would have violated D8.
  Net subtraction. Good.
- **PR #117 closed the action-result broken-promise on sign-step
  failures** — the terminal-failure-invisible gap flagged in review #24
  is now covered for the sign path.
- The `register_chirp_actions` + `register_nip29_actions`
  pre-`nmp_app_start` ordering rule (`apps/chirp/nmp-app-chirp/src/
  ffi.rs:147-165`) is correct and well-documented. Leave it alone.

### Concerns

- **`AddRemoteSigner` / `RemoveRemoteSigner` / `BunkerHandshakeProgress`
  are still `#[allow(dead_code)]`** (`actor/mod.rs:196-216`). The
  comments say "only test code instantiates it today." These have been
  dead across multiple reviews. They are the *exact* infrastructure
  `DmInboxProjection`'s bunker phase and ADR-0026 will need — which is
  another argument for doing the bunker DM story as one coherent unit
  rather than dribbling out inert seams. Either a broker PR wires
  `AddRemoteSigner` to a real handshake this cycle, or these three
  variants get a hard 2-cycle deletion deadline. Do not let them ossify.

- **`SendGiftWrappedDm` will join that dead-code list the moment PR #122
  merges**, until `nmp.dm.send` is registered. That is acceptable for
  exactly one cycle. If review #41 still sees `SendGiftWrappedDm` with
  no `nmp.dm.send` registration and no DM screen, that is a process
  failure, not a phasing choice.

- **Inert-surface census drift.** ADR-0025 says the `dispatch_action`
  namespace census "excludes Marmot op types by design." PR #122 adds a
  *new* category — `ActorCommand` variants that are reachable only from
  Rust, never from `dispatch_action` or FFI. `SendGiftWrappedDm` is one;
  the three broker variants above are others. The census Opus reviews
  run on `dispatch_action` registrations no longer captures the whole
  inert surface. Future reviews should also grep `#[allow(dead_code)]`
  on `ActorCommand` variants and count `ActorCommand` arms with no
  non-test caller. Flagging so review #41 has the right lens.

- **Docstring drift, minor:** `tap.rs:51-58` has two consecutive
  `/// Kinds the inbound tap subscribes to:` doc paragraphs on the same
  `TAP_KINDS` const — the first (lines 51-54) looks like a stale draft
  left above the rewrite (lines 55-58). Harmless but sloppy; fold into
  one. Worth a one-line cleanup next time `tap.rs` is touched, not a
  dedicated PR.

---

## 4. Recommended sequence

1. **Merge PR #122** (send infrastructure) and **PR #124** (clippy
   one-liner) once CI is green. PR #122's "not deliverable yet" status
   must be stated in its merge commit / changelog, not buried.
2. **Re-scope or pause ADR-0026.** Preferred: pause until step 3 exists.
   Acceptable: scope to `nip44_decrypt` only.
3. **Next feature PR: `DmInboxProjection`** — raw kind:1059 observer +
   local-key NIP-44 unseal → kind:14 rumor → `register_snapshot_
   projection`. Coexists with Marmot's tap (Section 1 proves this).
4. **Same cycle: kind:10050 DM-relay resolution** for the send path, so
   the loop actually closes, plus a **minimal Swift DM screen** consuming
   the inbox projection. The merged result of steps 3-4 is the first
   thing a human can open and verify.
5. **Then** bunker DM support: wire `RemoteSignerHandle` nip44 (the
   re-scoped ADR-0026) on both send and receive, with the broker
   `AddRemoteSigner` path no longer dead.

The through-line: **no merged seam without a consumer in the same
cycle.** PR #122 gets a one-cycle grace because the consumer
(`DmInboxProjection` + screen) is the explicitly-named next step. ADR-0026
does not get that grace because its consumer is two phases out.

---

## Appendix — files read

- `crates/nmp-core/src/remote_signer.rs` (47 LOC) — `RemoteSignerHandle`;
  `sign` + `deliver_rpc_response` only, no nip44.
- `crates/nmp-signers/src/signers/traits.rs` — `Nip44` trait def
  (lines 41-46); `Signer::nip44()` namespace.
- `crates/nmp-signers/src/signers/{local,nip07,nip46}.rs` — all three
  have working `Nip44` impls.
- `crates/nmp-core/src/kernel/event_observer.rs`,
  `actor/commands/event_observer.rs` — N-way fan-out confirmed.
- `crates/nmp-core/src/kernel/raw_event_observer.rs`,
  `actor/commands/raw_event_observer.rs` — N-way fan-out confirmed.
- `crates/nmp-marmot/src/{interest,projection/tap,projection/state}.rs` —
  Marmot uses the raw tap for kind:1059; `KernelEventObserver` early-
  returns for it.
- `crates/nmp-core/src/actor/mod.rs:130+` — `ActorCommand` enum; three
  `#[allow(dead_code)]` broker variants.
- `apps/chirp/nmp-app-chirp/src/ffi.rs` — registration ordering.
- `docs/decisions/0025-marmot-bespoke-ffi-cluster.md`.
- PR #122 diff (`gh pr diff 122`) — `dm.rs` relay-routing TODO.
