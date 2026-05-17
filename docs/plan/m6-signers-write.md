# M6 — Sessions + signers + write path

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** iOS app gets a login screen. After login the user can compose and publish a kind:1 note to primal that atomically appears in their own timeline.

**Scope.** Per `product-spec.md` §7.4, §7.5, §7.15:

**Subsystem deliverables.**

- `IdentityModule::HumanAccount` with local-key signer (raw nsec, NIP-49 encrypted).
- `IdentityModule::ExternalSigner` with NIP-46 (Nostr Connect / bunker) signer.
- `KeychainCapability` real implementation: encrypted nsec storage via iOS Keychain, app-private access group.
- Action ledger in `nmp-core::kernel::ledger`: durable rows with ULID action IDs, status transitions, retry/cancel, restart recovery.
- Action atomicity contract: a `SendNote` action's publish to relays and local store insert happen as one actor message; partial failure rolls back.
- `nmp-nip01::SendNoteActionModule` as the first write-path action.
- Login UX (single nsec field for now; multi-step onboarding deferred to [M16](m16-cli-starter.md)).

**Exit gate.**

- Bug-extinction #7 (action partial-success): inject "publish OK / store fail" and "store OK / publish fail" — both roll back atomically.
- Bug-extinction #9 (NIP-46 lost on suspend): simulate suspend mid-publish; resume retries or surfaces failure as toast.
- Bug-extinction #10 (re-publish keeps event id): re-publish of an event preserves `id` and `sig`.
- Compose flow on iOS: login → compose → publish → note visible on primal externally and in local timeline within one ViewBatch.

**Runnable artifact.** iOS Twitter slice with working compose. Report in `docs/perf/m6/write-path.md`.
