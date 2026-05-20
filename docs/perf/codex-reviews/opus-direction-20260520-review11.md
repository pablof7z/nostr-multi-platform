# Opus direction review #11 — the developer experience of shipping on NMP

Reviews #9/#10 called a moratorium on architecture audits. This one obeys the
spirit of that: it does not re-litigate `ActionRegistry::reduce` being dead or
the bespoke-FFI verbs existing. It walks the path a developer actually takes
shipping an app on NMP *tomorrow*, citing only what is in the tree today.

## 1. "I want to build a new app on NMP. Where do I start?"

The entry point is `nmp init <app-name>` (`crates/nmp-cli/src/main.rs:5`,
`:32`). It scaffolds `nmp.toml` + a per-app FFI crate. `nmp gen modules`
(`crates/nmp-cli/src/gen.rs:71`) regenerates the FFI crate from the manifest.
That part works and is deterministic.

What confuses a developer first, in order:

- **The reference "second app" is a 12-line stub.** `apps/fixture/nmp-app-fixture/src/lib.rs`
  is pure re-exports. The dispatch shell `apps/fixture/nmp-app-fixture/src/ffi.rs:44-49`
  returns `UriRejected` for *every* module-projected action with the comment
  "module-projected action has no generated reducer; routing requires a
  module-reducer seam (see NMP-145)." A developer who copies the fixture as
  their template inherits a no-op for their own app domain. The codegen author
  states the cause plainly at `crates/nmp-codegen/src/generate.rs:160-163`:
  "the real NIP crates (`nmp-nip01` → `RepliesDomain`, `nmp-nip22` →
  `CommentsSpec`, …) do not [export `::Action`/`::Update`/`::ViewSpec`], so
  codegen has no live NIP-module consumer." The codegen pipeline has zero
  protocol-module consumers — only `fixture-todo-core` conforms.

- **Two different types are both named `KernelUpdate`.** `crates/nmp-core/src/kernel/types.rs:501`
  is a ~60-field snapshot *struct*. `crates/nmp-core/src/app.rs:39` is an
  *enum* with a `UriRejected` variant. The generated FFI crate uses the enum
  (`ffi.rs:44`); the actor's update channel serialises the struct. A developer
  grepping `KernelUpdate` on day one cannot tell which contract they hold.

- **Missing docs the plan promises.** `docs/plan/m16-cli-starter.md:13-15`
  commits to `docs/recipes/`, `docs/nips.md`, `docs/migration.md`. None exist
  (`docs/recipes/` absent; no `nips.md`/`migration.md`). The builder-guide
  walkthrough `docs/builder-guide/19a-walkthrough-microblog.md` is good prose
  but its example app (`apps/microblog/`) is not in the tree — it is a paper
  walkthrough, not a runnable starter.

Honest answer: a developer starts by copying `fixture-todo-core` (the one
conforming module) and the per-app FFI crate, and must already understand the
5 trait families before the scaffold does anything useful.

## 2. "Something went wrong in production. How do I debug it?"

This is where NMP is **strong**. The observability surface is genuinely rich
and well-shaped:

- `RelayStatus` (`kernel/types.rs:150-179`) exposes per-relay `connection`,
  `auth`, `reconnect_count`, `bytes_rx/tx`, `last_error`, `last_notice`, and
  crucially `error_category` with a *closed key set*
  (`auth_required|transient|permanent|malformed_event|policy_denied`,
  `:162-166`) and `last_close_reason` / `denied` (`:169-178`). A host can
  branch on error *class* without substring-matching English prose.
- `PublishOutboxItem` / `PublishOutboxRelay` (`kernel/types.rs:209-228`) give
  per-relay `status`, `attempt`, and `message` — a developer *can* see exactly
  why a publish failed and on which relay.
- `last_planner_error` (`kernel/types.rs:550`) surfaces structural planner
  failures instead of silent empty frames.
- Actor-thread death is a *terminal observable signal*: a panic emits one
  envelope-conforming `Panic` frame on the update channel before the channel
  closes (`ffi/mod.rs:308-333`). `last_tick_ms` (`kernel/types.rs:517`) lets a
  host detect a frozen actor by watching the field stop advancing.

Gap: all of this is *projected into the snapshot*. There is no log sink, no
structured event stream a developer can tail off-device — `logs: Vec<String>`
(`kernel/types.rs:534`) is a bounded in-snapshot buffer, not a diagnostics
channel. Production debugging means "render the snapshot's diagnostic fields
in a debug screen," which the host app must build itself.

## 3. "I want NMP to do X it doesn't do — how do I add it?"

Concrete feature a real developer wants: **a bookmarks list (NIP-51, kind:30003).**
Trace the path:

1. The clean way is a protocol module: implement `DomainModule` + `ViewModule`
   for bookmark records, register them. The seam exists —
   `register_raw_event_observer` (`ffi/mod.rs:516-528`) and `push_interest`
   (`ffi/mod.rs:547-549`) let a protocol crate subscribe and ingest typed
   events without touching `nmp-core`. That part is well-designed.
2. But to surface the bookmark *list view* to the host, the module's
   `ViewModule` must reach the FFI. There is no `ViewRegistry` wiring, and the
   generated `FfiApp::dispatch` cannot route module actions
   (`ffi.rs:44-49`) — it returns `UriRejected`. So the developer's bookmark
   action has nowhere to land.
3. The practical path a developer would actually take: add an
   `nmp_app_*` C verb to `crates/nmp-core/src/ffi/`, exactly as
   `publish_note`/`react`/`follow` already exist. That works — and that is the
   architecture *being in the way*: the doctrine-compliant module path is
   half-wired, so every new feature gets pulled back into bespoke FFI.

The path from "I want this" to "it's shipped" is clear *only* if you give up
on modules and add a C symbol. The supported path is the unsupported one.

## 4. "What is NMP actually good at right now?" — preserve this

Prior reviews never answered this. It is real:

- **Diagnostics/observability** (section 2): closed-key error categories,
  per-relay publish outbox with attempt counts, planner-error surfacing,
  actor-death panic frame. This is better than most production Nostr clients.
- **Secret hygiene.** The active nsec lives in `Zeroizing<String>`
  (`ffi/mod.rs:199`) and never crosses FFI for the create-account path; the
  capability socket keeps keyring policy Rust-side (`ffi/mod.rs:554-566`).
- **The observer seams.** `register_event_observer` (typed) and
  `register_raw_event_observer` (verbatim signed event, kind-filtered)
  (`ffi/mod.rs:487-528`) are a genuinely clean protocol-crate ingest seam —
  no Swift polling, kernel fan-out does the work.
- **Codegen determinism.** `check_modules` (`nmp-codegen/src/lib.rs:10-28`)
  and the determinism tests make regeneration safe and reviewable.
- **Lifecycle handling.** scenePhase foreground/background FFI + observer
  (`ffi/mod.rs:46-48`) is wired end-to-end.

None of this should be deleted in any future cleanup.

## 5. One decision the developer needs to make today

**Decision: pick the single canonical FFI pattern for app actions — this week.**

There are 49 unique `nmp_app_*` symbols. Among them, bespoke verbs
(`nmp_app_publish_note`, `nmp_app_react`, `nmp_app_follow`,
`nmp_app_unfollow`) coexist with the generic `nmp_app_dispatch_action`. Both
do the same job; no doc says which a developer should call.

- **Option A — bespoke verbs are canonical.** Delete `dispatch_action` and the
  half-built `ActionRegistry`. The FFI becomes "one C symbol per verb"; the
  module/codegen action path is abandoned and the docs say so. Honest, small,
  shippable.
- **Option B — `dispatch_action` is canonical.** Finish the module-reducer
  seam (NMP-145) so `FfiApp::dispatch` stops returning `UriRejected`, then
  deprecate the four bespoke verbs behind a Swift shim.

What breaks if deferred another week: every new feature (section 3's
bookmarks, and the next ten) gets added through whichever pattern the
developer guessed at — and the guess is currently "add a bespoke C verb,"
because that is the only path that actually runs. The drift compounds; review
#12 finds the same split, wider. Picking A is a one-day delete. Picking B is
real work but ends the ambiguity. Either is better than a third week of two
half-supported paths.
