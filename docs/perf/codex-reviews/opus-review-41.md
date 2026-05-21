# Opus Direction Review #41

Date: 2026-05-21
Reviewer: Opus (direction review #41)
Scope: post-PR-#122/#125/#128/#129 audit, NIP-17 receive path, ADR-0026
inert-seam verdict, ActionPlan / ViewModule / IdentityModule status
correction.

## TL;DR

1. **ADR-0026 shipped fully inert — this is the #1 finding.** PR #125
   merged `nip44_encrypt` / `nip44_decrypt` onto `RemoteSignerHandle`,
   the NIP-46 handle implements both (`nip46/handle.rs:43-58`), tests
   cover them. **Zero non-test consumers.** `dm.rs` still calls
   `nmp_nip59::gift_wrap` with raw `Keys`. Review #40 explicitly said
   "pause or re-scope ADR-0026 — do not merge a symmetric two-method
   extension with zero consumers." It was merged anyway, symmetric, with
   zero consumers. The predicted failure landed in the same cycle.
2. **Three task-brief claims are stale — flagged so they do not
   propagate.** `ActionPlan` is already deleted (`substrate/action.rs:18-25`
   says so). `ViewModule` and `IdentityModule` traits are already deleted
   (`substrate/view.rs:88-97`, `substrate/identity.rs:3-9`). No
   `DmInboxProjection` branch exists — `git branch -a` / `-r` show
   nothing; the receive side has not started.
3. **DM send path is signed-but-undeliverable.** `dm.rs:129-133` still
   routes both kind:1059 envelopes to the sender's Content relays, not
   the recipient's kind:10050 inbox. A DM sent today will not reach an
   arbitrary recipient. Unchanged since review #40.
4. **Next move: one PR cluster — `DmInboxProjection` + `dm.rs` nip44
   migration + kind:10050 routing.** These three close the DM loop *and*
   retire the ADR-0026 inert violation. If only one PR: `dm.rs` nip44
   migration, because it converts a merged-but-inert seam into a live one
   and is the smaller blast radius. See §3.

---

## 1. ADR-0026 — the inert-seam violation, in detail

**What shipped (PR #125).** `RemoteSignerHandle` gained two methods
(`remote_signer.rs:49,56`). `Nip46Signer`'s handle implements them by
enqueueing `nip44_encrypt` / `nip44_decrypt` RPCs to the bunker
(`nip46/handle.rs:43-58`). `remote_signer_tests.rs` proves the RPC
round-trips. ADR-0026 status: "Implemented."

**What consumes it.** Nothing. `grep nip44_encrypt` across non-test code
outside `nmp-signers`: two hits, both **stale comments** that assert the
seam does *not* exist —
- `dm.rs:8`: "`RemoteSignerHandle` ... exposes only `sign()` — there is
  no `nip44_encrypt` / `nip44_decrypt` seam".
- `actor/mod.rs:334`: "A remote (NIP-46) signer exposes only `sign()` —
  no `nip44_encrypt` ... Bunker support is gated on ADR-0026 ... which is
  not yet built."

Both are now factually wrong. A reader following them to the seam will
be misled. This is not docstring drift — it is a live correctness defect
in the module that is supposed to consume the seam.

**Verdict.** ADR-0026 is the "shipped-but-inert, camouflaged by green
CI" pattern reviews #33–#40 named the #1 project risk. Two consecutive
reviews predicting the same pathology and watching it land is signal,
not noise. The fix is not to delete the seam — `DmInboxProjection` and
`dm.rs` both genuinely need it. The fix is to **wire a consumer in the
very next cycle and not let a third review find it still inert.** Hard
deadline: review #42 must find `dm.rs` calling `nip44_encrypt`, or the
ADR-0026 methods get the same 2-cycle deletion treatment as
`RemoveRemoteSigner`.

## 2. Stale-brief corrections (do not propagate these)

- **ActionPlan: already deleted.** `substrate/action.rs:18-25` —
  `ActionModule::start` is now a pure `Result<(), ActionRejection>`
  validator; the docstring states the `ActionPlan` return type "has been
  removed." `nip57/action.rs:78` confirms ("no `ActionPlan`, no
  `type Step` — both were [removed]"). No "delete ActionPlan"
  recommendation is needed. Done.
- **ViewModule / IdentityModule traits: already deleted.**
  `substrate/view.rs:88-97` and `substrate/identity.rs:3-9` both carry
  explicit "there was once a … trait here … It has been removed" notes.
  They are not inert surfaces; they are not surfaces. `ViewDependencies`
  (the planner bridge) correctly survives.
- **DmInboxProjection: not in-flight.** No branch, local or remote. The
  receive side has not started. Reviews must not assume it.
- **DomainModule:** real per-NIP impls exist, no kernel registry fans to
  them — static dispatch in tests only. Unchanged. Lowest-priority
  inert item; leave it (the impls are cheap and the trait documents the
  per-NIP contract). Not worth a deletion PR this cycle.

## 3. NIP-17 — the honest state and the next cluster

**Shipped:** `nmp-nip17` rumor builder (pure, no keys — clean);
`ActorCommand::SendGiftWrappedDm` + `dm.rs` handler (local-keys send,
gift-wraps twice, publishes signed envelopes verbatim — correct crypto).

**Missing for a usable feature, in dependency order:**
1. **Receive side (`DmInboxProjection`).** A `RawEventObserver` on
   kind:1059 — it needs the verbatim `sig` to `unwrap_gift_wrap`, so the
   raw tap (not the lossy `KernelEventObserver`) is the right seam.
   Review #40 §1 already proved this coexists cleanly with Marmot's
   `MarmotIngestTap` (both raw observers, N-way fan-out, each silently
   discards the other's traffic). That analysis still holds.
2. **`dm.rs` nip44 migration.** Replace the `nmp_nip59::gift_wrap(keys,
   …)` raw-key call with the actor reaching the active signer through
   `RemoteSignerHandle::nip44_encrypt` for the kind:13 seal. This both
   extends DM send to bunker accounts *and* retires the ADR-0026 inert
   violation. The outer kind:1059 wrap stays actor-local (fresh
   ephemeral key) — only the seal needs the signer.
3. **kind:10050 routing.** `dm.rs:129` TODO: resolve the recipient's
   kind:10050 inbox-relay list and pin the recipient envelope there;
   the self-copy goes to the sender's own kind:10050. Until this lands,
   "send works" means "signs + publishes to the wrong relays."

**Snapshot shape for the Swift screen.** When `DmInboxProjection` is
built: project **one global inbox snapshot**, not one-per-contact. A DM
list screen needs the set of conversations ordered by latest activity;
per-contact snapshots force the host to enumerate contacts it does not
yet know. Minimum shape:

```
DmInboxSnapshot {
  threads: [DmThread]   // sorted by last_activity desc
}
DmThread {
  counterparty_pubkey: String   // hex; the other party
  last_message_preview: String  // truncated kind:14 content
  last_activity: u64            // newest rumor created_at
  unread_count: u32             // host may ignore in v1; 0 is fine
}
```

A per-thread message list belongs behind an `OpenAuthor`-style command
(`OpenDmThread { counterparty }`) projecting a separate
`DmThreadSnapshot { messages: [DmMessage] }` — keep the inbox snapshot
small and let the detail screen pull the full thread. Do NOT put every
message of every thread into the global snapshot; it grows unbounded.

**Phase 2 (bunker DM) "shipped-but-inert" risk.** Real. The minimum
viable test that proves the bunker path: a `dm.rs` unit test with a mock
`RemoteSignerHandle` whose `nip44_encrypt` returns a fixed ciphertext —
assert the seal is built from the seam's output, not from a local key,
and that a bunker-only identity (no `active_local_keys`) now produces
envelopes instead of a toast. That test is the forcing function; without
it Phase 2 will look done while the bunker branch is never exercised.

## 4. Other doctrine / inert-surface notes

- **`RemoveRemoteSigner` — deadline is review #42.** `actor/mod.rs:213`
  carries `TODO(2-cycle-deadline): if review #42 still finds no
  production caller, delete`. This is review #41. Restating, not
  restarting: next review either finds a sign-out flow constructing it,
  or it and its `dispatch.rs:262` arm get deleted. `AddRemoteSigner` and
  `BunkerHandshakeProgress` have live broker callers
  (`broker.rs:329`) — their `#[allow(dead_code)]` is a correct
  per-crate-lint suppression, leave them.
- **`marmot_local_nsec` exception — still justified.** It supersedes
  only once `dm.rs` reaches keys through the actor's signer seam. `dm.rs`
  has not migrated (§3 item 2), so the actor-identity pattern that would
  replace the `Arc<Mutex<Option<Zeroizing<String>>>>` slot does not yet
  exist. Migrate Marmot to it *after* the `dm.rs` nip44 migration proves
  the pattern — not before. ADR-0025 stays valid this cycle.
- **D7 — clean.** `dm.rs:82-84`, `dispatch.rs:312/329` all re-stamp
  `created_at == 0` from `kernel.now_secs()`. Good.
- **D6 — clean on the DM path.** `send_gift_wrapped_dm` surfaces every
  failure (no local key, malformed pubkey, gift-wrap error) as a toast,
  publishes nothing, never panics.
- **D8 — watch ADR-0024.** Still "Proposed." NIP-57 zaps remain blocked
  on the async-capability protocol; nothing changed. The two-phase
  design (`nmp_app_deliver_capability_result` re-entry) is the right
  shape — but it is an ADR, not code. NIP-57 is correctly *not* the next
  feature; do not start it until ADR-0024 is implemented.
- **live:inert ratio.** ~7 live `dispatch_action` namespaces. Inert
  surfaces that actually remain: ADR-0026's two trait methods (the real
  problem), `RemoveRemoteSigner` (deadline #42), `DomainModule` static
  dispatch (benign). The census lens review #40 asked for —
  `#[allow(dead_code)]` on `ActorCommand` variants — is the right one;
  `SendGiftWrappedDm` is correctly NOT dead-code-flagged because it has
  no FFI registration yet but is on a one-cycle grace as the named-next
  consumer target.

## 5. Recommended next PR

**The DM-loop cluster, in one mergeable unit:** `DmInboxProjection`
(raw kind:1059 observer → global `DmInboxSnapshot` via
`register_snapshot_projection`) **+** `dm.rs` migrated to
`RemoteSignerHandle::nip44_encrypt` for the seal **+** kind:10050
recipient-relay routing **+** a minimal Swift DM list screen. The
merged result is the first DM behavior a human can open and verify, and
it retires the ADR-0026 inert violation in the same cycle.

**If forced to a single smaller PR:** `dm.rs` nip44 migration. It is the
smaller blast radius, it converts a merged-but-inert seam into a live
one (closing the #1 finding), and it unblocks bunker DM send. Tradeoff:
it produces no user-visible behavior on its own — send is still
undeliverable without kind:10050 routing and unverifiable without the
receive side. So it is the right *first* PR only if the receive side
and routing follow immediately; otherwise lead with `DmInboxProjection`
so something is visible. Either way, the non-negotiable: **review #42
must not find `nip44_encrypt` still inert.**

---

## Appendix — files read

- `crates/nmp-core/src/remote_signer.rs` — nip44 seam present, 0 consumers.
- `crates/nmp-core/src/actor/commands/dm.rs` — raw-key gift_wrap; stale
  "no nip44 seam" comment; kind:10050 TODO.
- `crates/nmp-core/src/actor/dispatch.rs` — `SendGiftWrappedDm` arm;
  D7 re-stamps.
- `crates/nmp-core/src/actor/mod.rs` — `ActorCommand` enum; stale
  ADR-0026 comment at :334; `RemoveRemoteSigner` deadline at :213.
- `crates/nmp-core/src/substrate/{action,view,identity,mod}.rs` —
  ActionPlan / ViewModule / IdentityModule all confirmed deleted.
- `crates/nmp-core/src/ffi/action.rs` — dispatch path; result observer.
- `crates/nmp-nip17/src/lib.rs` — pure rumor builder, clean.
- `crates/nmp-nip59/src/wrap.rs` — `gift_wrap` local-keys primitive.
- `crates/nmp-marmot/src/projection/tap.rs` — `RawEventObserver` pattern.
- `crates/nmp-signers/src/signers/nip46/handle.rs` — nip44 RPC impl.
- `apps/chirp/nmp-app-chirp/src/ffi.rs` — action registration.
- `docs/decisions/0024-async-capability-protocol.md` (Proposed),
  `0026-signer-nip44-seal-seam.md` (Implemented).
- `docs/perf/codex-reviews/opus-review-40.md`.
- `git branch -a` / `-r`, `gh pr list` — no DmInboxProjection branch.
