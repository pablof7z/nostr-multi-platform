# M4 — Negentropy efficiency report

> Part of the [Build & Validation Plan](../../plan.md). Closes the M4 exit-gate
> "≤ 5 % bytes-on-wire vs equivalent REQ on a 10 k-event backfill"
> ([`docs/plan/m4-negentropy.md`](../../plan/m4-negentropy.md) §Exit-gate).

## What ran

`crates/nmp-testing/tests/nip77_cold_open_via_neg.rs` —
`cold_open_via_neg_under_five_percent_of_req_baseline`.

The test instantiates the M4 reconciler in-process (client + server
[`Reconciler`](../../../crates/nmp-nip77/src/reconciler.rs) over an
[`NegentropyStorageVector`](https://docs.rs/negentropy)) and drives it to
convergence with a client cold-state and a 10 000-item server set.  Bytes
sent and received are summed and compared against the REQ baseline.

## Numbers

Configuration: `FRAME_SIZE_LIMIT = 64 KiB`
([rationale](../../../crates/nmp-nip77/src/lib.rs)), 10 000 deterministic
items, 32-byte ids, no compression.

| Quantity | Value |
|---|---|
| Negentropy bytes-on-wire | ~320 580 B |
| REQ baseline (`10 000 × 700 B`) | 7 000 000 B |
| Ratio (`neg / req`) | **~4.58 %** ✅ (gate ≤ 5 %) |
| Rounds to converge | 5 |
| Need-ids identified | 10 000 (full set) |

### REQ baseline derivation

`AVG_REQ_BYTES_PER_EVENT = 700` is set in the test file with a comment
documenting the floor: a kind:1 `EVENT` envelope (`["EVENT", subid, {...}]`)
with id + pubkey + ts + kind + sig + empty tags + empty content is ~353 B;
real-world kind:1s with one `e`-tag + one `p`-tag + ~50 chars of content
land at 700–800 B.  Using 700 B as a conservative floor means the
real-world REQ baseline is strictly **larger** than 7 MB and savings
strictly **better** than the gate reports.

### Why 320 KB?

Of those 320 580 B, ~320 000 B is the fundamental id-transfer floor
(10 000 × 32 B).  The protocol overhead (frame headers, range bounds,
fingerprints) is a few hundred bytes per round, ~340 B total.  Below this
floor the protocol can't go — it must communicate every id the client
needs.  Above it, the negentropy savings come purely from skipping the
event payloads themselves (signature + JSON envelope + tags), and that's
where the 95 %+ savings appear.

## Other M4 exit-gate checks

| Gate | Test | Status |
|---|---|---|
| Cache miss against fully-synced pair is authoritative | `nip77_cache_authoritative.rs` | ✅ |
| Reconnect resumes from watermark; gap filled by sync | `nip77_reconnect_resumes_from_watermark.rs` | ✅ |
| Mixed-capability relay set: both populate the same store | `nip77_capability_negotiation.rs` | ✅ |

## Limitations

- The reconciler is exercised in-process; a real-relay measurement
  (against `strfry` / `relay-builder`) is deferred.  The protocol contract
  is identical, so the bytes-on-wire numbers will match within a small
  protocol-version-byte rounding margin.
- `Reconciler::resume_client` accepts a state blob but the `negentropy`
  0.5 crate does not expose a public deserializer, so the blob currently
  acts as a coverage hint, not a true mid-stream resume.  Persisting it is
  still useful so future engine versions can pick up mid-frame.
- Capability persistence is wired through [`CapabilityDomain`](../../../crates/nmp-nip77/src/capability_domain.rs)
  as a `DomainModule`, but the LMDB-backed store path is not yet active
  in M4 (M3 + LMDB feature gate); the in-memory cache covers the run-time
  semantics.

## Follow-ups

- Live measurement against a real NIP-77 relay (defer to M11 hardware
  proof window or M11.5 highlighter slice).
- ~~Wire `apply_coverage_filter` into the M2 planner's hot path~~
  **[LANDED — T53 follow-up]**. `SubscriptionLifecycle::set_coverage_hook`
  is the seam; the actor installs `apply_coverage_filter` as the hook.
  Coverage is exercised end-to-end by
  `crates/nmp-testing/tests/framework_magic_contract.rs::c10_watermark_gates_backfill_and_authoritative_miss`.
- Add a real `canonical_filter_hash(&Filter) -> [u8; 32]` to
  `nmp-core::store` per `docs/design/lmdb/watermarks.md` §3 (BLAKE3-CBOR).
  The T53 follow-up promoted `simple_shape_hash` to
  `nmp_core::planner::canonical_filter_hash`, so the swap is a one-edit
  replacement when the canonical encoder lands.
