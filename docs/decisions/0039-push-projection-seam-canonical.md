# ADR-0039 — The push projection seam is canonical; reject generic pull accessors

- **Status:** Accepted (2026-05-31)
- **Relates to:** ADR-0025 (Marmot bespoke FFI cluster — the pull anti-pattern this
  ADR finishes retiring), ADR-0037 (typed FlatBuffers sidecar — a hot-path
  optimization *layered on* this seam, not an app-facing alternative to it),
  ADR-0036 (composition-root follow-set expansion — supersedes V-45). Resolves the
  long-standing **V-37** "snapshot output seam for non-Chirp apps" framing and the
  **PD-033-A** second-app thesis blocker. Governs **V-107** (live Marmot read-leg
  migration). Surfaced by the 2026-05-29 podcast-player polling incident and the
  `snapshot-projection-cleanup` + `open-backlog-resolution` workflows.
- **Scope:** how *any* app/host consumes kernel-derived projections across the FFI
  boundary; the disposition of bespoke `nmp_app_*_snapshot` pull symbols; and the
  V-37/PD-033-A affordance question.

## Context

Two patterns for delivering kernel-derived projections to a host coexist in the tree:

1. **PUSH (canonical).** A host registers a projector via
   `register_snapshot_projection` / `register_typed_snapshot_projection`
   (`crates/nmp-ffi/src/snapshot.rs:83`, C header `NmpCore.h:255`). Its output is
   appended to `KernelSnapshot::projections` (and the `typed_projections` sidecar)
   on every tick, serialized into the pushed FlatBuffers `SnapshotFrame`, and read
   by the host from `projections[key]` in its `apply()` callback. Emission is
   edge-triggered (`changed_since_emit`), so there is **no polling**.

2. **PULL (anti-pattern).** Bespoke per-app/per-protocol C-ABI accessors
   (`nmp_marmot_snapshot`, the deleted `nmp_app_chirp_snapshot`, the dead
   `nmp_app_gallery_snapshot`, and the downstream `nmp_app_podcast_snapshot`) that
   the host *calls* to pull serialized JSON. A pull accessor gives the host **no
   signal** for when the data changed, so it forces a poll loop.

The **podcast-player incident (2026-05-29)** is the canonical failure: a downstream
app minted `nmp_app_podcast_snapshot` and drove it with a 500 ms `Task.sleep` poll —
a D8 (no-polling) violation — purely because the push seam was undocumented and the
nearest in-repo examples were bespoke pull symbols. Apps copy the precedent they can
see.

For over a month, **V-37** framed three "missing affordances" as the blocker for an
honest stateful second app (**PD-033-A**), including **(b) a generic
`nmp_app_get_snapshot` PULL path**. The 2026-05-29 cleanup workflow established
(against stale wiki claims) that the push seam already exists, works end to end, has
live host exemplars (`nmp-app-template`, `nmp-nip02/17/47/57`, chirp `register.rs`),
and now has builder-guide docs (PR #790). The premise that the framework lacks a
projection path for non-Chirp apps is **false**.

## Decision

1. **The PUSH seam is the single canonical way for any app to consume kernel
   projections.** One seam, one default: register a projector → read `projections[key]`
   off the pushed frame. This satisfies the no-dual-seam doctrine and D8.

2. **Reject a generic `nmp_app_get_snapshot` pull accessor (V-37 affordance (b)).**
   A pull accessor has no change signal and forces polling — the exact D8 violation
   the incident demonstrated. It is an anti-pattern, not a missing affordance.

3. **V-37 affordance reassessment → V-37 is obviated, close it.**
   - **(a) `NmpSnapshotProjector` context pointer — obviated.** The projector is
     where state is *read out and reconciled*, not where it arrives. The live
     `nmp-app-template` controllers prove closure capture (an `Arc`-shared
     `KernelEventObserver` + `AppHost` handles) provides state access more safely
     than a `*const c_void` would. Do not build it.
   - **(b) generic pull path — rejected** per Decision 2.
   - **(c) follow-set interest — already provided** by ADR-0036's composition-root
     expansion (V-45 superseded).

4. **Bespoke pull-snapshot symbols are deprecated debt → migrate onto the push seam
   and remove (V-107).** The dead gallery chain is removed (PR #791); `nmp_app_chirp_snapshot`
   was already removed (PR #733/#766). The live **Marmot read-leg** symbols
   (`nmp_marmot_snapshot`, `nmp_marmot_group_messages`) migrate to registered
   projections; `nmp_marmot_group_messages`'s `group_id` parameterization folds into
   an `nmp.marmot.snapshot` projection keyed by group id. **The ADR-0025 Step-12
   read-leg sanction is rescinded** — it cited the now-deleted Chirp pull precedent,
   which this ADR reverses.

5. **PD-033-A is unblocked today — zero new affordances required.** An honest
   stateful second app must demonstrate, all consumed off the push frame:
   kernel-owned projection (no D5 parsing in the shell), handshake-gated sign-in (via
   the existing `projections["bunker_handshake"]`), and D3 outbox routing (not a
   raw-event tap). The **podcast-player** is the live candidate and must be re-built
   on the push seam (delete its bespoke pull symbol + poll).

## Consequences

- The builder-guide (ch. 15 / 17, PR #790) is the canonical teaching of the push
  seam; bespoke pull examples are removed or flagged as the anti-pattern.
- The C-ABI surface-freeze CI gate should **reject new `nmp_app_*_snapshot` pull
  exports** (extend the existing freeze check).
- ADR-0037's typed sidecar stays a hot-path *performance* optimization layered on
  the push seam — never an app-facing encoding choice — consistent with the
  single-seam principle here.
- V-37 closes as obviated; PD-033-A re-opens as buildable (no affordance gate);
  V-107 proceeds under this ratification.

## Alternatives considered

- **Keep both push and pull (status quo).** Violates the no-dual-seam and no-polling
  doctrines; it is what let the podcast-player incident happen. Rejected.
- **Build the generic pull path as V-37 (b) specified.** Institutionalizes polling at
  the FFI boundary. Rejected.

---

## Amendment — Marmot messages projection design (2026-05-31)

**Context:** V-107 Rust leg (PR feat/v107-marmot-push-projections) implements Decision 4
above for the two live Marmot pull symbols. One design question required explicit resolution:
`nmp_marmot_group_messages(group_id_hex)` is *parameterized* — the pull symbol takes a
`group_id_hex` argument. The push seam takes no arguments. Two shapes were considered for the
`"nmp.marmot.messages"` projection:

1. **All-groups keyed object (chosen).** A single `"nmp.marmot.messages"` projection that
   emits a JSON object `{ group_id_hex: MarmotMessageRow[] }` for every joined group. The
   host reads `projections["nmp.marmot.messages"][group_id_hex]`.

2. **Active-group-only.** Project only the "currently active" group's messages, requiring
   the kernel to know which group the user is viewing (view-state leak into the kernel).

**Decision: option 1 (all-groups keyed object).**

Rationale:
- **D1 / no view-state in kernel.** An "active group" concept would require a round-trip
  to set kernel state from the host — a one-way-data-flow violation. The keyed object
  keeps all view-state decisions on the host side.
- **Clean edge-triggering.** `"nmp.marmot.snapshot"` emits the group list / membership /
  key-package status; `"nmp.marmot.messages"` emits per-group message tails. A new message
  in one group updates `"nmp.marmot.messages"` without re-emitting the whole group list,
  and the host can diff at the group level.
- **Cheap reads.** `service().get_messages(&gid)` reads from the MDK SQLite message store
  directly — already-decrypted rows, no re-decrypt on each tick. Bounded newest-N per group
  (200 rows, `DEFAULT_MESSAGE_PAGE`). The single lock covers all groups in one iteration.
- **No STOP condition.** The task spec called for stopping and escalating if per-group
  messages required re-decryption on each tick. Confirmed it does not: MDK stores decrypted
  messages in SQLite; `get_messages` is a plain read, not a decrypt call.

**Edge-trigger confirmation:** The kernel sets `changed_since_emit = true` in
`kernel/ingest/mod.rs` (wildcard arm, line ~507) for every accepted inbound event,
including kind:445 (group message) and kind:1059 (gift-wrap welcome) handled by the
`MarmotIngestTap` raw-event observer. The push projection closures run on the very next
snapshot tick after that ingest — reactive, not polled (D8 satisfied).

**Registration:** Both projections are registered in `nmp-marmot/src/ffi.rs:register_with_keys`
via `NmpApp::register_snapshot_projection`. The registry is replace-by-key, so a second
`register_with_keys` call (account switch) replaces the prior closures without accumulation.

**Rust consumers:** `chirp-repl/src/app.rs` and `chirp-tui/src/runtime_commands.rs` were
migrated off the deprecated C-ABI pull symbols onto `MarmotHandle::snapshot_rust()` and
`MarmotHandle::messages_rust()` — Rust-native accessors on the same `MarmotProjection`,
same data path as the push projections, no C-ABI round-trip.

**Deprecated, not deleted:** `nmp_marmot_snapshot` and `nmp_marmot_group_messages` carry
`#[deprecated]` but remain exported. `MarmotBridge.swift` still calls the C-ABI symbols;
they will be removed in a later Xcode session once that Swift consumer migrates to reading
`projections["nmp.marmot.snapshot"]` / `projections["nmp.marmot.messages"][gid]` off
the pushed SnapshotFrame `apply()` callback.
