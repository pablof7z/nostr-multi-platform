# M10 — Blossom + media + long-running capabilities

> Part of the [Build & Validation Plan](../plan.md). Arc 1 — Kernel substrate + Nostr social stack.

**Demo product:** Chirp user can attach a photo to a compose, see upload progress, and the published note has a valid Blossom URL. Profile-picture upload also works.

**Scope.** Per spec §7.11. Establishes the **long-running capability lifecycle pattern** that future media-heavy app proofs can build on:

**Subsystem deliverables.**

- `nmp-blossom` protocol module: upload action module + download action module + media view module + upload-status view (progress).
- `FilePickerCapability` real implementation on iOS (PHPicker for photos / `UIDocumentPicker` for files).
- `BlossomCapability` callback interface: kernel asks platform to perform an HTTP PUT with progress; platform reports progress + completion back via reverse callback into the actor.
- Long-running action lifecycle: upload registers in the action ledger as `AwaitingCapability`; capability progress updates the ledger row; restart recovery resumes from the last checkpointed progress.
- Resumable uploads (Blossom range support where the server allows).
- BUD-01 / BUD-02 protocol support.

**Exit gate.**

- Upload a 5 MB photo on iOS, kill the app mid-upload, restart — upload resumes from the checkpoint, does not restart from byte 0.
- Cancellation works mid-upload (capability reports back `Cancelled`; ledger row finalizes correctly).
- Slow-network upload remains responsive — main UI is never blocked.
- Profile picture update through compose → kind:0 republish with new Blossom URL → in-place refinement across all open Profile / Timeline payloads (per doctrine D1).

**Runnable artifact.** Chirp with media compose. Report in `docs/perf/m10/blossom.md`.
