# ADR-0044 — Typing the Tier-3 top-level snapshot envelope fields

> Current status (2026-06-16): implemented and schema-clean. PR-B/F-05/F-10
> moved production reads to typed `SnapshotEnvelope` fields plus typed projection
> sidecars; the follow-up cleanup removed `SnapshotFrame.payload` and the
> `Value`/`Pair` variant tree from `nmp_update.fbs`. There is no schema-level
> compatibility slot or generic JSON escape hatch in the update frame.

- **Status:** Accepted / implemented (proposed 2026-06-10; PR-B completed 2026-06-13)
- **Relates to:** ADR-0037 (typed FlatBuffers sidecar for runtime projections —
  defines `TypedProjection` / `TypedPayload`; its `payload:Value` permanence and
  compatibility text is superseded by the current status above), ADR-0038 (typed
  op-feed projection — first sidecar payload after the
  `nmp.feed.home` pilot), ADR-0032 (raw-data projection doctrine), ADR-0033
  (`nmp-feed` viewport FFI). **Partially reverses** one specific commitment of
  ADR-0037 — see *The reversal* below.
- **Scope:** the `nmp-core` transport schema
  (`crates/nmp-core/schema/nmp_update.fbs`, the `SnapshotFrame` root table) and
  its checked-in bindings; the snapshot emission path
  (`crates/nmp-core/src/kernel/update.rs::make_update`); the `KernelSnapshot`
  struct (`crates/nmp-core/src/kernel/types.rs`); and every platform host that
  decodes the update stream (iOS Chirp `KernelUpdateFrameDecoder.swift`,
  chirp-tui, web, Android).
- **This is a decision record. No `.fbs`, `.rs`, or codegen changes ship with it.**
  The schema sketches below are the agreed *target*; implementation is a later,
  separately-reviewed step.

## Context

The body below is the historical decision record that prepared the migration.
References to dual-emission, fallback, or a staged removal window describe the
pre-2026-06-16 rollout path, not the current transport contract.

NMP is migrating the kernel snapshot off the dynamic `payload:Value` JSON-equivalent
tree onto strongly-typed FlatBuffers. The migration has three tiers:

- **Tier-1** — protocol projections (e.g. `nmp.feed.home`, `wallet`, `dm_inbox`,
  `nmp.wot.bootstrap`). Routed through the `typed_projections` sidecar defined by
  ADR-0037. **Done / in-wave** (see commits `302e9cec`, `09263c47`, `5db9de0f`,
  `944d5e92`, `e94b0237`, …). These are *app/protocol-owned* shapes — the sidecar
  exists precisely so `nmp-core` transport never declares their table types.
- **Tier-2** — kernel built-in projections carried in the `projections` map
  (`publish_queue`, `accounts`, `active_account`, `timeline`, `profile`,
  `claimed_events`, …). **In progress.**
- **Tier-3** — the **top-level `KernelSnapshot` scalar/struct fields**. This ADR.

The final step of the whole migration — deleting `payload:Value` from
`nmp_update.fbs` — is blocked by **two** distinct populations of data that the
`payload` tree carries today, and *both* must be typed before the field can go:

1. the **Tier-2 `projections` map** (a `HashMap<String, serde_json::Value>` —
   `types.rs:925`), and
2. the **Tier-3 envelope fields** — the rest of `KernelSnapshot`, which become the
   *top-level keys* of the `payload` Value root.

This is concrete in the emission path. `make_update`
(`update.rs:65`–`223`) assembles one `KernelSnapshot` struct and serializes the
**whole struct** with `serde_json::to_value(&update)` (`update.rs:223`); the
resulting `Value` *is* `SnapshotFrame.payload`. So `rev`, `running`, `metrics`,
`relay_statuses`, `logs`, etc. are not "inside a projection" — they are the
top-level map keys of the `payload` root. On the consumer side, Chirp's
`KernelUpdateFrameDecoder.swift` decodes the entire `KernelUpdate`
(= `KernelSnapshot`) from `snapshot.payload` through a generic
`FlatBufferValueDecoder` (line 56), then separately pulls the typed sidecar via
`extractTypedProjections` (line 57). Tier-3 is the half still read from the
generic tree.

**This ADR decides only how to type Tier-3.** It removes *one of the two* blockers
on deleting `payload:Value`; Tier-2 is the other and is out of scope here.

## 1. Inventory of the Tier-3 fields

All field citations are `crates/nmp-core/src/kernel/types.rs`. The whole
`KernelSnapshot` struct begins at `types.rs:799`. Nested struct definitions are
cited where they live. "Scalar" = a primitive/optional-primitive that maps to a
plain FlatBuffers scalar or native-optional field; "nested" = a struct or vector
that needs its own FlatBuffers table.

### identity / revision

| Field | Type | `types.rs` | Shape |
|---|---|---|---|
| `rev` | `u64` | `:800` | scalar |
| `schema_version` | `u32` | `:804` | scalar |

### timing

| Field | Type | `types.rs` | Shape |
|---|---|---|---|
| `last_tick_ms` | `u64` | `:815` | scalar |

### run-state

| Field | Type | `types.rs` | Shape |
|---|---|---|---|
| `update_kind` | `&'static str` (always `"ViewBatch"`) | `:816` | scalar (string) |
| `running` | `bool` | `:817` | scalar |

### metrics

| Field | Type | `types.rs` | Shape |
|---|---|---|---|
| `metrics` | `Metrics` (struct, ~50 scalar fields) | `:831` (def `:729`) | **nested struct** |

`Metrics` (`types.rs:729`–`792`) is a flat bag of `u64`/`usize`/`u32`/`f64`
counters plus a cluster of `Option<u128>` millisecond timestamps
(`first_event_ms`, `target_profile_loaded_ms`, …) and the perf trio
(`make_update_us`, `serialize_us`, `update_frame_degradations_total`). No nesting
below it — it flattens to one typed table of scalars.

### relay-status

| Field | Type | `types.rs` | Shape |
|---|---|---|---|
| `relay_status` | `RelayStatus` (struct) | `:832` (def `:324`) | **nested struct** |
| `relay_statuses` | `Vec<RelayStatus>` | `:833` | **nested vector** |

`relay_status` (singular) is the aggregate/primary connection summary;
`relay_statuses` (plural) is the per-relay vector. Both reuse the one
`RelayStatus` shape (`types.rs:324`–`354`): `role`, `relay_url`, `connection`,
`auth`, counters, and several `Option<String>` diagnostic fields. Consumers key
the vector by `relay_url` — it is a **vector**, not a map on the wire.

### interests / subscriptions

| Field | Type | `types.rs` | Shape |
|---|---|---|---|
| `logical_interests` | `Vec<LogicalInterestStatus>` | `:834` (def `:377`) | **nested vector** |
| `wire_subscriptions` | `Vec<WireSubscriptionStatus>` | `:835` (def `:360`) | **nested vector** |

`LogicalInterestStatus` (`:377`): `key`, `state`, `refcount`,
`relay_urls:Vec<String>`, `cache_coverage`, `warming_until_ms:Option<u128>`.
`WireSubscriptionStatus` (`:360`): `wire_id`, `relay_url`, `filter_summary`,
`state`, counters, `Option<u128>` timestamps, `close_reason:Option<String>`.

### logs

| Field | Type | `types.rs` | Shape |
|---|---|---|---|
| `logs` | `Vec<String>` | `:836` | **nested vector** (`[string]`) |

### error / diagnostic (all present-only-when-active)

| Field | Type | `types.rs` | Shape |
|---|---|---|---|
| `last_error_toast` | `Option<String>` | `:858` | scalar (optional) |
| `last_error_category` | `Option<String>` | `:864` | scalar (optional) |
| `last_planner_error` | `Option<String>` | `:868` | scalar (optional) |
| `store_open_failure` | `Option<String>` | `:878` | scalar (optional) |
| `no_configured_relays` | `Option<bool>` | `:891` | scalar (optional) |

The last two carry `#[serde(skip_serializing_if = "Option::is_none")]` — absent
from the wire in the healthy case. In FlatBuffers these are **native optional
scalar fields**: absent reads back as null, no presence wrapper needed.

**Not Tier-3:** the `projections` map (`types.rs:925`) is Tier-2 and is *not*
part of this decision. The "views cluster" (`timeline`, `profile`, `author_view`,
…) and the publish/identity clusters are entries *inside* that map, not typed
`KernelSnapshot` fields (the long comments at `types.rs:818`–`924` document that
they were deliberately de-typed into the map). They migrate under Tier-2.

**Shape summary:** five **nested** populations (`metrics`, `relay_status` +
`relay_statuses`, `logical_interests`, `wire_subscriptions`, `logs`); everything
else is a scalar or optional scalar.

## 2. Decision — typed fields directly on `SnapshotFrame`

Every Tier-3 field is a first-class typed field on the `SnapshotFrame` table in
`nmp_update.fbs`, with the five nested populations (`Metrics`, `RelayStatus`,
`LogicalInterestStatus`, `WireSubscriptionStatus`, plus the `logs` string vector)
declared as new tables in the `nmp.transport` namespace, appended after
`typed_projections`. They were *not* routed through the ADR-0037 app-projection
sidecar: that sidecar exists to keep **app nouns** out of `nmp-core` transport,
and Tier-3 is the opposite category — framework-owned envelope metadata that
`nmp-core` already declares in its own `types.rs`. `schema_version`,
`last_tick_ms`, `running`, and `update_kind` must be plain envelope fields anyway:
they are read to interpret the frame and detect liveness/version-skew *before* any
projection decode (a versioned field cannot live inside a versioned opaque
payload).

### Typed `SnapshotFrame` schema sketch

```fbs
// New nested tables in namespace nmp.transport. Each mirrors the nmp-core
// struct of the same name; all fields raw per ADR-0032 (hex pubkeys, u64
// unix-seconds, raw counts) — typing is a transport optimization, not a
// license to pre-format.

table Metrics {
  generated_events:ulong;
  note_events:ulong;
  // … the full flat scalar set from types.rs:729-792 …
  stored_events:ulong;            // usize on the Rust side → ulong on the wire
  store_to_payload_ratio:double;
  first_event_ms:ulong = null;    // Option<u128> → native-optional scalar
  // … remaining Option<u128> timestamps as native-optional scalars …
  make_update_us:ulong;
  serialize_us:ulong;
  update_frame_degradations_total:ulong;
}

table RelayStatus {
  role:string;
  relay_url:string;
  connection:string;
  auth:string;
  negentropy_probe:string;
  active_wire_subscriptions:uint;
  reconnect_count:uint;
  last_connected_at_ms:ulong = null;   // Option → native-optional
  last_event_at_ms:ulong = null;
  last_notice:string;                  // Option<String> → absent = null
  last_error:string;
  error_category:string;
  events_rx:ulong;
  bytes_rx:ulong;
  bytes_tx:ulong;
  denied:bool = false;
  last_close_reason:string;
}

table LogicalInterestStatus {
  key:string;
  state:string;
  refcount:uint;
  relay_urls:[string];
  cache_coverage:string;
  warming_until_ms:ulong = null;
}

table WireSubscriptionStatus {
  wire_id:string;
  relay_url:string;
  filter_summary:string;
  state:string;
  logical_consumer_count:uint;
  events_rx:ulong;
  opened_at_ms:ulong;
  last_event_at_ms:ulong = null;
  eose_at_ms:ulong = null;
  close_reason:string;
}

// SnapshotFrame — existing fields UNCHANGED and IN ORDER; new fields APPENDED.
table SnapshotFrame {
  schema_version:uint = 1;                 // existing (transport schema version)
  // Historical note: this slot existed when the ADR was written. The current
  // transport schema has no generic payload field.
  typed_projections:[TypedProjection];     // existing (ADR-0037)

  // ── Tier-3 envelope fields (new, all appended at the tail) ──
  rev:ulong;                               // identity/revision
  kernel_schema_version:uint;              // KERNEL_SCHEMA_VERSION (distinct from
                                           // the transport schema_version above)
  last_tick_ms:ulong;                      // timing / actor-liveness (ADR-0028)
  update_kind:string;                      // run-state ("ViewBatch")
  running:bool = false;                    // run-state
  metrics:Metrics;                         // nested
  relay_status:RelayStatus;                // nested (aggregate)
  relay_statuses:[RelayStatus];            // nested vector (keyed by relay_url)
  logical_interests:[LogicalInterestStatus];
  wire_subscriptions:[WireSubscriptionStatus];
  logs:[string];
  last_error_toast:string;                 // Option → absent = null
  last_error_category:string;
  last_planner_error:string;
  store_open_failure:string;
  no_configured_relays:bool = null;        // Option<bool> → native-optional
}
```

Notes on the sketch:

- **`kernel_schema_version` vs the existing `schema_version`.** `SnapshotFrame`
  already has a `schema_version` field — that is the *transport* schema version.
  The Tier-3 `schema_version` is `KERNEL_SCHEMA_VERSION` (a different concept:
  kernel-vs-shell mismatch detection, `types.rs:804`). To avoid conflating two
  versioning axes the new field is named `kernel_schema_version`. (Final naming is
  an implementation detail; the *decision* is that both versions are first-class
  envelope scalars.)
- **`usize` → `ulong`.** Rust `usize` counters serialize as `ulong` on the wire
  for platform-width independence.
- **`Option<T>` mapping.** `Option<u128>`/`Option<bool>` → FlatBuffers
  native-optional scalars (`= null` default); `Option<String>` → a string field
  that is simply absent when `None`. No presence-wrapper table is needed for any
  Tier-3 field. This directly answers the "present-only-when-active" diagnostic
  fields (`store_open_failure`, `no_configured_relays`, the `last_error_*` set):
  they are optional fields, absent = healthy.

**Reverses ADR-0037's `payload:Value` permanence commitment.** ADR-0037 recorded
that `payload:Value` stays in `SnapshotFrame` permanently and "does not schedule
removal of the generic tree." Typing Tier-3 (this ADR) plus typing Tier-2 removed
both blockers, and `payload:Value` and the `Value`/`Pair` variant tree were
deleted from `nmp_update.fbs` (2026-06-16). The "permanent" wording was correct
only while the generic tree was the sole home for envelope metadata and untyped
projections.

## 3. Consumer impact

### Current emitter (`nmp-core`)

`make_update` / `encode_snapshot_with_envelope` populates the typed
`SnapshotFrame` fields and `typed_projections` sidecars directly. It does not
serialize a `KernelSnapshot` JSON tree into the transport frame, and the schema no
longer has a `payload:Value` field for an emitter to fill.

### Current consumers

- **iOS Chirp**, **chirp-tui**, **web / TypeScript**, and **Android / Kotlin**
  read the typed envelope fields and typed projection sidecars from generated
  bindings.
- **Gallery Android/iOS** receives the same production `UpdateFrame` bytes but
  asks the Rust `nmp-app-gallery` helper to decode the typed envelope/sidecars
  into the gallery's existing JSON model. The native shells no longer carry a
  local generic `Value` decoder.

There is no host fallback to `payload:Value`; unknown future projection schemas
fail closed at the per-sidecar decoder boundary rather than falling back to a
schema-level JSON tree.

### Where this sits in the program

```
Tier-1 sidecar (ADR-0037/0038)  ──┐
  feed/wallet/dm/wot/zaps/…       │  (done / in wave)
                                  ├──►  payload:Value DELETED (2026-06-16)
Tier-2 projections map  ──────────┤      (typed sidecars own host-visible data)
  publish_queue/accounts/timeline │      iff BOTH Tier-2 and Tier-3 done
  /claimed_events/… (in progress) │
                                  │
Tier-3 envelope fields  ──────────┘
  THIS ADR (decision) → impl → host adoption → schema cleanup
```

This ADR was the Tier-3 decision that made the final schema cleanup possible. The
current transport contract is typed-only.

## 4. Two version axes (durable note)

`SnapshotFrame` carries two distinct version scalars and they must not be
conflated: the existing `schema_version` is the **transport-frame** schema
version, and `kernel_schema_version` is `KERNEL_SCHEMA_VERSION` (kernel-vs-shell
mismatch detection, `types.rs:804`). Both are first-class envelope scalars because
they answer different questions (transport-frame compatibility vs kernel-vs-shell
compatibility); the field site documents the distinction.
