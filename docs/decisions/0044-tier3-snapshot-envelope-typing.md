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

## 2. The two options

### Option (a) — typed fields directly on `SnapshotFrame`

Add the Tier-3 fields as first-class typed fields on the `SnapshotFrame` table in
`nmp_update.fbs` (with new nested tables `Metrics`, `RelayStatus`,
`LogicalInterestStatus`, `WireSubscriptionStatus` in the `nmp.transport`
namespace), appended after `typed_projections`.

### Option (b) — a typed `nmp.kernel.status` projection in the sidecar

Route the Tier-3 fields through one (or a few) `typed_projections` entries keyed
e.g. `nmp.kernel.status`, exactly like the Tier-1 built-ins — `nmp-core` would
own the schema crate-side and emit the bytes through the same sidecar path.

### Evaluation

| Criterion | (a) SnapshotFrame fields | (b) `nmp.kernel.status` projection |
|---|---|---|
| **Schema cleanliness** | Edits the shared transport root — but with *framework-owned* envelope types, which is what an envelope root is *for*. Clean. | Leaves `SnapshotFrame` untouched, but pushes always-present framework metadata into the app-projection escape hatch — semantically wrong (see below). |
| **Consumer-decode ergonomics** | Host reads `frame.snapshot.metrics.storedEvents` etc. by field offset — zero string-keyed map lookups. Replaces today's generic `FlatBufferValueDecoder` walk of `payload`. Cleanest possible read. | Host must: find the sidecar entry by key, match the descriptor, then decode a *second* opaque buffer — to learn whether the kernel is even `running`. Indirection on the most-read fields. |
| **Additivity / safety** | New fields appended at the table tail → existing `payload`-readers byte-compatible; FlatBuffers default-on-absent makes an old reader see an empty frame, never a mis-decode. | Additive too (new sidecar entry), but `schema_version` can't live here — chicken-and-egg (you'd need to decode a versioned payload to learn the version that tells you how to decode payloads). |
| **Uniformity with Tier-1/Tier-2** | *Different* from Tier-1/2 — but correctly so: those are app/protocol-owned shapes the sidecar exists to keep out of nmp-core; Tier-3 is the opposite category. | Superficially uniform ("just another projection"), but the uniformity is false: it equates framework envelope metadata with app projections. |
| **Unblocks deleting `payload:Value`** | Yes — once hosts read the typed fields, the Tier-3 top-level keys no longer need the generic tree. | Yes, mechanically — but only by relocating the same data into a sidecar entry, adding a decode hop for no ownership benefit. |

### The discriminating principle: ownership, not shape

ADR-0037's sidecar exists for exactly one reason (its Commitment 1): keep **app
nouns** — `Feed`, `Dm`, `Group`, `Article` — out of `nmp-core` transport, so
`nmp-core` never declares an app table type and never regenerates bindings for app
churn. The cut that justifies the sidecar is **ownership**: app-owned shapes go in
the opaque sidecar; the transport schema stays closed against them.

Every Tier-3 field is the **opposite category**. `rev`, `running`,
`last_tick_ms`, `Metrics`, `RelayStatus` already live in `nmp-core`'s own
`types.rs`. `nmp-core` declaring its *own* envelope types in its *own* transport
schema is not the ADR-0037 anti-pattern — it is the framework owning its envelope.
The "closed against app churn" property is **fully preserved**, because no app will
ever add a field to this set. Routing them through the app-projection sidecar buys
*nothing*: nmp-core owns the schema either way, so there is no "keep nmp-core from
learning the type" benefit to capture. It is pure indirection.

So the cut is: **app-owned → opaque sidecar (b's mechanism); framework-owned →
first-class `SnapshotFrame` fields (a).** Tier-3 is entirely framework-owned →
all of it is option (a).

### The functional argument that makes (b) not merely messier but wrong

`schema_version`, `last_tick_ms`, `running`, and `update_kind` are needed to
**interpret the frame and detect liveness / version-skew — before and
independently of any projection decode**:

- `schema_version` is the version discriminator the host uses to decide whether it
  can decode *anything* in the frame. It cannot itself live inside a versioned
  opaque payload — that is circular (you would need to decode a projection to learn
  the version that tells you how to decode projections). It must be a plain field
  on the envelope.
- `last_tick_ms` is the actor-liveness signal (ADR-0028): a host watches it stop
  advancing to detect a frozen actor thread. Burying it one opaque-decode deep
  makes the liveness probe pay for a projection decode.
- `running` / `update_kind` classify the frame itself.

These are functional necessities of the envelope, not cleanliness preferences.
That is the lead reason to reject (b).

### Reject the shape-based hybrid (scalars on the frame, vectors in the sidecar)

A tempting middle path puts the scalars on `SnapshotFrame` but routes the big
vectors (`metrics`, `relay_statuses`, `logs`) through the sidecar "to avoid paying
for them every frame." **Reject it** — it splits *framework-owned* state by shape,
which is not a principle:

- The vectors are **still framework-owned**, not app nouns. nmp-core owns
  `Metrics` / `RelayStatus` / `LogicalInterestStatus` / `WireSubscriptionStatus`
  whether they sit on the frame or in a sidecar buffer — there is no
  "keep nmp-core from learning the type" benefit to capture, so the sidecar earns
  nothing.
- **FlatBuffers is lazy / zero-copy.** A typed vector *field* on `SnapshotFrame`
  is not deserialized until a consumer accesses it. A diagnostics-only host that
  never reads `logs` or `wire_subscriptions` does not pay to decode them — the
  field is an offset it never follows. The "don't pay for big vectors every frame"
  motivation does not exist, so the hybrid's only perf argument is empty.
  *(Implementation note: confirm against each platform's generated accessors that
  vector access is offset-on-demand, not eager — this is the standard FlatBuffers
  contract but should be spot-checked when the bindings are generated.)*

The hybrid trades a clean single category boundary for an indirection with no
payoff. One uniform rule — *all* of Tier-3 as typed `SnapshotFrame` fields — is
both simpler and more correct.

## 3. Recommendation — Option (a), for all Tier-3 fields

Add every Tier-3 field as a first-class typed field on `SnapshotFrame`, with the
five nested populations declared as new tables in the `nmp.transport` namespace.
Append all new fields at the **tail** of `SnapshotFrame` so existing `payload`
readers stay byte-compatible (FlatBuffers identifies fields by vtable slot; new
trailing fields read as default/absent on an old reader).

### Additive schema sketch (target — not shipped with this ADR)

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

### Why not a hybrid

Evaluated and rejected above (§2): the shape-based hybrid splits framework-owned
state on a non-principle, and FlatBuffers laziness removes its only perf argument.
The recommendation is the *uniform* rule.

## The reversal (must be stated head-on)

ADR-0037 explicitly recorded that `payload:Value` **stays in `SnapshotFrame`
permanently** and that it **"does not schedule removal of the generic tree"**
(ADR-0037 Consequences → "What this does NOT change", lines 203–206). This ADR is
part of the program that **reverses that commitment**: the end state of the
typed-snapshot migration is the deletion of `payload:Value`.

No intervening ADR scheduled that deletion (verified: a scan of `docs/decisions/`
and the recent git history finds the Tier-1 sidecar waves but no superseding
"delete `payload:Value`" decision). **This ADR is therefore the record that
re-opens the question for the Tier-3 half**, and it states plainly: the ADR-0037
"permanent" wording was correct *while the generic tree was the only home for
envelope metadata and untyped projections*. It ceases to be correct once (Tier-3)
the envelope is typed and (Tier-2) the projection map is typed. This ADR removes
**one of the two** Tier-3/Tier-2 blockers; it does **not** itself schedule the
deletion — the deletion is a later, separate decision that can only land after
*both* halves are done (see §4 and Open risks).

## 4. Consumer impact and migration sequencing

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

## 5. Open risks

- **`Metrics` / `logs` shape churn.** `Metrics` is a wide, frequently-extended bag
  (the perf trio and the `Option<u128>` timing cluster were added incrementally).
  As a typed table it must absorb additions by **appending fields at the tail**
  (never reordering), and a host on an older binding simply doesn't see new tail
  fields. `logs` (`[string]`) is low-risk but unbounded — sizing/truncation policy
  is unchanged by typing and stays an emitter concern.
- **`relay_statuses` is a vector, not a map.** It is `[RelayStatus]` on the wire;
  consumers that want O(1) lookup key it by `relay_url` host-side. No map type is
  introduced. (The singular `relay_status` aggregate and the plural vector reuse
  the *same* `RelayStatus` table — fine, but a reader must not assume the singular
  is element 0 of the plural; they are computed independently in `make_update`,
  `update.rs:153`–`154`.)
- **Present-only-when-active diagnostics.** `store_open_failure`,
  `no_configured_relays`, and the `last_error_*` trio are absent in the healthy
  case (`skip_serializing_if` today). As native-optional FlatBuffers fields,
  absent reads back as null — hosts must treat "field absent" as "condition not
  active," never as a decode error. (This Tier-3 set is distinct from the
  *open-view* payloads — `author_view` / `thread_view` — which are present-only
  entries in the **Tier-2 projections map**, not Tier-3 fields, and are out of
  scope here.)
- **No generic-Value escape hatch.** ADR-0037 argued the long tail of low-frequency
  projections "never needs typing" because the generic tree is always available.
  That is no longer true for host-visible update frames. Any host-visible
  projection must have a typed envelope/sidecar home; internal Rust-only
  `serde_json::Value` projections do not cross the update transport boundary.
- **Two version axes.** Introducing `kernel_schema_version` alongside the existing
  transport `schema_version` on the same table risks confusion. The decision keeps
  both as first-class scalars (they answer different questions: transport-frame
  compatibility vs kernel-vs-shell compatibility); implementation must document the
  distinction at the field site.
