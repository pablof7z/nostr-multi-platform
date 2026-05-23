# V6 — `nmp-codegen` Swift `Decodable` emitter — implementation plan

- **Status:** Plan (no code lands with this doc).
- **Severity:** HIGH.
- **Doctrine:** D2 (single source of truth) — the Rust projection types are the
  truth; the Swift `Decodable` mirrors are noise that has drifted and will
  drift again.
- **ADR foundation:** ADR-0030 §(b) has already chosen the lane ("ship a Swift
  `Decodable` emitter in `nmp-codegen`"). This plan resolves the open
  sub-questions ADR-0030 left unanswered: *what input format the emitter
  reads*, *how the dotted-projection-key registry is represented*, and *how
  the build / CI gate is wired*.
- **Backlog item:** F-05 (`docs/BACKLOG.md:287`).

---

## 1. Current state — what does `nmp-codegen` actually do today?

### 1a. Code inventory

`crates/nmp-codegen/` is ~1.2k LoC of pure-Rust scaffolding generator. It
emits **Rust** files per per-app manifest, and **zero** files for any host
platform.

| File (absolute path) | Role |
|---|---|
| `/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-codegen/src/lib.rs` | Public API: `generate_modules`, `check_modules`, helpers `rust_crate_name`, `variant_name`, `app_crate_name`. |
| `/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-codegen/src/main.rs` | `nmp gen modules [--manifest nmp.toml] [--out DIR] [--check]` CLI. |
| `/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-codegen/src/manifest.rs` | Hand-rolled `nmp.toml` parser → `AppManifest { name, display_name, modules: { kernel, protocol, app } }`. |
| `/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-codegen/src/generate.rs` | Eight Rust file emitters (`Cargo.toml`, `lib.rs`, `action.rs`, `update.rs`, `envelope.rs`, `view_spec.rs`, `capability.rs`, `domain.rs`, `ffi.rs`). |
| `/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-codegen/src/ffi_gen.rs` | Per-app `FfiApp` Rust shell that calls `nmp_app_dispatch_action`. |
| `/Users/pablofernandez/Work/nostr-multi-platform/crates/nmp-codegen/tests/` | Three integration tests covering determinism, fixture round-trip, ffi dispatch. None exercise Swift output. |

### 1b. What the generator knows about Rust types

**Nothing.** It does not parse any Rust source. It does not load `syn`. It
does not enumerate fields on `KernelSnapshot` / `KernelMetrics` /
`ProfileCard` / etc. The output Rust files are produced by string templates
that take the manifest (a list of module crate names) and emit boilerplate
that re-exports each module's `Action`, `Update`, `ViewSpec`, `Store`
symbols. Field-level schema information is entirely absent.

This is the structural reason `nmp-codegen` emits no Swift today: there is
no schema layer to emit *from*.

---

## 2. What Rust types need to be reflected in Swift?

### 2a. Inventory of every hand-written Swift `Decodable`

Source: `/Users/pablofernandez/Work/nostr-multi-platform/ios/Chirp/Chirp/Bridge/KernelBridge.swift`
(2,054 LoC total; the Decodable block runs ~571–1988).

| # | Swift type (file:line) | Rust counterpart (file:line) | Visibility in Rust | Notes |
|---|---|---|---|---|
| 1 | `SnapshotEnvelope` (KernelBridge.swift:571) | `UpdateEnvelope::Snapshot` (`nmp-core/src/update_envelope.rs`) | `pub` | Tagged-union outer frame; codegen already emits a Rust mirror. |
| 2 | `KernelUpdate` (KernelBridge.swift:719) | `KernelSnapshot` (`nmp-core/src/kernel/types.rs:671`) | `pub(super)` | Top-level snapshot. Carries the `projections` map. |
| 3 | `SnapshotProjections` (KernelBridge.swift:931) | implicit — `HashMap<String, serde_json::Value>` populated at `nmp-core/src/kernel/update.rs:242-393` | (no type) | **Dotted-key registry** — see §3c. |
| 4 | `MentionProfileWire` (KernelBridge.swift:1107) | `MentionProfilePayload` (`nmp-core/src/kernel/types.rs:238`) | `pub(super)` | |
| 5 | `SettingsHubSummary` (KernelBridge.swift:1134) | `SettingsHubSummary` (`nmp-core/src/kernel/identity_state.rs:155`) | `pub(crate)` | |
| 6 | `GroupChatMessage` (KernelBridge.swift:1156) | `GroupChatMessage` (`nmp-nip29/src/projection/group_chat.rs:70`) | `pub` | |
| 7 | `GroupChatSnapshot` (KernelBridge.swift:1167) | `GroupChatSnapshot` (`nmp-nip29/src/projection/group_chat.rs:103`) | `pub` | |
| 8 | `DiscoveredGroup` (KernelBridge.swift:1186) | `nmp-nip29/src/action/discover.rs` | `pub` | |
| 9 | `DiscoveredGroupsSnapshot` (KernelBridge.swift:1208) | `nmp-nip29/...` | `pub` | |
| 10 | `ZapCount` (KernelBridge.swift:1235) | `nmp-nip57` projection | `pub` | |
| 11 | `ZapsAggregateSnapshot` (KernelBridge.swift:1245) | `nmp-nip57` projection | `pub` | |
| 12 | `DmRelayListSnapshot` (KernelBridge.swift:1265) | `nmp-nip17/src/dm_relay_list.rs` | `pub` | |
| 13 | `DmMessage` (KernelBridge.swift:1288) | `nmp-nip17/src/inbox.rs:83` | `pub` | |
| 14 | `DmConversation` (KernelBridge.swift:1307) | `nmp-nip17/src/inbox.rs:114` | `pub` | |
| 15 | `FollowEntry` (KernelBridge.swift:1330) | `nmp-app-chirp` (`FollowListProjection`) | `pub` | |
| 16 | `FollowListSnapshot` (KernelBridge.swift:1341) | as above | `pub` | |
| 17 | `DmInboxSnapshot` (KernelBridge.swift:1349) | `nmp-nip17/src/inbox.rs:143` | `pub` | Has custom `init(from:)` defaulting `remoteSignerUnsupported = false`. |
| 18 | `RelayDiagnosticsWireSub` (KernelBridge.swift:1394) | `nmp-core/src/kernel/relay_diagnostics.rs:109` | `pub(super)` | |
| 19 | `RelayDiagnosticsRow` (KernelBridge.swift:1412) | `nmp-core/src/kernel/relay_diagnostics.rs:51` | `pub(super)` | |
| 20 | `RelayDiagnosticsInterest` (KernelBridge.swift:1438) | `nmp-core/src/kernel/relay_diagnostics.rs:143` | `pub(super)` | |
| 21 | `RelayDiagnosticsSnapshot` (KernelBridge.swift:1449) | `nmp-core/src/kernel/relay_diagnostics.rs:155` | `pub(super)` | |
| 22 | `BunkerHandshake` (KernelBridge.swift:1472) | `BunkerHandshakeDto` (`nmp-core/src/actor/commands/identity.rs:44`) | `pub(crate)` | |
| 23 | `Nip46Onboarding` + `SignerApp` + `StageKind` (KernelBridge.swift:1502) | `Nip46OnboardingDto` (`nmp-core/src/actor/commands/identity.rs:234`) | `pub(crate)` | Has a nested struct + `String`-raw enum. |
| 24 | `LogicalInterestStatus` (KernelBridge.swift:1536) | `nmp-core/src/kernel/types.rs:311` | `pub(super)` | |
| 25 | `WireSubscriptionStatus` (KernelBridge.swift:1546) | `nmp-core/src/kernel/types.rs:297` | `pub(super)` | |
| 26 | `ThreadView` (KernelBridge.swift:1562) | `ThreadViewPayload` (`nmp-core/src/kernel/types.rs:246`) | `pub(super)` | |
| 27 | `AccountSummary` (KernelBridge.swift:1580) | `nmp-core/src/kernel/identity_state.rs:48` | `pub(crate)` | |
| 28 | `PublishQueueEntry` (KernelBridge.swift:1604) | publish snapshot | `pub(crate)`-ish | |
| 29 | `LastActionResult` (KernelBridge.swift:1623) | from `action_results` projection | `pub` | |
| 30 | `ActionStage` (KernelBridge.swift:1655) | `ActionStage` (`nmp-core/src/kernel/action_stages.rs:97`) | `pub` | `enum` with `tag = "stage"` snake_case serialization. |
| 31 | `ActionStageEntry` (KernelBridge.swift:1678) | `StageEntry` (`nmp-core/src/kernel/action_stages.rs:138`) | `pub` | |
| 32 | `PublishOutboxItem` (KernelBridge.swift:1711) | `PublishOutboxItem` (`nmp-core/src/kernel/types.rs:325`) | `pub(super)` | |
| 33 | `PublishOutboxRelay` (KernelBridge.swift:1743) | `PublishOutboxRelay` (`nmp-core/src/kernel/types.rs:357`) | `pub(super)` | |
| 34 | `OutboxSummary` (KernelBridge.swift:1765) | `OutboxSummarySnapshot` (`nmp-core/src/kernel/types.rs:382`) | `pub(super)` | |
| 35 | `RelayEditRow` (KernelBridge.swift:1789) | `RelayEditRow` (`nmp-core/src/kernel/identity_state.rs:116`) | `pub` | |
| 36 | `RelayRoleOption` (KernelBridge.swift:1809) | `relay_role_options()` in `nmp-core/src/actor/relay_roles.rs` | `pub` | |
| 37 | `WalletStatusData` (KernelBridge.swift:1819) | `WalletStatus` (`nmp-core/src/actor/commands/wallet.rs:79`) | `pub(crate)` | |
| 38 | `ProfileCard` (KernelBridge.swift:1834) | `ProfileCard` (`nmp-core/src/kernel/types.rs:141`) | `pub(super)` | |
| 39 | `ProfileDispatchSpec` (KernelBridge.swift:1861) | `nmp-core/src/kernel/types.rs:180` | `pub(super)` | |
| 40 | `ProfileAction` (KernelBridge.swift:1866) | `nmp-core/src/kernel/types.rs:201` | `pub(super)` | |
| 41 | `AuthorProfileSnapshot` (KernelBridge.swift:1878) | `AuthorViewPayload` (`nmp-core/src/kernel/types.rs:216`) | `pub(super)` | |
| 42 | `TimelineItem` (KernelBridge.swift:1891) | `TimelineItem` (`nmp-core/src/kernel/types.rs:91`) | `pub(super)` | Has custom `init(from:)` defaulting `isRepost: false`, `navTargetId: id`, `repostInnerContent: ""`. |
| 43 | `KernelMetrics` (KernelBridge.swift:1989) | `Metrics` (`nmp-core/src/kernel/types.rs:615`) | `pub(super)` | 42 primitive fields; pure flat record. |
| 44 | `RelayStatus` (KernelBridge.swift:2032) | `RelayStatus` (`nmp-core/src/kernel/types.rs:265`) | `pub(super)` | |

Plus, in `/Users/pablofernandez/Work/nostr-multi-platform/ios/Chirp/Chirp/Bridge/TimelineBlock.swift`:

| # | Swift type | Rust counterpart |
|---|---|---|
| 45 | `TimelineBlock` (TimelineBlock.swift:29) — enum | `nmp-app-chirp` content-tree projection |
| 46 | `ThreadPointer` (TimelineBlock.swift:98) — enum | as above |
| 47 | `ChirpEventCard` (TimelineBlock.swift:158) | as above |
| 48 | `ChirpTimelineSnapshot` (TimelineBlock.swift:177) | as above |
| 49 | `ContentTreeWire` (TimelineBlock.swift:186) | `nmp-app-chirp` content tree |
| 50 | `MediaKind` (TimelineBlock.swift:195) — `String`-raw enum | as above |
| 51 | `ContentWireNode` (TimelineBlock.swift:201) — enum | as above |
| 52 | `WireNostrUri` (TimelineBlock.swift:269) | NIP-21 |

### 2b. The discriminating constraint — visibility & dotted keys

Two facts shape the design:

1. **Most projection types are `pub(super)` or `pub(crate)` in `nmp-core`.**
   They cannot be reached from `crates/nmp-codegen/` as a downstream
   dependency. The names exist; the type symbols do not. Any approach that
   parses the schema *from outside `nmp-core`* must either (a) make the
   types `pub`, (b) duplicate them, or (c) execute the schema export *from
   inside `nmp-core` itself*.
2. **The `projections` map keys are not in any Rust type.** They are
   string literals at `nmp-core/src/kernel/update.rs:242-393` (`"wallet"`,
   `"bunker_handshake"`, `"nmp.nip29.group_chat"`, etc.). The
   `SnapshotProjections` struct in Swift is therefore not a *type
   reflection* — it is a *registry* mapping (json_key → Swift_field →
   Decodable_type). No `syn`-of-`types.rs` walk and no `schemars` derive
   on a single type can recover it. The current Swift `CodingKeys` enum
   spells the dotted keys out by hand
   (`KernelBridge.swift:1052-1091`), and an `XCTest` named
   `SnapshotProjectionsConformanceTests` is the only safety net.

---

## 3. Chosen approach

### 3a. ADR-0030 has already picked the lane

ADR-0030 §(b) is binding: ship a Swift `Decodable` emitter in `nmp-codegen`.
The four-way A/B/C/D framing in the V6 prompt is now narrower: ADR-0030
rules out **A** (UniFFI on the read surface — explicitly *out of scope* per
ADR-0030 §"Out of scope"), and the M14 deferral rules out **C**. The
remaining question is **what input format the emitter consumes** to produce
the Swift output. Three sub-variants of B remain:

| Variant | Input | How visibility is solved |
|---|---|---|
| **B-schemars** | JSON schemas exported by a `--features codegen-schema` build of `nmp-core` (and each NIP crate that owns a projection type) | `schemars::JsonSchema` derive runs *inside* the crate that owns the type, so `pub(super)` visibility is irrelevant. |
| **B-manifest** | Per-projection TOML / JSON manifest hand-written in `crates/nmp-codegen/projections/`, declaring `{ rust_type, json_key, fields: [{name, type, optional, default}] }` | Visibility irrelevant — manifest is the schema. |
| **B-syn** | `syn`-parse `nmp-core/src/kernel/types.rs` and friends | Requires either making types `pub` (a structural change with no benefit) OR re-implementing serde semantics by hand. Drift hazard. |

### 3b. Recommendation — **B-schemars**

**Pick B-schemars.** It satisfies D2 (the Rust struct IS the truth — no
parallel definition) and sidesteps `pub(super)` cleanly. The cost is one
optional `schemars` dependency on `nmp-core` and the small handful of NIP
crates that own projection types, behind a feature flag so production
builds stay unaffected.

Mechanism:

1. `nmp-core/Cargo.toml` (and `nmp-nip17`, `nmp-nip29`, `nmp-nip57`,
   `nmp-app-chirp`) grow:
   ```toml
   [features]
   codegen-schema = ["dep:schemars"]

   [dependencies]
   schemars = { version = "0.8", optional = true }
   ```
2. Existing serde derives gain a feature-gated sibling derive:
   ```rust
   #[derive(Clone, Debug, Serialize)]
   #[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
   pub(super) struct ProfileCard { ... }
   ```
3. A new bin target inside `nmp-core` — `src/bin/dump_projection_schemas.rs`
   — is compiled only when `--features codegen-schema` is on. It calls
   `schemars::schema_for!(T)` for every type listed in a registry function
   `pub fn projection_schema_registry() -> &'static [(&'static str,
   schemars::Schema)]` and writes the JSON to
   `target/nmp-codegen/projection-schemas.json`.
   The registry function ALSO encodes the dotted-projection-key →
   Rust-type-name mapping (the §2b registry that's nowhere on disk today).
4. `crates/nmp-codegen/src/swift.rs` (new) reads
   `projection-schemas.json` and emits Swift `Decodable` structs to
   `bindings/swift/KernelTypes.swift`.

This pattern avoids hand-writing field lists, makes the dotted-key registry
discoverable from one Rust function, and reuses serde-coherent JSON
representations (schemars composes with `serde(rename_all)`,
`serde(default)`, `serde(tag = "...")` etc.).

### 3c. Why **not** B-manifest

The TOML/JSON manifest is simpler to land in one PR (no new dep on
`nmp-core`, no feature flag, no schema export bin), and it's the right
escape hatch if `schemars` turns out to disagree with NMP's serde shape on
edge cases (tagged enums, `#[serde(default)]` defaults that come from
custom `Default` impls). I'd ship B-schemars by default and fall back to a
per-projection manifest entry only for types where `schemars::schema_for!`
produces the wrong shape. The plan below codes that fallback in.

### 3d. Why **not** B-syn

`syn` walks Rust tokens, not serde semantics. It cannot see
`#[serde(rename_all = "snake_case")]`, `#[serde(default)]`, or
`#[serde(tag = "stage", content = "...")]` without re-implementing serde
itself — exactly the drift hazard V6 wants to remove.

---

## 4. What does the generated Swift look like?

Goal: drop-in functional replacement for the hand-written Decodables.
Preserve *decode behavior on captured snapshot fixtures*, NOT
byte-identical text (the hand-written types carry Equatable / Identifiable
/ Hashable conformances that we do want to keep — see §4c).

### 4a. Example — `Metrics` (the pilot's flat-record case)

**Rust input** (`nmp-core/src/kernel/types.rs:615`, with the gated derive):
```rust
#[derive(Clone, Debug, Serialize)]
#[cfg_attr(feature = "codegen-schema", derive(schemars::JsonSchema))]
pub(super) struct Metrics {
    pub(super) generated_events: u64,
    pub(super) note_events: u64,
    // ... 40 more primitive fields ...
    pub(super) first_event_ms: Option<u128>,
    pub(super) max_event_to_emit_ms: u128,
    pub(super) max_events_per_update: u64,
    pub(super) dispatch_drops_total: u64,
    pub(super) claim_drops_total: u64,
}
```

**Generated Swift output** (`bindings/swift/KernelTypes.swift`):
```swift
// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.
// Regenerate via: cargo run -p nmp-codegen -- gen swift
// Source: crates/nmp-core/src/kernel/types.rs `Metrics`

struct KernelMetrics: Decodable, Equatable {
    let generatedEvents: UInt64
    let noteEvents: UInt64
    // ... 40 more ...
    let firstEventMs: UInt64?
    let maxEventToEmitMs: UInt64
    let maxEventsPerUpdate: UInt64
    let dispatchDropsTotal: UInt64
    let claimDropsTotal: UInt64
}
```

**Type mapping rules:**

| Rust | Swift |
|---|---|
| `u8`, `u16`, `u32` | `UInt32` (Swift host renders monotonic counts) |
| `u64`, `u128`, `usize` | `UInt64` (current hand-written convention) |
| `i32`, `i64`, `isize` | `Int` |
| `f32`, `f64` | `Double` |
| `String` | `String` |
| `bool` | `Bool` |
| `Option<T>` | `T?` |
| `Vec<T>` | `[T]` |
| `HashMap<String, V>` / `BTreeMap<String, V>` | `[String: V]` |
| `&'static str` | `String` |
| `#[serde(rename_all = "snake_case")]` on struct | rendered through `JSONDecoder.keyDecodingStrategy = .convertFromSnakeCase` at the call site (no per-field `CodingKey` needed) |

### 4b. Example — `ActionStage` (the tagged-enum case)

**Rust input** (`nmp-core/src/kernel/action_stages.rs:97`):
```rust
#[derive(Serialize, Deserialize)]
#[serde(tag = "stage", rename_all = "snake_case")]
pub enum ActionStage {
    Requested,
    AwaitingCapability,
    Publishing,
    Accepted,
    Failed { reason: String },
}
```

**Generated Swift:**
```swift
enum ActionStage: Decodable, Equatable {
    case requested
    case awaitingCapability
    case publishing
    case accepted
    case failed(reason: String)

    private enum CodingKeys: String, CodingKey { case stage, reason }

    init(from decoder: Decoder) throws {
        let c = try decoder.container(keyedBy: CodingKeys.self)
        switch try c.decode(String.self, forKey: .stage) {
        case "requested": self = .requested
        case "awaiting_capability": self = .awaitingCapability
        case "publishing": self = .publishing
        case "accepted": self = .accepted
        case "failed":
            self = .failed(reason: try c.decodeIfPresent(String.self, forKey: .reason) ?? "")
        default:
            throw DecodingError.dataCorruptedError(forKey: .stage, in: c,
                debugDescription: "unknown ActionStage discriminant")
        }
    }
}
```

The current hand-written version (KernelBridge.swift:1655) has an
`.unknown(raw:)` D1-forward-compat case; the emitter should preserve that
by adding an `unknown(raw: String)` arm by default (configurable via a
per-type `forward_compat = true` flag in the registry).

### 4c. Conformances — preserved verbatim, but data-driven

The hand-written Decodables carry `Equatable`, `Identifiable`, `Hashable`,
`Default`, sometimes `static let empty` literals. None of those are derivable
from the JSON schema. The emitter needs a small per-type config in the
registry:

```rust
pub fn projection_schema_registry() -> &'static [TypeBinding] {
    &[
        TypeBinding {
            rust_type: "nmp_core::kernel::types::Metrics",
            swift_name: "KernelMetrics",
            json_key: None,             // not a top-level projection key
            conformances: &["Decodable", "Equatable"],
            id_field: None,
            forward_compat_enum: false,
        },
        TypeBinding {
            rust_type: "nmp_nip29::projection::group_chat::GroupChatSnapshot",
            swift_name: "GroupChatSnapshot",
            json_key: Some("nmp.nip29.group_chat"),
            conformances: &["Decodable", "Equatable"],
            id_field: None,
            forward_compat_enum: false,
        },
        TypeBinding {
            rust_type: "nmp_nip17::inbox::DmMessage",
            swift_name: "DmMessage",
            json_key: None,
            conformances: &["Decodable", "Identifiable", "Equatable"],
            id_field: Some("id"),       // Identifiable.id wires to this field
            forward_compat_enum: false,
        },
        // ... 49 more rows ...
    ]
}
```

This is the registry that *also* solves §2b: the `json_key` column is the
dotted-projection-key map.

### 4d. Decode-tolerance / D1 forward-compat

The hand-written code has two tolerance patterns:

1. **Optional-field default** (`KernelBridge.swift:1361-1365`,
   `DmInboxSnapshot.remoteSignerUnsupported = false` when absent):
   automatically derivable from `Option<T>` in Rust → `T?` in Swift, with
   the call site free to coalesce. NO custom emitter logic needed if the
   Rust field is `Option<T>`. **If the Rust field is `T` and the desired
   Swift behavior is "default to X when absent", that's a Rust bug —
   change Rust to use `#[serde(default)]` and add `Default` if needed**.
   The emitter renders `T` (non-optional) and the field will hard-fail
   decode if absent. This forces the schema source of truth back to Rust,
   which is the whole point.
2. **`TimelineItem` field defaults** (`KernelBridge.swift:1960-1983`,
   `isRepost ?? false`, `navTargetId ?? id`, `repostInnerContent ?? ""`):
   the Rust `TimelineItem` has these as non-Option fields. The drift
   here is that *the Rust side ALWAYS emits them* (so the defaults are
   never reached), and the Swift side defaults them anyway for legacy
   snapshots. The emitter should:
   - Default to: every Rust `T` → Swift `T` (non-optional, hard fail).
   - Allow an explicit per-field override in the registry
     (`legacy_default = ".empty_string"`) for fields that *must* be
     optional on the Swift side for legacy-snapshot tolerance.
   - The migration step (§6 step 2) for `TimelineItem` writes the override
     for those three fields explicitly, so the change is auditable.

---

## 5. How is it wired into the build?

### 5a. Where the generator runs

**Not as an Xcode build phase.** Per the memory note on
`xcodegen pbxproj churn` and the broader hazard of editor-side mutations
to the project, Xcode build phases that mutate checked-in files are
banned.

**The generator runs in three places:**

1. **Developer command:** `cargo run -p nmp-codegen -- gen swift`
   (subcommand parallel to the existing `gen modules`). Regenerates
   `bindings/swift/KernelTypes.swift` from the projection schema.
2. **CI gate:** `cargo run -p nmp-codegen -- gen swift --check` runs in
   `.github/workflows/test.yml` (or a new `.github/workflows/codegen-drift.yml`
   modeled on the existing `ffi-drift.yml` at
   `.github/workflows/ffi-drift.yml:19-27`). Exits non-zero if the
   generated output differs from the checked-in file.
3. **Local pre-PR habit:** `make codegen` (or just the cargo invocation
   above) in the developer loop. Cheap — a `schemars` build of `nmp-core`
   with the feature flag is the only cost.

### 5b. Where the generated file lands

`/Users/pablofernandez/Work/nostr-multi-platform/bindings/swift/KernelTypes.swift`

The `bindings/swift/` path is named in `aim.md §5 lines 203–204` and
ADR-0030 §"Decision (b)" both. The file is **committed** to the repo so:

- iOS engineers without a Rust toolchain can still build Chirp.
- Code review can read the diff when a Rust field changes.
- The CI gate has something stable to diff against.

The Chirp iOS target includes the file directly. Xcode group path:
`ios/Chirp/Chirp/Bridge/Generated/KernelTypes.swift` as a *folder reference*
(or a symlink) to `bindings/swift/KernelTypes.swift`.

### 5c. Generated-file format requirements

- Header comment: `// THIS FILE IS GENERATED. DO NOT EDIT BY HAND.`
- Regenerate command shown in the header.
- Source-of-truth provenance per type (Rust crate + file).
- Deterministic output (sorted types, sorted fields-within-type per
  schema order, no timestamps). Same rule as `nmp gen modules --check`
  (`crates/nmp-codegen/src/lib.rs:10`).

### 5d. CI gate wiring (concrete)

New file `/Users/pablofernandez/Work/nostr-multi-platform/.github/workflows/codegen-drift.yml`:

```yaml
name: codegen-drift
on:
  push: { branches: [master] }
  pull_request: { branches: [master] }
jobs:
  swift-codegen-drift:
    runs-on: ubuntu-latest
    steps:
      - uses: actions/checkout@v4
      - uses: dtolnay/rust-toolchain@stable
      - name: Check Swift codegen is up to date
        run: cargo run -p nmp-codegen --features codegen-schema -- gen swift --check
```

---

## 6. Migration plan

### 6a. Sequencing principle

V6's principle ("zero hand-maintained Swift Decodables") is the **end state**,
not the first PR. The migration is staged because:

- The dotted-projection-key registry (§2b) is the *hardest* sub-problem and
  touches `SnapshotProjections` plus `KernelUpdate`'s 20+ computed
  accessors.
- The `schemars` feature flag must land in `nmp-core` AND every NIP crate
  that owns a projection — that's a tree-shaped Cargo change spanning ~6
  crates.
- The CI gate must land green from PR #1; we cannot start adding new
  generated types until the build-and-check loop is proven.

### 6b. Stage 1 — `nmp-codegen` learns to emit Swift (one PR, the pilot)

**Scope:** Prove the loop end-to-end with the lowest-risk subset.

1. Add `schemars` optional dep + `codegen-schema` feature to `nmp-core`.
2. Add `schemars::JsonSchema` derive (feature-gated) to a small set of
   types:
   - `Metrics` (42 flat primitive fields — proves field emission, optional
     handling, snake_case conversion)
   - `RelayStatus` (nested optionals + denied bool — proves optional
     coverage)
   - `LogicalInterestStatus`, `WireSubscriptionStatus`, `AccountSummary`,
     `RelayEditRow`, `RelayRoleOption` (5 more leaf types — proves
     `Identifiable` + `id_field` registry config)
3. Add `src/bin/dump_projection_schemas.rs` to `nmp-core` (writes
   `target/nmp-codegen/projection-schemas.json`).
4. Add `crates/nmp-codegen/src/swift.rs` and a `gen swift` subcommand
   that reads the JSON and emits Swift to `bindings/swift/KernelTypes.swift`.
5. Commit the generated file. Delete the seven corresponding hand-written
   types from `KernelBridge.swift` (~200 LoC removed). Compile-check Chirp
   (the seven types are renamed-not-moved; existing imports adjust).
6. Land `.github/workflows/codegen-drift.yml` from §5d.
7. **PR exit criteria:** Chirp builds, all existing tests pass,
   `SnapshotProjectionsConformanceTests` passes (the six dotted-key
   projection types are STILL hand-written — Stage 1 does not touch
   `SnapshotProjections`).

Net: ~200 LoC removed, the CI gate is live, the loop is proven, the
hardest sub-problem is deferred to Stage 2 where it belongs.

### 6c. Stage 2 — Add the dotted-projection-key registry

**Scope:** Replace `SnapshotProjections` and its `CodingKeys` enum.

1. Extend the registry function to enumerate the eight kernel-owned dotted
   keys (`wallet`, `bunker_handshake`, `nip46_onboarding`, `publish_queue`,
   `publish_outbox`, `outbox_summary`, `relay_edit_rows`,
   `relay_role_options`, `accounts`, `active_account`, `action_results`,
   `action_stages`, `last_action_result`, `profile`, `timeline`,
   `author_view`, `thread_view`, `inserted`, `updated`, `removed`,
   `relay_diagnostics`, `mention_profiles`, `settings_hub`) and the seven
   host-registered ones (`nmp.nip29.group_chat`, `nmp.nip29.discovered_groups`,
   `nmp.nip17.dm_inbox`, `nmp.nip17.dm_relay_list`, `nmp.follow_list`,
   `nmp.nip57.zaps`).
2. The emitter generates `SnapshotProjections` + its `CodingKeys` enum
   from the registry. Each entry produces one Swift property and one
   `CodingKeys` case with the correct post-`.convertFromSnakeCase` raw
   value.
3. The 23 hand-written types (the `pub(super)` projection types in
   `nmp-core` + the four `pub` types in `nmp-nip17` / `nmp-nip29` /
   `nmp-nip57`) gain the `JsonSchema` derive. The 23 corresponding Swift
   types are removed from `KernelBridge.swift`.
4. `KernelUpdate`'s 20+ computed accessors (`var walletStatus`,
   `var publishQueue`, `var profile`, `var groupChat`, etc. at
   `KernelBridge.swift:762-920`) stay hand-written — they're forwarding
   shims (`projections?.wallet`), not Decodable shapes.
   *Stretch:* the registry could emit those too. Defer.
5. Update `SnapshotProjectionsConformanceTests.swift` ONLY if a new key is
   wired by Stage 2 itself (none expected — Stage 2 is migration, not new
   features).
6. **PR exit criteria:** Chirp builds, `SnapshotProjectionsConformanceTests`
   passes against generated `SnapshotProjections`, ~600 LoC removed from
   `KernelBridge.swift`.

### 6d. Stage 3 — Sweep up the remaining hand-written types

**Scope:** `KernelUpdate` (top-level), `TimelineItem`,
`DmInboxSnapshot.init(from:)` legacy default, `BunkerHandshake`,
`Nip46Onboarding` + `SignerApp` + `StageKind` enum, all
diagnostics types, `TimelineBlock` family in `TimelineBlock.swift`.

1. Each Rust type gains the `JsonSchema` derive.
2. Each registry entry lands.
3. `TimelineItem`'s three forward-compat fields use the `legacy_default`
   override flag (§4d).
4. `DmInboxSnapshot.remoteSignerUnsupported` becomes `Option<bool>` in
   Rust (the right fix — it's optional in spirit, the kernel emits it
   conditionally per V-08), and the Swift becomes `Bool?` with the call
   site coalescing to `false`.
5. `Nip46Onboarding.StageKind` String-raw enum is straightforward to
   emit; `ActionStage`'s tagged enum + `Failed(reason)` arm follows the
   pattern in §4b.
6. `TimelineBlock` (an internally-tagged enum) is the largest single test
   of the enum emitter — it lives in `nmp-app-chirp`, so the `JsonSchema`
   derive moves to that crate behind the same feature.
7. **PR exit criteria:** `KernelBridge.swift` carries ZERO hand-written
   `Decodable` types. `TimelineBlock.swift` is gone (replaced by the
   generated file or merged into `KernelTypes.swift`). The
   `SnapshotProjectionsConformanceTests` covers every key.

### 6e. Stage 4 — Test coverage extension

After Stage 3, extend `SnapshotProjectionsConformanceTests` (and a new
`KernelTypesConformanceTests` companion) to:

1. Round-trip every generated type against a Rust-emitted JSON fixture
   produced by `serde_json::to_value(<type>)` from a `nmp-core` unit test
   that captures one snapshot per type into
   `tests/fixtures/projection-samples/<type>.json`.
2. The Swift test loads each fixture, decodes with the same JSONDecoder
   config `KernelHandle.decode` uses, and asserts no field is silently
   nil.
3. This is the regression net that catches "Rust added a field, codegen
   added it to Swift, Swift consumer never updated" — the new failure
   mode the emitter introduces.

---

## 7. PR structure

**Three PRs.** Stage 1 is the pilot; Stages 2 and 3 each get their own PR;
Stage 4 piggybacks on Stage 3.

| PR | Net LoC | Risk | Independent? |
|---|---|---|---|
| **PR-V6-A** (Stage 1) | -200 hand-written Swift; +schemars dep; +emitter; +CI gate | LOW — touches 7 leaf types, no dotted-key complication, no test changes | yes (lands first) |
| **PR-V6-B** (Stage 2) | -600 hand-written Swift; +registry expansion to dotted keys | MEDIUM — `SnapshotProjections` migration, the conformance test is the critical safety net | depends on PR-V6-A |
| **PR-V6-C** (Stage 3 + 4) | -400 hand-written Swift; +`KernelUpdate` + `TimelineItem` + enums + `TimelineBlock`; conformance test extension | MEDIUM — multiple custom-decoder migrations | depends on PR-V6-B |

PR-V6-A alone removes >200 LoC of hand-written Swift, proves the approach,
lands the CI gate, and gives the team a working pattern. The bigger PRs
follow.

---

## 8. Risks and mitigations

### 8a. Decode-behavior drift

**Risk:** Generated Swift decodes a captured snapshot differently from
the hand-written version, silently losing data.

**Mitigation:**

1. Stage 4 test extension is the primary defense — every type gets a JSON
   fixture round-tripped through both the hand-written code (in the PR
   that *deletes* it, prior to deletion) and the generated code, with
   `XCTAssertEqual` on the decoded values.
2. The `SnapshotProjectionsConformanceTests` at
   `ios/Chirp/ChirpTests/SnapshotProjectionsConformanceTests.swift:50`
   already catches the silent-nil class of bug for `SnapshotProjections`.
   Stage 2 must run green against the generated `SnapshotProjections`.
3. Run Chirp manually (per the user's `run` skill or
   `mcp__xcode__build_run_sim`) on a real device after each stage and
   sanity-check that the feed renders, the DM inbox loads, the diagnostics
   screen populates. The conformance test catches schema drift; manual
   verification catches semantic drift.

### 8b. `schemars` disagrees with serde on tagged enums / `serde(default)`

**Risk:** `schemars` and serde have slightly different defaults on tagged
enums and `#[serde(default)]` semantics. A `schemars`-derived schema for
`ActionStage` (the `#[serde(tag = "stage", rename_all = "snake_case")]`
case) might not match the actual JSON shape.

**Mitigation:** The pilot (Stage 1) deliberately picks types that have NO
tagged-enum or rename complications — `Metrics` is 42 flat fields, the
five other pilot types are flat records. Tagged enums (`ActionStage`,
`TimelineBlock`) land in Stage 3 with a per-type test fixture asserting
the generated decoder accepts kernel-emitted JSON byte-for-byte. If
`schemars` produces the wrong shape for any type, fall back to a per-type
manifest override (§3c) — the registry already supports that escape hatch.

### 8c. `pub(super)` types don't acquire `JsonSchema` without crate-internal access

**Risk:** Adding `#[derive(schemars::JsonSchema)]` to a `pub(super)` type
in `nmp-core` requires the trait to be reachable. Since the `JsonSchema`
trait comes from `schemars` and the derive lives in the same crate, this
is fine — but the `dump_projection_schemas` bin needs `pub use` from one
of `nmp-core`'s internal modules. The cleanest path: a feature-gated
`pub mod codegen_schema { ... }` module in `nmp-core/src/lib.rs` that
`pub use`-re-exports every projection type *only when the feature is on*.

**Mitigation:** Land that feature-gated module in PR-V6-A. The
public-API impact in non-codegen builds is zero (the `cfg_attr` removes
the re-exports).

### 8d. iOS test target file inclusion

**Risk:** Adding `bindings/swift/KernelTypes.swift` to the Chirp iOS
target without `xcodegen`-induced project.pbxproj churn. Per memory
`xcodegen-pbxproj-churn`, running `xcodegen generate` rewrites
`project.pbxproj` with churn even when nothing structural changes.

**Mitigation:** Add the file via Xcode's "Add Files to…" dialog ONCE
during PR-V6-A, commit the minimal `project.pbxproj` delta, and never
re-run `xcodegen` against the bindings folder. Use a folder reference
(blue folder) instead of a group reference (yellow folder) so subsequent
file additions to the generated bindings don't require a project change.

### 8e. The CI gate fails on macOS-only builds for `schemars`

**Risk:** `schemars` is a build-time-only dependency, but its proc-macro
crate must be buildable in CI. `dtolnay/rust-toolchain@stable` already
covers this.

**Mitigation:** None additional; this is a stock Rust setup.

### 8f. Generated file pollutes `git blame`

**Risk:** Every Rust schema change produces a Swift diff in the same PR.
That's the *whole point*, but a noisy review.

**Mitigation:** Configure `bindings/swift/KernelTypes.swift` in
`.gitattributes` as `linguist-generated=true` so GitHub collapses the
diff in PRs by default, and add the file to `.git-blame-ignore-revs`
configuration on the regeneration commits.

---

## 9. Out of scope (do NOT do in V6)

- Kotlin / TypeScript emitters (ADR-0030 §"Out of scope" — follow-on once
  the Swift pattern is validated).
- UniFFI migration on the write surface (M14).
- WASM bindings (separate structural problem per V-01 / direction review
  #74).
- Refactoring the `pub(super)` visibility of projection types into a
  proper `pub` API. The feature-gated re-export module is sufficient and
  preserves the existing encapsulation.
- Replacing `KernelBridge.swift`'s pointer-wrangling FFI code. Only the
  Decodable block is in scope.

---

## 10. Definition of done

After PR-V6-C lands:

1. `KernelBridge.swift` has **zero** top-level `struct ... : Decodable`
   declarations. Every Decodable / Codable type the bridge uses imports
   from `bindings/swift/KernelTypes.swift`.
2. `TimelineBlock.swift` is **deleted**. Its types live in the generated
   bindings.
3. `cargo run -p nmp-codegen --features codegen-schema -- gen swift --check`
   returns 0 against `master`.
4. The `codegen-drift` CI workflow is required by branch protection on
   `master`.
5. Adding a single field to a Rust projection type and re-running `gen
   swift` produces a Swift diff and nothing else. The next PR that uses
   that field in the iOS shell compiles; a PR that skips the regen step
   fails CI.
6. `SnapshotProjectionsConformanceTests` + the new
   `KernelTypesConformanceTests` cover every registry entry. A registry
   entry without a fixture fails CI.

The hand-mirror pattern is structurally gone, not just patched.
