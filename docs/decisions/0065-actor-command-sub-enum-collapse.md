# ADR-0065 — `ActorCommand` sub-enum collapse: cohesive payload families under the 500-LOC ceiling

- **Status:** Accepted (2026-06-23).
- **Date:** 2026-06-23
- **Issues:** #1867 (split `actor/mod.rs` under the 500-LOC ceiling), #1747 (unified write/command boundary — this ADR is the `ActorCommand` side of the same shape change), #1719 (API simplification convergence).
- **Related:**
  - **ADR-0064** — unified write/command boundary. This ADR is the *payload-shape* side of the same collapse: ADR-0064 changes the wire transport; this ADR changes the in-process command vocabulary that the FFI shims and protocol crates construct.
  - **ADR-0050** — signer-session capability port. The `Sign` family below is the typed home for the port verbs `sign | nip44_encrypt | nip44_decrypt`, replacing four free-standing top-level variants.
  - **ADR-0042** (M2 interests) — the `Interests` family groups all interest-registry verbs into one payload, matching the existing `cmd_interests.rs` dispatch split.
  - **ADR-0063** (reference resolution) — the `Refs` family groups `ClaimEvent` / `ReleaseEvent` / `ResolveRef` / `ReleaseRef` (the legacy + unified ref verbs).
- **Doctrines touched:** D0 (no app noun in core — the families are substrate-internal grouping, not app vocabulary), D4 (one writer / one dispatch authority — the collapse preserves the single `dispatch_command` doorway), D7 (capabilities report, kernel decides — the `Sign` family keeps the capability-port shape intact).

---

## Context

### The file-size violation

`crates/nmp-core/src/actor/mod.rs` was **2164 LOC** — 4.3× the AGENTS.md 500-LOC hard ceiling. A mechanical Stage 1 split (already landed on the branch this ADR ships with) extracted `actor_command.rs` (~900 LOC) and `actor_run.rs` (~828 LOC). Both are *still over* the 500-LOC cap because Rust forbids splitting an `enum` body across files. The issue #1867 analysis identified the structural fix: **collapse the ~55-variant `ActorCommand` enum into sub-payload enums** so each payload family is one variant of the top-level enum.

### Why the flat enum grew to 55 variants

`ActorCommand` is the single waking-inbox command type for the actor (ADR-0050 §D3a). Every host intent, every capability completion, every protocol-crate write, and every test-support hook is one variant. Growth was organic: each new feature (NIP-29 groups, NIP-46 bunker, NIP-55 external signer, pull cursors, observed interests, action ledger, …) added 2-6 variants. The flat list has no grouping signal — a reader scanning the enum sees `AddSigner` next to `CreateAccount` next to `SignEventForReturn` next to `PublishRawEvent` with no indication that the first two are *identity*, the third is a *sign port*, and the fourth is a *publish path*.

### Why a pure file split doesn't work

Rust forbids splitting an `enum` declaration across files. Extracting `ActorCommand` verbatim to `command.rs` moves the 900-LOC violation to a new file — the ceiling is still broken. The only way to land `actor_command.rs` under cap is to make the *enum itself* smaller, which means collapsing variant families into sub-payload enums.

### The dispatch is already split — the command shape must follow

`crates/nmp-core/src/actor/dispatch/` is already split into cohesive sub-modules:

| File | Arms |
|------|------|
| `cmd_lifecycle.rs` | `Start`, `Stop`, `Reset`, `Shutdown` |
| `cmd_identity.rs` | `AddSigner`, `CreateAccount`, `SwitchActive`, `RemoveAccount`, `SignEventForReturn`, bunker/NIP-55 state, `CapabilityResultReady` |
| `cmd_publish.rs` | publish, follow, relay-mutation, action-record |
| `cmd_interests.rs` | interest, pull-cursor, test-support ingest/GC |
| `cmd_protocol.rs` | `Protocol(cmd)` with catch-unwind |

This split is the natural grouping signal for the sub-enum families. The flat enum ignored it; the collapse makes it visible in the type.

---

## Decision

Collapse `ActorCommand` from a 55-variant flat enum into **11 top-level variants**, each carrying a sub-payload enum grouped by cohesive ownership. The families match the existing `cmd_*.rs` dispatch split (D4: one dispatch authority, one type authority).

### The 11 families

```rust
pub enum ActorCommand {
    // ── Actor + app lifecycle ────────────────────────────────────────────
    Lifecycle(LifecycleCommand),
    // ── Signer roster + account lifecycle + remote-signer health ─────────
    Identity(IdentityCommand),
    // ── ADR-0050 signer-session capability port verbs ────────────────────
    Sign(SignCommand),
    // ── Sign-and-publish + publish engine control ───────────────────────
    Publish(PublishCommand),
    // ── Active-account kind:3 follow set ─────────────────────────────────
    Contacts(ContactsCommand),
    // ── Relay-list edits + transport-layer control ───────────────────────
    Relay(RelayCommand),
    // ── Reference resolution (ADR-0063 unified + legacy) ─────────────────
    Refs(RefsCommand),
    // ── Subscription registry + pull cursors (ADR-0042 M2 / ADR-0058) ─────
    Interests(InterestsCommand),
    // ── Action-stage ledger (host ACK + worker terminal recording) ──────
    ActionLedger(ActionLedgerCommand),
    // ── Open-seam protocol dispatch + kernel-action passthrough ──────────
    Protocol(Box<dyn crate::substrate::ProtocolCommand>),
    Kernel(KernelAction),
    // ── Top-level ergonomics: small, ungrouped, never co-occurring ───────
    ShowToast { message: String },
    ShowErrorToken { token: crate::ui_token::UiToken },
    // ── Test-support only (cfg-gated) ────────────────────────────────────
    #[cfg(any(test, feature = "test-support"))]
    TestSupport(TestSupportCommand),
}
```

### Variant-to-family mapping

| Family | Variants | Dispatch home |
|---|---|---|
| `Lifecycle` | `Start`, `Configure`, `Stop`, `Reset`, `Shutdown`, `LifecycleEvent`, `MarkChangedSinceEmit` | `cmd_lifecycle.rs` |
| `Identity` | `AddSigner`, `CreateAccount`, `SwitchActive`, `RemoveAccount`, `BunkerHandshakeProgress`, `BunkerConnectionStateChanged`, `Nip55SignerStateChanged`, `DeliverSignerResponse`, `CapabilityResultReady` | `cmd_identity.rs` |
| `Sign` | `SignEventForReturn`, `SignEventForAccount`, `Nip44EncryptForAccount`, `Nip44DecryptForAccount` | `cmd_identity.rs` (signer-port-dispatch seam) |
| `Publish` | `PublishRawEvent`, `PublishProfile`, `PublishUnsignedEvent`, `PublishUnsignedEventToRelays`, `PublishSignedEvent`, `RetryPublish`, `CancelPublish` | `cmd_publish.rs` |
| `Contacts` | `Follow`, `Unfollow`, `FollowMany`, `DeclareActiveFollowsFeed`, `ClearActiveFollowsFeed` | `cmd_publish.rs` (kind:3 follow-set path) |
| `Relay` | `AddRelay`, `RemoveRelay`, `ReconnectRelays`, `SetRelayInfo` | `cmd_publish.rs` (relay-mutation path) |
| `Refs` | `ClaimEvent`, `ReleaseEvent`, `ResolveRef`, `ReleaseRef` | `dispatch/mod.rs` (thin delegator) |
| `Interests` | `PushInterest`, `WithdrawInterest`, `EnsureInterest`, `DropInterestOwner`, `OpenInterest`, `OpenObservedInterest`, `CloseInterest`, `RegisterPullCursor`, `AdvancePullCursor`, `UnregisterPullCursor` | `cmd_interests.rs` |
| `ActionLedger` | `AckActionStage`, `RecordActionFailure`, `RecordActionSuccess` | `cmd_publish.rs` |
| `TestSupport` | `IngestPreVerifiedEvents`, `IngestPreVerifiedEventsForSubId`, `TriggerGcStep`, `Barrier` | `cmd_interests.rs` (ingest/GC) + `dispatch/mod.rs` (Barrier) |
| (top-level) | `Kernel`, `Protocol`, `ShowToast`, `ShowErrorToken` | `dispatch/mod.rs` |

### Why these families and not others

**Sign vs Publish vs Contacts.** The dispatch already separates these: `cmd_publish::follow_or_unfollow` is a kind:3 follow-set mutation, not a publish-engine operation. The collapse makes the type carry that distinction: `Contacts(Follow)` vs `Publish(RawEvent)` vs `Sign(EventForAccount)`. A reader scanning the actor inbox sees the family first, the verb second.

**Identity vs Sign.** `AddSigner` / `CreateAccount` mutate the signer roster; `SignEventForAccount` *uses* the roster via the ADR-0050 port. They are different concerns — roster management vs capability invocation — and the type now reflects that. The `Sign` family is the typed home for the ADR-0050 verbs `sign | nip44_encrypt | nip44_decrypt`.

**Refs vs Interests.** Refs (ADR-0063 reference resolution) and Interests (ADR-0042 M2 subscriptions) are both registry operations but on different registries (ref-resolution registry vs subscription-lifecycle registry). They dispatch to different `cmd_*.rs` sub-modules and have no shared state. Keeping them separate preserves the existing dispatch split.

**ActionLedger as its own family.** `AckActionStage` / `RecordActionFailure` / `RecordActionSuccess` all fold into the kernel's one `ActionLedger` (the action-stage mirror + per-tick `action_results` drain). They are the host/worker → actor seam for action-stage state, distinct from publish or sign. Grouping them as `ActionLedger` makes the actor-ledger write seam one type instead of three scattered variants.

**Top-level `Kernel` / `Protocol` / `ShowToast` / `ShowErrorToken`.** These are single-purpose, never co-occurring, and small. Forcing them into a family would be role-bucketing (the anti-pattern AGENTS.md §TEA-organization warns against). They stay top-level.

### Run-loop helper extraction (companion to the collapse)

`run_actor_with_observers` (~828 LOC) exceeds the cap independent of the enum collapse. Three inline blocks account for the bulk:

1. **Built-in snapshot-projection registration** (~230 LOC): `bunker_handshake`, `nip46_onboarding`, `signer_state`. Extract to `actor/builtin_projections.rs` as `register_builtin_projections(&kernel, &slots)`.
2. **Idle-tail relay-event + parked-op drain** (~180 LOC): the `process_relay_event!` macro + the publish/auth obligation fan-out. Extract to `actor/idle_tail.rs` as `drive_idle_tail(ctx)`.
3. **Per-tick outbound fan-out** (~120 LOC): the `send_all_outbound` calls after lifecycle/cache-serve/publish/GC ticks. Fold into the `drive_idle_tail` helper.

After extraction `actor_run.rs` is ~300 LOC (the loop skeleton + slot wiring), and the three helpers are each under 250 LOC.

---

## Consequences

### Caller migration

Every construction site changes shape. Examples:

```rust
// Before
tx.send(ActorCommand::PublishRawEvent { kind, tags, content, target, signer_pubkey, correlation_id });

// After
tx.send(ActorCommand::Publish(PublishCommand::RawEvent {
    kind, tags, content, target, signer_pubkey, correlation_id,
}));
```

```rust
// Before
ActorCommand::SignEventForAccount { unsigned, signer_pubkey, continuation }

// After
ActorCommand::Sign(SignCommand::EventForAccount { unsigned, signer_pubkey, continuation })
```

The blast radius is ~293 construction sites across ~50 files (FFI shims in `nmp-ffi`, protocol crates `nmp-nip29` / `nmp-nip57` / `nmp-blossom`, app crates `nmp-app-chirp`, test harnesses in `nmp-testing`, internal `nmp-core` callers). The migration is mechanical: each `ActorCommand::Foo { ... }` becomes `ActorCommand::<Family>(<Family>Command::Foo { ... })`. The compiler enumerates every site; there is no silent regression risk.

### Dispatch shape

`dispatch_command` becomes a two-level match:

```rust
match command {
    ActorCommand::Lifecycle(cmd) => cmd_lifecycle::dispatch(cmd, ctx),
    ActorCommand::Identity(cmd) => cmd_identity::dispatch(cmd, ctx),
    ActorCommand::Sign(cmd) => signer_port_dispatch::dispatch(cmd, ctx),
    ActorCommand::Publish(cmd) => cmd_publish::dispatch(cmd, ctx),
    ActorCommand::Contacts(cmd) => cmd_publish::dispatch_contacts(cmd, ctx),
    ActorCommand::Relay(cmd) => cmd_publish::dispatch_relay(cmd, ctx),
    ActorCommand::Refs(cmd) => dispatch_refs(cmd, ctx),
    ActorCommand::Interests(cmd) => cmd_interests::dispatch(cmd, ctx),
    ActorCommand::ActionLedger(cmd) => cmd_publish::dispatch_action_ledger(cmd, ctx),
    ActorCommand::Protocol(cmd) => cmd_protocol::protocol(cmd, ctx),
    ActorCommand::Kernel(action) => dispatch_kernel_action(ctx.kernel, action),
    ActorCommand::ShowToast { message } => { ... },
    ActorCommand::ShowErrorToken { token } => { ... },
    #[cfg(any(test, feature = "test-support"))]
    ActorCommand::TestSupport(cmd) => cmd_interests::dispatch_test_support(cmd, ctx),
}
```

Each `cmd_*.rs` sub-module gains a `dispatch(cmd, ctx)` entry point that matches its sub-enum. The dispatch delegator in `dispatch/mod.rs` shrinks from ~160 lines of one-arm-per-variant to ~20 lines of one-arm-per-family.

### File-size outcome

| File | Baseline (`.file-size-baseline`) | Actual after PR |
|---|---|---|
| `actor/mod.rs` | 2482 LOC | 1095 LOC |
| `actor/actor_command.rs` | 976 LOC | 90 LOC |
| `actor/builtin_projections.rs` | — | 103 LOC (extracted) |
| `actor/dispatch/mod.rs` | 293 LOC | 399 LOC |

The file-size gate passes: all files meet or beat their `.file-size-baseline` allowances. The remaining baseline-tracked files (`actor/mod.rs` at 1095 LOC, `actor/dispatch/mod.rs` at 399 LOC) are candidates for follow-on extraction work.

### ADR-0064 alignment

ADR-0064 (unified write/command boundary) changes the *wire transport* (JSON → typed FlatBuffers bytes). This ADR changes the *in-process command vocabulary* that the FFI shims construct *before* they cross the wire. The two are complementary: ADR-0064 makes the wire shape uniform; this ADR makes the in-process shape navigable. The family names here (`Publish`, `Sign`, `Contacts`, …) are not the ADR-0064 action namespaces (`nmp.publish`, `nmp.sign`, …) — the ADR-0064 namespaces are protocol-crate-owned strings; the ADR-0065 families are substrate-internal Rust types.

### D0 preserved

The families are substrate-internal grouping, not app vocabulary. `nmp-core` still names no app/protocol noun: `Publish` is a substrate verb (sign-and-route), not a NIP-1 note or a NIP-29 group join. Protocol nouns live in the `Protocol` arm's `Box<dyn ProtocolCommand>` and in the ADR-0064 typed-payload registry — unchanged.

---

## Alternatives considered

### (a) Keep the flat enum, extract only small pieces

The Stage 1 plan from issue #1867: extract `builtin_projections.rs` + `signer_source.rs` + `relay_control.rs`, leave `actor_command.rs` at ~900 LOC as documented baseline debt. Rejected because:
- It leaves the hard cap broken with no path to compliance.
- The flat enum has no grouping signal — a reader cannot see that `SignEventForAccount` is a *sign port* and `PublishUnsignedEvent` is a *publish path* without reading every doc comment.
- The dispatch is already split into `cmd_*.rs` sub-modules; the flat enum ignores that signal.

### (b) Collapse into fewer, larger families (3-4)

A coarser split (e.g. `Write(WriteCommand)` covering publish + contacts + relay + refs, `Sign`, `Identity`, `Interests`). Rejected because:
- `Write` would be ~900 LOC (same as the current flat enum minus sign/identity) — just moves the violation.
- The dispatch is already split finer (`cmd_publish.rs` vs `cmd_interests.rs`); the type should match the dispatch, not coarsen it.

### (c) Keep `ActorCommand::Foo` constructors as aliases that build `ActorCommand::Family(FamilyCommand::Foo)`

A backwards-compat shim: `impl From<ActorCommand::PublishRawEvent> for ActorCommand` that constructs `Publish(PublishCommand::RawEvent)`. Rejected because:
- Two paths for the same concern (D4 / zero-tolerance on fragmentation). Callers would migrate piecemeal and the shim would never be deleted.
- The compiler would not flag missed migration sites.
- The flat-enum aliases would re-introduce the file-size violation they're meant to solve.

### (d) Trait-object command bus (erase the enum entirely)

Replace `ActorCommand` with `Box<dyn ActorCommandHandler>` where each handler carries its own state and implements `handle(ctx)`. Rejected because:
- Erases the type-level grouping signal entirely — a reader can't enumerate the commands without grepping for `impl ActorCommandHandler`.
- Loses the `Debug`-safe logging property (the current `#[derive(Debug)]` on `ActorCommand` produces a log-safe tag; a trait object would need a per-impl `Debug` that risks leaking secrets — the `SignerSource::LocalNsec` redaction is currently compiler-enforced via the manual `Debug` impl).
- The enum is the right shape for a single-writer inbox (ADR-0050 §D3a): one type, one match, one dispatch authority.