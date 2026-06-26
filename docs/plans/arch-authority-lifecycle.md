# WIP Plan — NMP capability/authority & lifecycle ownership

**Status:** design / pre-ADR · **Started:** 2026-06-15 · **Owner-driven**

Temporal working file. Detail moves into ADR(s) + durable wiki once landed; delete this file when merged.

**Sibling plan:** event-flow architecture (acquire/ingest/publish — the kernel's one kind-agnostic door) lives in
[`docs/plans/arch-fixes.md`](./arch-fixes.md). This plan is the **separate problem domain** surfaced by the same
2026-06-15 parallel audit: **the kernel owning its own *authority and lifecycles*** rather than leaking them to the
host/native shell or scattering them across ad-hoc state. Same caliber as the event-flow work, orthogonal concern,
sequenced independently.

---

## Thesis

Two ownership leaks of equal severity to the event-flow ones:

1. **Authority is ambient, not capability-scoped.** Protocol/signer code reaches for broad ambient surfaces
   (`active_local_keys` in `ProtocolCommandContext`, the wide `AppHost` trait, native-inferred capability state)
   instead of receiving exactly the narrow capability it needs. Signer backend (local vs bunker) is supposed to be
   invisible to protocol workers (see `blossom-uploads-and-signer-transparency`, `signer-session-port`,
   `capability-socket` durable notes); today it isn't fully.
2. **Lifecycle/expiry is host-driven or multi-sourced.** Action feedback exists across `action_stages` /
   `action_results` / `action_lifecycle`; pending-state expiry leans on hosts/event-ingress instead of kernel time;
   projection declarations can drift from the decoded set.

Both are "the kernel must own X, with a narrow seam" — X = authority (Workstream D) and lifecycle (Workstream E).

Each item below lands as an atomic PR or short PR sequence. Each PR **removes the old path it replaces** and adds a
test or doctrine gate preventing reintroduction. No compat shims, no "later" TODOs, no parallel authorities.

---

## Workstream D — signer and capability authority
- [ ] Make external-signer capability state explicit and kernel-owned: native reports raw capability facts and raw
      completion results; Rust owns retry, fallback, serialization, and user-visible state.
- [ ] Interactive signer operations cannot be represented by a native singleton correlation slot. Either the Rust
      signer serializes them or the capability contract represents multiple pending operations safely.
- [ ] Split requested permissions from persisted granted capability facts at the NIP-55 capability seam so capability
      selection is based on Rust-owned state, not ad-hoc native inference.
- [ ] Make actor-facing `BunkerBroker::cancel()` signal-only/detached; joins belong to a background reaper or
      worker-owned shutdown path, never the actor/capability call path.
- [ ] Remove `active_local_keys` from `ProtocolCommandContext` / protocol command authority; route protocol
      signing/encryption through signer-session ports or a named non-protocol capability.
- [ ] Split broad `AppHost` authority into narrow registration/capability traits so protocol modules receive only the
      surfaces they actually use.
- [x] Doctrine gate: protocol/command code cannot reference `active_local_keys` or the broad `AppHost`; signing goes
      through the signer-session port only. **Landed as doctrine-lint rule D26** (D21-adjacent). `AppHost` is banned
      across the protocol-command surface (reusable protocol crates + `nmp-core` `substrate/protocol*` /
      `actor/commands/`, minus the `AppHost` definition and the `nmp-defaults`/`nmp-ffi` composition root).
      `active_local_keys` is banned in the protocol-command IMPLEMENTATION crates (NIP crates + marmot/blossom/nwc).
      Both halves are green on master and non-vacuous (pos/neg fixtures + per-crate clean guards). NOTE: item 5 above
      (remove the `active_local_keys` accessor FROM `ProtocolCommandContext` / `LocalSignerAccess` in `nmp-core`) is
      still open — the gate locks the implementation surface so no command can re-grow a reach once item 5 lands; it
      does not (and must not) fire on the legitimate port definition that item 5 removes.

## Workstream E — action/projection lifecycle ownership
- [ ] Action feedback has one kernel-owned lifecycle. If `action_stages`, `action_results`, and `action_lifecycle`
      still coexist, collapse them so ack is early-dismiss/retention cleanup, not a correctness dependency.
- [ ] Expiration of pending/action state is driven by kernel time at update/snapshot boundaries as well as event
      ingress; no host is responsible for making stale state disappear.
- [ ] Projection declaration is generated or mechanically checked from the registry so host-declared consumed
      projections cannot drift from the decoded set.
- [ ] Empty declared projection sets warn/assert instead of silently permitting every projection and hiding
      serialization waste.

---

## Doctrine ties
- **D0** — capability honesty: modules receive narrow capability traits, not ambient god-objects; signer backend
  invisible to protocol workers.
- **D9** — kernel owns time: lifecycle expiry is driven by the injected clock at update/snapshot boundaries, never by
  a host calling "clean up now."
- **ADR-0046 / nmp-defaults**, signer-session-port, capability-socket — prior seams this work extends.

## Verification / gates
Each workstream PR carries its own regression gate (listed inline above). No shared chokepoint here — these are
ownership-boundary fixes, each independently testable.

## Sequencing
Independent of `arch-fixes.md`. D and E are also independent of each other and can run in parallel. Write a short ADR
per workstream (or one combined "kernel owns authority & lifecycle" ADR) before the implementing PRs.

## Open questions
- Is one combined ADR right, or one per workstream? (D is a bigger surface than E.)
- Which of D/E items are v1-blocking vs post-v1? Check GitHub Issues and v1 scope before sequencing.
- Cross-check against existing issues for V-78 (`active_local_keys` signing defect) and the signer-session-port work so
  this doesn't duplicate in-flight fixes.
