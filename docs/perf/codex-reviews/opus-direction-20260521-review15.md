# Opus Direction Review #15 — 2026-05-21

Grounded in the working tree at `33f86b06` plus live `gh pr view` of the open
PRs. Two corrections to the dispatch brief up front, because they change the
findings:

- The brief calls **PR #62** the module-validator complement to #60. It is not.
  PR #62 is *"Refactor chirp-repl onto Chirp app runtime."* The actual
  `nmp_app_register_action_module` is **PR #64**, open and stacked on #60.
- The brief says PR #63 *deleted* `WelcomeWrapModule` as dead code. PR #63 is
  *open*, and it **relocates** `WelcomeWrapModule` into `nmp-marmot` (141 lines
  moved, not deleted). That is the right call — but it is not merged.

## 1. Headline finding

PR #60 alone does **not** open the substrate seam, and the code says so in its
own doc comment. `ffi/action.rs:117-120`: a namespace wired via
`nmp_app_register_action_executor` "is reachable by the registry's internal
`execute` path but **not yet by `nmp_app_dispatch_action`**." The seam-proof
test `host_registered_executor_dispatches_successfully` (`action.rs:424`) calls
`test_execute_action`, deliberately bypassing the public FFI entry point.
`nmp_app_dispatch_action` runs `ActionRegistry::start` first
(`action_registry.rs:161`), which looks up a *module* and rejects any namespace
`default_registry()` did not register. PR #60 shipped the executor half of a
two-half handshake and was merged as if it were the whole thing.

PR #64 is the half that matters. It adds `ClosureModule`, `register_with_validator`,
`nmp_app_register_action_module`, **and** three integration tests that run a
host-registered namespace end-to-end through `nmp_app_dispatch_action` —
including `executor_only_namespace_is_rejected_by_dispatch_action`, which proves
#60-alone is insufficient. Once #60 **and** #64 both land, the claim "a host can
dispatch a custom namespace without editing nmp-core" is true and tested.

What that still does not prove: nothing outside `nmp-core` exercises it. Every
proof test lives in `crates/nmp-core/src/ffi/action.rs` under `#[cfg(test)]`.
The registration seam will be *demonstrable* and *unused* — exactly the state
`dispatch_action` itself sat in for four reviews. A seam with only in-crate test
callers is a seam on probation.

## 2. The highest-impact gap

`KernelSnapshot` (`crates/nmp-core/src/kernel/types.rs:506-565`) is a sealed
social wire schema, and PR #64 makes this gap *more* visible, not less. The
action **input** path will be host-extensible; the snapshot **output** path is
not. Lines 527-531 bake `profile: ProfileCard`, `items: Vec<TimelineItem>`,
`author_view`, `thread_view`, `inserted/updated/removed: Vec<TimelineItem>`
directly into the struct every shell decodes — mirrored field-for-field in
`KernelBridge.swift:417-452` (`struct KernelUpdate`). A host can register
`market.listing` as an action namespace, dispatch it, and then receive a
snapshot whose entire typed payload is a social timeline. There is no
`nmp_app_register_snapshot_projection`, no namespaced extension field, no
`Value`-typed escape hatch. After #64, the asymmetry is the architecture:
extensible in, frozen out.

## 3. Three other significant gaps

**a. Chirp grew since review #14.** Review #14 reported 8,384 Swift LoC and
called it "unchanged since #13." Measured today: **8,660** (`find ios/Chirp
-name '*.swift' | xargs wc -l`). That is +276 LoC of shell code added *after* a
review explicitly flagged the 21× budget overrun, with PRs #50 and #59 (content
renderer, REPL kind:0) merged in between. The thin-shell thesis is not stalled;
it is actively regressing. `KernelBridge.swift` itself is the proof — of its 670
lines, the `dispatch_action` call is ~15 (`KernelBridge.swift:212-229`); the
rest is snapshot decoding, observer plumbing, capability callbacks, sink
lifetime management. Every new app inherits that whole surface.

**b. The action seam is a one-way door — it carries no result.** `NmpActionExecutor`
(`action.rs:104`) returns `*const c_char`: `NULL` for success or an error
string. There is no correlation-id-keyed completion, no typed output. A host
module that needs to *return data* (a query, a computed view) cannot — it must
round-trip through the snapshot, which (gap 2) it cannot extend. The registry's
`ActionPlan` carries `Output` as an associated type (`action_registry.rs:46`),
but the erased facade discards it: `ErasedActionModule::start` returns only
`ActionPlan<Value>` with no output channel. The substrate API is fire-and-forget
publish, generalized. That is fine for "social verbs"; it is not a general
action runtime.

**c. Per-verb symbols still bypass the registry, and the brief admits it.**
`nmp_app_react`, `nmp_app_follow`, `nmp_app_unfollow` remain live C symbols
(`ffi/identity.rs:328/351/365`), each constructing an `ActorCommand` directly.
Review #14 prescribed *not* migrating them, on the logic that they "prove
nothing new." That reasoning is now inverted by the evidence: `nmp_app_publish_note`
was deleted in PR #56 and the structural pull stopped dead at one caller. Three
verbs routed around the registry are not a neutral leftover — they are a
standing demonstration that the registry is optional, which is exactly why no
out-of-tree caller has appeared.

## 4. The bet that should scare you

The dangerous assumption is no longer "can a second app be built" (review #14).
It is sharper: **registration + a frozen snapshot schema + the existing 670-line
bridge surface ≠ a thin shell — and nobody has measured the gap.**

Suppose #64 lands perfectly. App #2's host still must: decode a `KernelSnapshot`
whose typed fields are social (gap 2); reproduce `KernelBridge`'s capability
callback, update-sink, observer-registration, and JSON-redecode machinery
(8,660 LoC of precedent); and route its results back through a snapshot it
cannot extend (gap 3b). The action *input* seam is the cheap 15 lines. The
expensive 95% — output projection, bridge boilerplate, schema neutrality — is
untouched. The bet is that the seam PRs address the hard part. They address the
visible part. If app #2 is attempted and the honest path is "fork
`KernelSnapshot` and copy `KernelBridge`," the substrate premise is refuted, and
the input seam will have been a well-tested door into a single-shaped room.

## 5. The next concrete PR

**Not** another input-side PR. After #60+#64 the input seam is complete; a sixth
review prescribing more registration plumbing would be reprinting #14.

Land `nmp_app_register_snapshot_projection`: a host-registered closure that
appends one namespaced JSON object to `KernelSnapshot` under a host-chosen key,
emitted on every tick alongside the existing fields. Prove it with an
out-of-`nmp-core` test crate that registers a non-social projection key, runs a
tick, and decodes its own field back — the symmetric counterpart to PR #64's
`host_registered_module_and_executor_enables_dispatch_action`.

Why this one: it is the smallest change that makes the snapshot *output* as
extensible as #64 makes the action *input*, and it is the precondition for a
non-social app to exist at all — without it, app #2 reads social fields it does
not want or edits `nmp-core`. It also forces the first honest conversation about
`KernelSnapshot`'s frozen core: once a host can append, the question "why are
`items`/`author_view`/`thread_view` not themselves projections?" becomes
unavoidable. Until an out-of-tree projection round-trips through a real tick,
review #16 will reprint section 2 of this document.
