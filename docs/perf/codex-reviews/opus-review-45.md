# Opus direction review #45 — NIP-17 phase, ADR-0026 inert, structural gaps

Date: 2026-05-21. PRs covered: #120–#143.

## Q1 — Highest-ROI next PR: publish own kind:10050

- `ingest_dm_relay_list` exists; **no symmetric publish path**. No
  `ActorCommand::PublishDmRelayList`, no `nmp.dm.publish_relay_list` action.
  Without it, every Chirp user is invisible as a DM recipient — peers fall back
  to Content relays (the kernel's documented warn-path).
- Single small PR: `nmp_nip17::publish_dm_relay_list_command` →
  `ActorCommand::PublishUnsignedEventToRelays` (kind:10050, `relay` tags), auto-
  invoked from a `RelaySettingsStore`-driven UI + first sign-in defaults.
- Files: `crates/nmp-nip17/src/action.rs` (new `PublishDmRelaysAction`),
  `crates/nmp-nip17/src/lib.rs`, `apps/chirp/nmp-app-chirp/src/ffi.rs`
  (`register_nip17_actions`), iOS `Bridge/DmBridge.swift` +
  `Features/RelaySettingsView.swift`.
- Substrate value: a second NIP-17 action that proves `dispatch_action` carries
  a non-DM-send NIP-17 verb. User value: receive-side routing actually works.

## Q2 — Stop doing

- **ADR-0026 has zero consumers on either side.** `nip44_encrypt` and
  `nip44_decrypt` shipped (PR #125) but neither `send_gift_wrapped_dm` nor
  `DmInboxProjection::ingest_gift_wrap` calls them — both still take raw
  `&Keys`. This is exactly the "shipped-but-inert" pathology Opus #33/#34 named.
  Stop adding seams without their first consumer in the same PR.
- Stale prompt items: Swift `DmListView`/`DmConversationView`/`GroupChatView`
  **already exist**. The "no Swift DM/GroupChat screen" line in the spec list
  is wrong; the orchestrator should refresh the known-gaps register.
- 14 dormant NIP-29 ActionModule impls — 2-cycle deadline was set in #36; delete
  the 11 that won't ship in v1 (admin/membership/artifact/discussion/share).

## Q3 — Structural risk

**The substrate is consumer-starved by design discipline failure.** Inert ADR-
0026 + 14 dormant NIP-29 modules + `nmp.zap` no-op + `last_action_result` with
no iOS consumer means the seam count is now an unreliable signal of substrate
health. The thesis fails not from architecture but from a recurring "ship the
seam, defer the wiring" habit. Enforce a registration-requires-consumer rule in
the same PR — green CI on registered-but-inert code is the camouflage.

## Q4 — ADR-0026 assessment

Correct on theory (`make_seal` is the only step needing the sender's account
key), **wrong on operational claim "preserved on the grounds it works"**. It is
NOT wired: `DmInboxProjection::ingest_gift_wrap` (line 259) uses
`unwrap_gift_wrap(&keys, ...)` with raw keys; `send_gift_wrapped_dm` requires
`active_local_keys()`. To actually use it on send, a `PendingDmSend` state
machine is needed (two async `SignerOp`s: `nip44_encrypt` → `sign` of the seal,
then local ephemeral wrap). For receive, `unwrap_gift_wrap_with_signer` needs a
matching async refactor of `RawEventObserver` for bunker accounts. This is not
"one PR." Scope it as a milestone.

## Q5 — 36 ActorCommands gap

Acceptable. ~24 of 36 are infrastructure (Configure, Start, Stop,
LifecycleEvent, PushInterest, ClaimProfile, OpenAuthor, AddRemoteSigner,
BunkerHandshakeProgress, etc.) — they aren't user actions and don't belong on
`dispatch_action`. The honest user-verb ratio is closer to 6/12. Don't migrate
the rest; migrate `React`/`Follow`/`Unfollow` only because they ARE user verbs.

Highest-ROI: PR (a) Publish own kind:10050. Stop: registering inert seams.
