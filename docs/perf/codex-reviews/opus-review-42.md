# Opus Direction Review #42 — 2026-05-21

## What just shipped

PR #136 landed NIP-17 Phase 3: `DmInboxProjection` (a `RawEventObserver` taps
kind:1059 gift-wraps, unseals with the active account's local keys, projects a
per-peer conversation list under snapshot key `nip17.dm_inbox`), the
`nmp.dm.send` `ActionModule` (wired through the generic `dispatch_action` seam,
not a bespoke cluster — ADR-0025 respected), and Swift `DmListView` /
`DmConversationView` / `DmBridge`. Supporting PRs: #132 moved Chirp onboarding
relay policy into Rust, #134 deleted the dead `RemoveRemoteSigner` variant
(-141 LOC), #135 fixed D0 doc-noun leaks. Phase 3 is genuinely wired
end-to-end — the action is registered (`ffi.rs:561`), the inbox observer is
registered and the kind:1059 `#p` interest is pushed (`ffi.rs:338-361`), and
Swift dispatches `nmp.dm.send` and mirrors the snapshot. This is the cleanest
"shipped and live" feature in several reviews. **But Phase 3 closed only one
of the three #41 gaps, and the way it closed it converted a dormant TODO into
a live silent-correctness bug.**

## Inert surface inventory

This review splits inert surfaces into two classes. The second class is more
dangerous: it passes CI and looks live, but produces empty/wrong output for a
whole class of real users.

### Class A — fully inert (zero callers, camouflaged by tests only)

- **ADR-0026 signer NIP-44 seam — STILL fully inert one cycle later.**
  `RemoteSignerHandle::nip44_encrypt` / `nip44_decrypt`
  (`crates/nmp-core/src/remote_signer.rs:49,56`) have **zero non-test
  callers**. The only references are the trait definition, the
  `ArcRemoteSigner` forwarders (`broker.rs:377-381`), the `Nip46Signer` impl
  (`handle.rs:43,58`), and unit tests. ADR-0026 explicitly scheduled the
  `dm.rs` migration onto this seam as "Phase 3." Phase 3 shipped (#136) and
  did **not** do it. By the project's own 2-cycle-deadline doctrine
  (review #34), the seam has now been registered-but-inert for one full cycle;
  one more and it is deletable.

- **`DomainModule::run_migrations()` — does not exist; question is moot.**
  The `DomainModule` trait (`crates/nmp-core/src/substrate/domain.rs:1-26`)
  has `ingest_kinds`, `migrations`, `indexes` — **no `run_migrations`
  method**. The `run_migrations` that exists is `EventStore::run_migrations`
  (`store/events.rs:375`), implemented by both backends. Its only callers
  outside the store backends themselves are
  `crates/nmp-testing/tests/store_domain_migration.rs` — **test-only**. So
  the domain-migration *machinery* is built and backend-implemented but has
  zero production driver. Not a #1 risk, but it is dead weight: a 3-method
  trait family plus per-backend `domain.rs` files (mem + lmdb) with no app
  consuming them.

### Class B — silently inert for a whole user class (passes CI, looks live)

- **`DmInboxProjection` produces `{conversations: []}` forever for bunker
  (NIP-46) users.** `inbox.rs:233-241`: the projection requires the
  `nip17_local_keys` slot to be `Some`. Remote-signer accounts **never**
  populate that slot — `IdentityRuntime::active_local_keys` returns `None`
  for them by design (`identity.rs:183`), and `update_nip17_keys_slot` in
  `dispatch.rs` writes the slot from that same source. A bunker user signs
  in, opens the DM screen, and sees an empty inbox with no toast, no
  diagnostic, no "unsupported" placeholder. The unit tests all use local
  keys, so CI is green. This is the same camouflage pattern as the send-side
  gap — wired, snapshot-emitting, zero output for real users.

- **`nmp.dm.send` for bunker users — graceful toast, so at least visible.**
  The send path (`dm.rs:70-77`) detects the missing local key and surfaces a
  toast. This one is *correctly* handled (D6) and is NOT a Class B failure —
  but it means bunker users get a toast on send and silence on receive,
  an asymmetry worth fixing in the same cycle as the receive gap.

## Top 3 risks

### 1. kind:10050 silent mis-routing — now has a live consumer (CORRECTNESS)

`dm.rs:132` routes BOTH gift-wrap envelopes through
`kernel.bootstrap_urls_for_role(RelayRole::Content)` — the **sender's**
Content relays. NIP-17 requires each envelope to go to the **recipient's**
kind:10050 DM-relay list. There is **zero kind:10050 support anywhere in the
codebase** (`grep 10050` finds only the TODO and three doc comments). What
breaks, how, when:

- A user taps "send", `nmp.dm.send` dispatches, the actor publishes to the
  sender's own relays, **no toast, no error** — the send "succeeds."
- If the recipient's client reads from a different relay set (the normal
  case — that is the entire point of kind:10050), the DM is **never
  delivered**. Silent data loss on the primary messaging path.
- Pre-Phase 3 this was a TODO with no consumer. Phase 3 added the
  `nmp.dm.send` action that Swift dispatches — so the bug is now **live**.
- Secondary: publishing the kind:1059 envelope to the sender's known relay
  set partially defeats NIP-59 unlinkability — the outer ephemeral key
  becomes correlated with the sender's relay fingerprint.

### 2. Bunker users get an empty DM inbox forever (WHOLE-CLASS USER FAILURE)

See Class B above. `DmInboxProjection` requires `nip17_local_keys` to be
`Some`; remote-signer accounts never populate it. A bunker user's
`nip17.dm_inbox` projection is `{conversations: []}` permanently. No toast,
no placeholder, no log. CI is green because every inbox test uses
`Keys::generate()`. This is invisible until a real bunker user reports
"my DMs are empty" — and there is nothing in the snapshot to tell them why.

### 3. ADR-0026 seam dormancy hits the 2-cycle deadline (DEBT)

The `nip44_encrypt`/`nip44_decrypt` seam is fully inert (Class A). The
project's own doctrine (#34) gives registered-but-inert surfaces two cycles.
This is cycle one post-merge. The risk is not the seam itself — it is the
pattern: a seam was built "for Phase 3," Phase 3 shipped without consuming
it, and the next review will face the choice of deleting protocol
infrastructure or letting the dormant-surface census keep growing. Decide
now: either Phase 4 consumes it next cycle, or it gets cut.

## What to cut or freeze

- **FREEZE** any new "Phase N" NIP-17 PR that does not either (a) fix
  kind:10050 routing or (b) consume the ADR-0026 seam. Adding more DM surface
  on top of a broken delivery path compounds the camouflage.
- **DO NOT delete the ADR-0026 seam yet** — but put it on a hard one-cycle
  clock. If Phase 4 (bunker DM seal/unseal) does not consume `nip44_encrypt`
  + `nip44_decrypt` in the next cycle, delete both methods, the broker
  forwarders, and the `Nip46Signer` impl, and reopen them when bunker DMs
  are actually built.
- **Consider cutting `DomainModule` migration machinery** — the trait's
  `migrations()`/`indexes()`/`run_migrations` chain has zero production
  consumer and is implemented twice (mem + lmdb `domain.rs`). It is not
  load-bearing for any shipped feature. Not urgent, but it should not be
  cited as "infrastructure ready" — it is unproven scaffolding.
- **Fix `register_dm_inbox`'s leak-by-design** (`ffi.rs:310-315`): its own
  docstring admits "calling it twice registers a second event observer (a
  small, bounded leak)." `DmInboxStore.apply` re-invokes on every
  `activePubkey` change (`DmBridge.swift:152-161`). Every account switch
  leaks an observer. Either make the FFI replace the prior observer
  (preferred — give it a stable observer id keyed on the projection) or
  expose an `unregister`. A seam billed as ship-ready should not leak by
  design at its hottest re-invocation point.

## What to build next

**Single highest-ROI PR: a kind:10050 DM-relay resolver in `dm.rs`,
replacing the `bootstrap_urls_for_role(Content)` call (`dm.rs:132`).**

Why this and not Phase 4 bunker DMs: Phase 4 builds on the delivery path.
If that path is broken, Phase 4 ships bunker DMs that also do not deliver.
Fix the foundation first. This is also the smallest change with the largest
correctness payoff — it converts the primary messaging feature from
"silently loses messages" to "actually delivers."

Concrete changes:

1. **Add a kind:10050 ingest + lookup path.** kind:10050 is a NIP-17 DM-relay
   list (replaceable, like kind:10002). The store already enforces
   replaceable semantics for kind:10002; add 10050 to the same path. Provide
   a `kernel.dm_relays_for(pubkey: &str) -> Vec<String>` analogous to the
   existing kind:10002 resolver.

2. **Rewrite the routing in `send_gift_wrapped_dm` (`dm.rs:113-134`)** so each
   envelope resolves its own target:
   - Recipient envelope → recipient's kind:10050 list.
   - Self-copy envelope → the **sender's own** kind:10050 list.
   - Fallback chain when kind:10050 is absent: recipient's kind:10002
     **read** relays (the NIP-65 inbox role — semantically the right
     fallback, the recipient reads there), then the discovery seed. Today's
     code uses the sender's *Content* relays, which is wrong on both the
     "whose relays" and "which role" axes.

3. **Push a kind:10050 fetch interest for DM recipients.** When a DM
   conversation is opened (or a recipient pubkey is first seen), the kernel
   should fetch that pubkey's kind:10050 so the resolver is not always
   falling back. This mirrors the existing kind:10002 gossip path.

4. **For the bunker-empty-inbox bug (risk #2)** — out of scope for this PR
   but file it: until Phase 4 wires `nip44_decrypt` into the unwrap path,
   `register_dm_inbox` should detect a remote-signer active account and emit
   a one-line `nip17.dm_inbox` projection field (e.g.
   `{"conversations": [], "unsupported": "bunker accounts cannot yet
   decrypt DMs"}`) so the Swift screen can render an honest placeholder
   instead of a blank list. D1: a placeholder is part of the type contract.

## Structural health snapshot

- **`unwrap()`/`expect()` at public boundaries in `nmp-nip17`: zero in
  non-test code.** All 8 hits (`action.rs:157,188`, `inbox.rs:403,411,520,
  556,701,703`) are inside `#[cfg(test)]` modules. The production paths use
  `let-else` / `?` / `unwrap_or_else` throughout — `inbox.rs` is exemplary
  D6 (every failure is a documented silent no-op). Clean.

- **D0 violations introduced in Phase 3: none.** `nmp.dm.send` routes
  through the generic `dispatch_action` seam (not a bespoke
  `nmp_app_chirp_dm_*` cluster) — ADR-0025's explicit "NIP-17 must NOT copy
  the Marmot pattern" constraint is honored. `dm.rs:18-20` explicitly
  refuses to read `marmot_local_nsec`, using the actor's own identity state
  instead — ADR-0026's constraint honored. The `nip17_local_keys` slot is
  correctly distinct from `marmot_local_nsec`. **No double-keying bug** —
  the two slots hold different types (bech32 nsec string vs parsed
  `nostr::Keys`) for different consumers — but they ARE co-maintained (six
  `update_nip17_keys_slot` sites in `dispatch.rs` mirror the marmot slot's
  six). Flag: the next protocol crate that needs key access must NOT add a
  third slot; design one typed key-access seam before that happens.

- **Chirp Swift LOC: 9,944** (`ios/Chirp/Chirp/*.swift`) — ~33× the 300 LOC
  thin-shell budget, and up from prior reviews. `DmBridge.swift` (178),
  `DmListView.swift` (229), `DmConversationView.swift` (156) are
  individually fine and genuinely thin (the DM bridge has zero protocol
  logic — verified). But the aggregate "Chirp is a thin shell" claim is
  structurally false and getting worse every cycle. This is a standing
  finding, not a Phase 3 regression.

- **Action registration status: 5 live `dispatch_action` namespaces.**
  `nip29.post_chat_message`, `nip29.react_in_group`,
  `nip29.comment_in_group`, `chirp.*` (react/follow/unfollow), and now
  `nmp.dm.send`. `nmp.dm.send` is genuinely live: registered (`ffi.rs:561`),
  dispatched by Swift (`DmBridge.swift:94`), reaches the actor via
  `ActorCommand::SendGiftWrappedDm` (`dispatch.rs:363`). The action layer
  is healthier than it has been — the live:inert ratio for *actions* is
  good. The inert surfaces are seams (ADR-0026) and projections-for-a-class
  (bunker inbox), not actions.

---

**Bottom line:** Phase 3's wiring is the cleanest in many reviews — but
"wired" is not "correct." The DM send path silently drops messages because
it routes to the wrong relays, and the DM receive path silently returns
nothing for bunker users. Both pass CI. Fix kind:10050 routing before
building Phase 4 on top of it.
