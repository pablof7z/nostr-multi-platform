# Opus Direction Review #15 — Snapshot as Broadcast, not Subscription
_2026-05-24_

> Code-grounded against HEAD (workspace dirty). No bug-spelunking; this is a
> direction call. Citations use `file:line` so the next reviewer can
> re-verify in one open.

## TL;DR

- **Reverse the snapshot fan-broadcast model.** `kernel/update.rs:266+`
  unconditionally inserts ≥14 projection keys into every 4 Hz tick whether or
  not any view is open against them. aim.md §7 Q1+Q2 ("state granularity
  across FFI", "where do views live") explicitly listed this as unresolved;
  the codebase silently chose "snapshot everything" with no ADR. Until that
  reverses, every thin-shell win adds cost and the per-tick payload grows
  monotonically with feature count.
- **Add a host-declared projection primitive.** A second-app developer
  cannot say *"give me a list of kind:30023 by these authors, ordered by
  `created_at`"* without writing a Rust `ActionModule` + closure-based
  `register_snapshot_projection`. Notes (`apps/notes/`) bypasses the
  framework entirely (`register_raw_event_observer([1])` + JSON in Swift)
  because the framework has nothing more declarative to offer. **This is
  the v1-B framework gap, not codegen and not WASM.**
- **Stop calling host-baked-in projections "kernel-owned."** `outbox_summary`
  (English title + subtitle), `accounts.npub_short`, `settings_hub`
  ("N relays" pluralization), `relay_role_options` are Chirp UI nouns
  living in `nmp-core`. The doctrine reads "no app nouns in nmp-core"
  (D0); the implementation reads "no app nouns in *typed fields*, the
  projection bag is exempt." That is a silent renegotiation.
- **Replace deliberate display-helper duplication with one
  `nmp-display` substrate crate.** `format_ago_secs` / `avatar_color_hex`
  are byte-duplicated across `nmp-nip17`, `nmp-nip29`, `nmp-marmot` on the
  rule "a NIP crate must not depend on another NIP crate." V-25 already
  caught the algorithm *drifting* (group-chat avatars rendered with
  different tint than DM avatars). The right answer is a tiny shared crate
  every NIP depends on. The doctrine corollary is wrong.
- **Notes is the framework with a missing rung, not a proof.**
  PD-033-A was re-opened by review #13 for the right reason
  (raw tap + Swift JSON + Swift ordering). Don't try to "rewrite Notes
  properly" — ship the host-declared-projection primitive first, then
  Notes rewrites itself in ~60 LOC Swift.

## What NMP should add (that it doesn't have)

### 1. Host-declared filtered, ordered projections (`nmp.view.declare`)

Today a host that wants a typed timeline has exactly two options:

1. **Implement an `ActionModule` + closure** registered through
   `NmpApp::register_snapshot_projection` (`crates/nmp-core/src/ffi/mod.rs:841`).
   The closure runs every tick inside the actor (`update.rs:273`) and must
   build the projection from raw kernel state by hand. The closure-based
   seam requires a Rust per-app crate; Swift cannot use it.

2. **Tap raw events via `nmp_app_register_raw_event_observer`** (what
   Notes does — `apps/notes/ios/Notes/Bridge/NotesBridge.swift:74`) and
   parse/order/dedupe in Swift. Bypasses D3 outbox routing,
   thin-shell, replaceable-event semantics, and snapshot cadence — every
   anti-pattern the framework exists to prevent.

There is no third option that is both (a) protocol-aware and (b)
host-declarable from Swift/JS. The missing call shape:

```
nmp_app_open_view(app, namespace, json_filter) -> view_handle
// snapshot tick now carries projections["<namespace>:<handle>"]
//   = { items: [...], inserted: [...], updated: [...], removed: [...] }
```

The filter shape mirrors `nostr::Filter` (kinds, authors, `#e`/`#p` tags,
since/until, limit) and routes through the planner and D3 outbox — so a
second-app developer gets correct relay routing, replaceable-event
semantics, and bounded subscriptions for free.

This is the v1-B framework gap. Notes (~96 LOC bridge) only got there by
*not* using the framework; if it had to use it, the bridge would be the
Chirp bridge (1,887 LOC).

### 2. The projection-handle subscription protocol from aim.md §7 Q3

aim.md §7 Q3 lists this as one of the seven open design questions:

> Reactive cross-FFI subscription protocol. UniFFI gives callback
> interfaces, not native reactive streams. … Define a single
> `Subscription` opaque handle + reconciler-style callback that adapts
> cleanly per platform.

V-12 says production splits of `actor/mod.rs` (1,490 LOC), `dispatch.rs`
(1,477), `kernel/mod.rs` (1,396) are post-v1. They are post-v1 because
nobody can decompose them without a view-handle abstraction — every
`ActorCommand` variant carries the implicit assumption that the snapshot
projects everything every tick. The handle is the structural fix; the LOC
violations are the symptom.

### 3. A `Filter` action surface

`PublishRaw { kind, tags, content, target }`
(`crates/nmp-core/src/publish/action.rs:137`) closed the generic-write
gap that review #77 §2 flagged. Match it on the read side: instead of
the 11 bespoke `nmp_app_open_*` symbols (`open_timeline`, `open_author`,
`open_thread`, `open_firehose_tag`, `open_uri`, the four
`*_register_*` projection variants), have one `nmp.view` dispatch
namespace that accepts a declarative filter. Symmetry with publish.

## What NMP should stop doing (that it currently does)

### 1. Stop emitting every projection every tick

`update.rs:266–399` — `snapshot_projections_with_publish_cluster` inserts
14 keys unconditionally per tick: `publish_queue`, `publish_outbox`,
`outbox_summary`, `relay_edit_rows`, `relay_role_options`, `settings_hub`,
`accounts`, `active_account`, `profile`, `timeline`, `author_view`,
`thread_view`, `inserted`, `updated`, `removed`. The actor thread runs
this every 4 Hz regardless of whether the host has any view open against
any of them. `thread_view` is non-null only when a thread is open;
nothing tells the kernel a thread is *not* open before serialization.

This is the dual of D8 ("≤60Hz/view") on the *emission* side. D8 limits
how often a view re-emits; nothing limits which views participate in
emission. Snapshots are O(open-views) by aim.md §6 ("Snapshots bounded by
open views"); today they are O(everything-Chirp-might-want).

Concrete fix shape: a host registers projections with a *visibility
gate* — a cheap predicate (closure on Rust side; Swift can fire
`nmp_app_view_visible(handle, bool)` on `onAppear` / `onDisappear`). The
kernel skips projection bodies for invisible handles. Saves serde work
and bandwidth; preserves thin-shell because the kernel still owns the
computation when the gate opens.

### 2. Stop calling Chirp-specific projections "kernel-owned built-ins"

`outbox_summary` (`OutboxSummarySnapshot` — `crates/nmp-core/src/kernel/types.rs:404`)
carries pre-formatted English `title` / `subtitle` strings and per-status
counters. `settings_hub` builds the string "N relays" with English
pluralization (`kernel/mod.rs:209`). `accounts.npub_short` truncates a
bech32 string for Chirp's account picker. `relay_role_options` is the
Chirp relay-role picker's pre-rendered label/tint list.

These are all *correct per the thin-shell rule* — and exactly wrong per D0
("kernel never grows app nouns"). The doctrine has been silently
renegotiated from "no app nouns in nmp-core" to "no app nouns in typed
`KernelSnapshot` fields; the projection bag is exempt." Either:

- Move these to `apps/chirp/nmp-app-chirp` and register them through
  `register_snapshot_projection` (the seam already exists, it just
  isn't used for built-ins because the kernel ergonomics for cross-crate
  projection state are worse than just inlining); OR
- Honestly amend D0: "kernel may carry projections required by the iOS
  Chirp product; new app crates must not."

The current state is the worst of both — auditors keep checking typed
fields for D0 violations (V-03, V-07) and missing the projection-bag
back door, which is now 14 keys wide and growing.

### 3. Stop the "deliberate byte-identical duplication" of display
helpers

`format_ago_secs` and `avatar_color_hex` are duplicated across
`crates/nmp-nip17/src/display.rs:74,97`, `crates/nmp-nip29/src/projection/group_chat.rs`,
and `crates/nmp-marmot/.../projection/display.rs`, each carrying a
comment that a NIP crate must not depend on another NIP crate.

V-25 (BACKLOG section, marked DONE) records exactly the failure mode this
rule guarantees: the algorithm *drifted* — same author, different avatar
tint in DMs vs. group chat. The fix was "duplicate again, more
carefully, with a pinned-vector test." That is treating a symptom of a
wrong rule.

The right fix: a `nmp-display` (or `nmp-substrate-format`) crate at
substrate tier (below all NIP crates). Single owner, single algorithm.
The "no NIP-on-NIP deps" rule remains. The reason it broke is that the
formatters were never NIP-protocol concerns to begin with — they are
display-substrate concerns that happened to be co-located with the first
NIP crate to need them.

## The biggest thing that could be fundamentally better

**The snapshot is a broadcast, not a subscription.** Everything else
follows.

Look at `kernel/update.rs:266`. The kernel does not know which projections
the UI is currently rendering against. It serializes all of them every
tick because that is the only way to be correct under "Rust is the single
source of truth." Two consequences:

1. **Performance scales with surface area, not view depth.** Adding a new
   social feature (say, a kind:9735 zap-receipts feed) means a new
   `register_snapshot_projection` closure that runs every tick forever,
   contributing JSON to every snapshot, parsed by every host, whether or
   not zap receipts are anywhere on screen. The 4Hz tick is fine; the
   "build every projection every tick" model is what makes it expensive.

2. **The thin-shell rule cannot scale.** Each PR moves another display
   string into Rust (V-20, V-22, V-23, V-24, V-25 in the last sprint).
   Each one *correctly* moves work to Rust under thin-shell. Each one
   *necessarily* adds bytes to every snapshot of every app. The
   discipline holds the line on quality and pushes against the broadcast
   model from the supply side; eventually the broadcast model breaks
   first.

The fundamental shift is from **"snapshot is a full mirror of kernel
state"** to **"snapshot is a multiplex of currently-open view handles."**
Concretely:

- Host opens a view → kernel allocates a handle, registers the
  projection's input dependencies (event-store filter, settings keys,
  identity slot).
- Tick: kernel walks open handles only, recomputes each, emits a
  per-handle delta (re-emit unchanged handles as `{handle, unchanged: true}`
  — single-byte heuristic).
- Host closes view → kernel drops the handle and stops computing.

aim.md §7 Q1+Q2 listed this explicitly:

> State granularity across FFI. Full-state snapshots are clean but
> expensive for large stores. … Where do views live? (a) Materialized in
> AppState, (b) lazy with ViewHandle opaque references the UI
> subscribes to, (c) computed in platform code. Bible rules out (c).
> **Pick between (a) and (b)** — leaning (b) for efficiency, but it
> complicates the FFI surface.

We picked (a) implicitly by accretion and never wrote the ADR. Picking
(b) deliberately is the structural shift that unlocks v1-B framework
viability — and incidentally lets `kernel/mod.rs` decompose, lets the
14-key projection bag get smaller instead of monotonically larger, and
makes the thin-shell rule sustainable.

## The one bet I'd reverse

**"Snapshots by default, granular updates as optimization."** (aim.md §2,
doctrine #10.)

This is the load-bearing decision under everything above. It was the right
default at M0 when there was one view, one host, one product. It is now
the proximate cause of:

- 14 projections × every tick × every app
- KernelBridge.swift at 1,887 LOC (handwritten Codables for 14 projections)
- The 1,988-LOC F-05 codegen project to mechanize the 1,887 LOC
- The "Swift triple-parses every snapshot" perf finding in review #77
- Notes choosing to **not use the framework** rather than pay the
  Codable-mirror tax for one feed

The reverse: **handle-based granular updates by default, full snapshot as
the cold-start / `Reset` exception.** The bible's commandment #10 is
about not exposing per-event callbacks; a handle that re-emits on tick
when its inputs change is not a per-event callback — it is the bounded
projection §6 anti-pattern #5 (native-side caching of derived values)
was warning against, owned correctly on the Rust side.

If I'm wrong about this in 18 months it will be because UniFFI's
callback story for per-handle deltas turned out to be worse than the
JSON-blob model. That is a real risk. But the current model has already
generated a 1,887-LOC bridge and a codegen sub-project to mechanize the
bridge — the JSON-blob model has not paid off.

## What I would NOT change

### 1. The 4Hz tick cadence itself

Earlier reviews keep eyeing the 4Hz rate. It is the right answer:
fast enough that publish settles feel instant, slow enough that
projection cost is amortized over a perceptible interval, and aligned
with iOS display refresh boundaries. The problem is what gets done *per
tick*, not how often the tick fires. Don't touch the cadence; touch the
work-per-tick model.

### 2. The PD-033-A re-opening (review #13)

Review #13 was right to re-open this. Don't try to "fix Notes" as the
work item — Notes is fine *as evidence of where the framework draws
the line*. The work item is the host-declared-projection primitive
("What NMP should add" §1 above); Notes rewrites itself in ~60 LOC
once that ships. Trying to rewrite Notes first produces a second Chirp
(the only way Notes consumes anything more than raw taps today is to
become Chirp-shaped). That isn't the framework win.

### 3. The 71-symbol C-ABI freeze + ADR override gate

The deprecation calendar (PD-039) and `ci/check-ffi-surface-freeze.sh`
are doing what they should. The surface is bounded; net-additions
require an ADR; the 16 migration-debt symbols have a roadmap. This is
load-bearing infrastructure that should not be revisited.
