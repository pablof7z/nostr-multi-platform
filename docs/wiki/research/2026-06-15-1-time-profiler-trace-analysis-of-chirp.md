---
type: research-record
date: 2026-06-15
session: c9a794f6-6ad7-4ee9-a620-fc342fd495c3
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/c9a794f6-6ad7-4ee9-a620-fc342fd495c3.jsonl
source_lines: 441-575
agent_attribution: a81cdde163a5a0e00
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Time Profiler trace analysis of Chirp iOS app (1,062 samples, 1.82s) identifying CPU hotpaths; verdict: SubscriptionCompiler::compile_with_context (205ms, 19.3%) is dominant Rust hotspot, SwiftUI AttributeGraph (223ms, 21%) is dominant UI hotspot, root cause is unconditional full subscription recompilation + full snapshot serialization on every actor tick cascading into SwiftUI invalidation"
captured_at: 2026-06-15T08:00:16Z
---

Time Profiler trace analysis of Chirp iOS app (1,062 samples, 1.82s) identifying CPU hotpaths; verdict: SubscriptionCompiler::compile_with_context (205ms, 19.3%) is dominant Rust hotspot, SwiftUI AttributeGraph (223ms, 21%) is dominant UI hotspot, root cause is unconditional full subscription recompilation + full snapshot serialization on every actor tick cascading into SwiftUI invalidation

---

User: [Agent task result: Agent "Analyze Chirp iOS performance trace" completed]
Now I have a comprehensive picture. Let me compile the final analysis report.

---

## Time Profiler Trace Analysis — Chirp iOS (io.f7z.chirp)

**Trace metadata:** 1.82s deferred recording on iPhone 17 Pro Max (iOS 26.6), app process `Chirp7z` pid 30402. 1,062 CPU samples captured. Note: deferred mode causes all samples to report with an "unknown" thread name — thread identity was recovered from stack entry points.

---

### CPU Layer Attribution (across all threads)

| Layer | CPU time | % |
|---|---|---|
| SwiftUI / AttributeGraph rendering (main thread) | ~412ms | 38.8% |
| **Rust actor loop** (`run_actor_with_observers`) | **~335ms** | **31.5%** |
| Empty stacks (sleeping/idle threads) | ~247ms | 23.3% |
| FFI snapshot callback (`nmpUpdateCallback` + FlatBuffer decode) | ~57ms | 5.4% |
| Other background threads (networking, GCD, MLS) | ~11ms | 1.0% |

No hang detector triggers fired. The app was not technically hung during the 1.82s window — this is pure CPU burn profile.

---

### Top 7 Hot Call Stacks

**#1 — 205ms (19.3%): Subscription recompile → `SubscriptionCompiler::compile_with_context`**

```
thread_start → run_actor_with_observers → drain_lifecycle_tick
  → SubscriptionLifecycle::drain_tick_with_lookup
    → SubscriptionLifecycle::recompile_and_diff_with_lookup
      → SubscriptionCompiler::compile
        → SubscriptionCompiler::compile_with_context
          → [BTreeMap ops on InterestShape / lattice::merge / rule2_tags]
```

This is the single hottest call stack. The `compile_with_context` function itself burns ~100ms on `BTreeMap` traversal of `InterestShape` (the struct that holds `BTreeSet<Pubkey>`, `BTreeSet<u32>` kinds, and `BTreeMap<TagKey, BTreeSet<String>>` tags). `lattice::merge` accounts for another ~31ms (merging two `InterestShape` structs involves cloning nested `BTreeSet`s). The `InterestShape::drop` (destructor freeing those trees) is a visible 5ms leaf. **This is the dominant Rust hotspot.**

**#2 — 223ms (21.0%): SwiftUI AttributeGraph update (UI diff)**

```
UIKitCore → SwiftUI → AG::Graph::UpdateStack::update()
  → AG::Graph::update_attribute / AG::Subgraph::update / AG::Graph::propagate_dirty
    → NostrAvatar.body.getter → ChirpColor.avatar(from:)
    → [SwiftUICore layout/render passes]
```

The `AG::Graph::UpdateStack::update()` subtree accounts for the majority of main-thread CPU. `NostrAvatar.body.getter` is the only app-owned frame caught in a leaf, and it calls `ChirpColor.avatar(from:)` which recomputes a `LinearGradient` on every body evaluation. This is expected to fire whenever the Rust kernel emits snapshot updates (since `NostrAvatar` reads `profileHost?.profile(forPubkey:)`).

**#3 — 99ms (9.3%): Rust actor → `make_update` → `snapshot_projections_with_publish_cache`**

```
run_actor_with_observers → dispatch_command → maybe_emit_after_dispatch
  → Kernel::make_update
    → Kernel::snapshot_projections_with_publish_cache
      → SnapshotRegistry::run / SnapshotRegistry::run_typed
        → MarmotProjection::messages_since → mdk_core::groups (SQLite)
          → sqlite3_step → sqlite3PagerSharedLock [kernel syscall]
```

Every command dispatch triggers `make_update` which runs the full snapshot. The leaf for 26/99ms here is `0x2388ba835` in `libsystem_kernel.dylib` — a `pread`/page-fault syscall from SQLite's WAL shared-lock. The Marmot (MLS) projection `messages_since` performs a SQLite query on every snapshot cycle, even when there are no new messages.

**#4 — 57ms (5.4%): FFI snapshot callback → FlatBuffer decode**

```
thread_start → nmp_app_new (Rust spawn) → nmpUpdateCallback (Swift closure)
  → KernelHandle.decodeFlatBuffer(bytes:count:cache:)
    → KernelUpdateFrameDecoder.decode / extractTypedProjections
      → FlatbufferVector.subscript → Data.init<Sequence> (copy)
        → swift_beginAccess [ARC exclusivity check]
```

The FlatBuffer decode path is called on every kernel snapshot emission. The hot leaf is `swift_beginAccess` (Swift 5 exclusive-access runtime enforcement) and `initializeWithCopy for ByteBuffer.Storage.Blob`, indicating each `FlatbufferVector.subscript` call triggers a copy of `ByteBuffer.Storage.Blob`. The `Sequence._copySequenceContents → Data.init<A>` pattern suggests projection arrays are being materialized into `Data` heap buffers on each decode.

**#5 — 37ms (3.5%): OpenMLS / Marmot SQLite group load**

```
run_actor_with_observers → [snapshot path]
  → mdk_core::groups::MDK::get / load
    → openmls::MlsGroup::load → mdk_sqlite_storage
      → rusqlite rows → sqlite3_step [kernel syscall]
```

`MlsGroup::load` calls SQLite on every snapshot — loading the entire MLS group state from disk each cycle rather than keeping it in memory.

**#6 — 26ms (2.5%): `serde_json::Value` drop path**

```
nmp_core → drop_glue for serde_json::value::Value
```

20ms leaf weight on `core::ptr::drop_glue<serde_json::value::Value>` — still-live `Value` allocations being freed in the Rust actor thread. This is a residual from the pre–#1082 era or from non-typed paths still returning `serde_json::Value` somewhere in the snapshot/relay pipeline.

**#7 — ~150ms: UIKit layout cascade (main thread)**

```
UIApplicationMain → GSEventRunModal → UIView._updateTraitCollectionIfNeeded
  → UIView._wrappedProcessTraitChanges (×4 recursive)
    → CA::Layer::update_if_needed_ → UIView.layoutBelowIfNeeded
      → UIView._executeUpdateProperties
```

Trait-collection propagation is recursing 4+ levels of `_wrappedProcessTraitChanges` on each layout pass. This is UIKit/SwiftUI overhead from the app-level trait environment, likely triggered by each new SwiftUI state update from the Rust kernel (each snapshot tick causes a SwiftUI state change, which invalidates traits and re-layouts).

---

### Key Findings

**The Rust side dominates pure compute; SwiftUI dominates wall time.**

1. **`SubscriptionCompiler::compile_with_context` (205ms)** is the single hottest function. The cost is almost entirely in cloning and traversing nested `BTreeSet`/`BTreeMap` collections inside `InterestShape`. Every drain tick recompiles every subscription from scratch. There is no observed short-circuit when nothing changed. This aligns with ADR-0055 Rung 3's stated goal of "omit-unchanged" — that optimization would directly eliminate most of this cost. The `InterestShape` struct uses heap-allocated `BTreeSet<String>` for pubkeys (64-char hex strings), which makes cloning/dropping expensive.

2. **`make_update` runs on every `dispatch_command` (99ms + 57ms FFI = 156ms combined)**. The full snapshot is serialized, FlatBuffer-encoded, and sent across the FFI boundary after every single command dispatch, including trivial UI interactions. The SQLite calls from `MarmotProjection::messages_since` execute on every cycle regardless of whether any MLS events arrived.

3. **`lattice::merge` (31ms)** is O(n) in the number of author pubkeys, cloning `BTreeSet<String>` at each merge step. If subscriptions contain large author sets (a contact list feed), this compounds quickly.

4. **FlatBuffer decode overhead (57ms)**: `ByteBuffer.Storage.Blob` is being copied per subscript call — the Swift `FlatbufferVector` is not zero-copy. Each projection array materializes a new `Data` heap allocation.

5. **SwiftUI AttributeGraph (223ms + 150ms UIKit = ~373ms)**: Every Rust snapshot delivery invalidates SwiftUI state, which triggers a full AttributeGraph diff. `NostrAvatar.body` is re-evaluated on each snapshot, calling `ChirpColor.avatar(from:)` (a `LinearGradient` construction) every time. With N avatars on screen and 4Hz+ updates, this multiplies. `NostrAvatar` does not use `Equatable` conformance on its inputs — SwiftUI cannot skip body re-evaluation.

---

### Root-cause hypothesis

The primary performance issue is **the combination of (a) unconditional full subscription recompilation on every actor tick and (b) full snapshot serialization + FFI round-trip on every command dispatch**, where both paths involve heavy BTreeMap/BTreeSet cloning inside `InterestShape`. Each relay event, user interaction, or timer fires `drain_lifecycle_tick` which recompiles all subscriptions from scratch and then immediately calls `make_update` which snapshots all projections (including a SQLite query for Marmot groups). This drives continuous CPU load on the Rust thread (~335ms/1062ms sampled) which cascades into continuous SwiftUI state invalidation (~373ms/1062ms), producing the characteristic 4Hz+ full-redraw pattern flagged as the highest-risk architectural bet in the codebase notes.

**The fixes that would have the most impact, in priority order:**

1. ADR-0055 Rung 3 "omit-unchanged": skip snapshot emission when projections haven't changed — this eliminates the cascading SwiftUI invalidation entirely for idle ticks.
2. Subscription recompile memoization: cache the compiled `InterestShape` lattice result per-subscription and only recompile when the subscription's input interest actually changes.
3. `MarmotProjection::messages_since` caching: do not re-query SQLite on every snapshot cycle; use a watermark + dirty-flag to skip unchanged groups.
4. `InterestShape` memory layout: replace `BTreeSet<String>` (heap-allocated hex pubkeys) with a more cache-friendly representation (e.g., sorted `Vec<[u8;32]>` parsed bytes) to reduce clone/drop cost in `lattice::merge`.
5. `NostrAvatar` equatable guard: add `Equatable` on `(pubkey, url, colorHex)` so SwiftUI can skip body re-evaluation when snapshot delivers the same profile data.
