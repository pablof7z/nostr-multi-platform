# Opus Direction Review #14 — 2026-05-21

Verified against `origin/master` at `3b3e7ab0` (PR #56 merge commit), fetched
fresh during this review. The worktree's local `master` ref was stale at
`bbeb0282`; the discrepancy is noted because it matters for #2.

## 1. Headline finding

PR #56 landed for real. On `origin/master` the `nmp_app_publish_note` C symbol
is deleted, `PublishAction::PublishNote` exists, and `KernelBridge.publishNote`
(`ios/Chirp/Chirp/Bridge/KernelBridge.swift:224`) now calls
`nmp_app_dispatch_action(raw, "nmp.publish", json)`. After four reviews
(#9, #11, #13) flagging `dispatch_action` as a seam with zero production
callers, it finally has exactly one.

What that proves: the registry can carry a real domain action end-to-end
through a real shell. The type-erased `ErasedActionModule` keyed by namespace
string works in production, not just in tests. That is a genuine de-risking of
the *mechanism*.

What it does not prove — and this is the part you will not like — is the
*thesis*. One caller is not a pattern. `nmp_app_react`, `nmp_app_follow`,
`nmp_app_unfollow` are still per-verb C symbols at `ffi/identity.rs:328/351/365`.
The migration that #9 prescribed as a forcing function deleted exactly the one
symbol it was told to delete and stopped. The registry now has a publish module
and three actions still routed around it. "First caller" became "only caller,"
and the structural pull that was supposed to drag the rest of the verbs through
the seam did not materialize. The forcing function fired once and went quiet.

## 2. The highest-impact gap still open

There is no host registration seam, and PR #56 did nothing to create one.
`NmpApp.action_registry` is private (`crates/nmp-core/src/ffi/mod.rs:225`),
built once by `default_registry()` (`kernel/action_registry.rs:230`) at
construction time (`ffi/mod.rs:369`). There is no `nmp_app_register_action_module`
FFI. Grep for `nmp_app_register_action` across `origin/master` returns only a
prior review document.

This is the gap that decides whether NMP is a *substrate* or a *social SDK with
extra indirection*. The entire multi-app thesis rests on a host being able to
add a domain — a podcast app, a marketplace, a wiki — without editing
`nmp-core`. Today it cannot. Every action module must be compiled into the
kernel's `default_registry()`. The namespace-keyed `ErasedActionModule` design
is the right shape for external registration, but the shape is wasted while the
registry is sealed at the FFI boundary. Until a host can call
`register_action_module` and dispatch a namespace `nmp-core` has never heard
of, "multi-platform multi-app substrate" is an aspiration, not an architecture.

## 3. Three other significant gaps

**a. `KernelSnapshot` is D0 at the wire-schema level.** `kernel/types.rs:505`
bakes `profile: ProfileCard`, `items: Vec<TimelineItem>`, `author_view`,
`thread_view` directly into the JSON struct every shell decodes. Review #13
named this; PR #54/#56 did symbol-level cleanup and left the schema untouched.
A non-social app receives a snapshot whose wire contract is a social timeline.
You can delete every social *symbol* and the social *data model* is still the
contract. This is the D0 violation that actually constrains app #2.

**b. The protocol crates still carry app nouns.** `nmp-reactions` exports
`SocialRecord`/`SocialKind`/`SOCIAL_KINDS` (`crates/nmp-reactions/src/`);
`nmp-nip59` exports `WelcomeWrapModule`/`WrapPlan` referencing MLS/Marmot
(`crates/nmp-nip59/src/action/welcome_wrap.rs`). A NIP-59 gift-wrap crate that
knows what a Marmot Welcome is has the dependency arrow pointing the wrong way.
These are small files but they are load-bearing for the claim that protocol
crates are reusable.

**c. Chirp is 8,384 Swift LoC against a 400-LoC thin-shell budget — 21× over.**
Unchanged since #13. PRs #50/#51 (rich content renderer, view-pipeline
extraction) are still open and *add* shell logic. The thin-shell thesis is not
slipping; it was abandoned without a decision. Either revise the budget on the
record or stop merging shell-side rendering work.

## 4. The bet that should scare you

The bet: that a second non-social app can be built on this kernel at all.

Every review since #6 has asked for a second app and none exists. The reason is
not laziness — it is that the three gaps above compound. To ship app #2 you
need (i) a registration seam to add its actions, (ii) a snapshot schema that
isn't a social timeline, and (iii) protocol crates free of social nouns. None
exist. The single-app codebase has been polished — dead code removed, mutexes
de-poisoned, unwraps deleted — but polished in a shape that only a social app
fits. If a second app is attempted and the honest path turns out to be "fork
the kernel," the entire substrate premise is refuted, and ~28 crates of
generality were paid for and never collected.

## 5. The next concrete PR

Add `nmp_app_register_action_module` and prove it with a non-`nmp-core` module.

Make `ActionRegistry` accept post-construction registration, expose an FFI that
takes a namespace + an `ErasedActionModule` (or a host callback the adapter
wraps), and land a *test or example crate outside `nmp-core`* that registers a
namespace the kernel does not know and dispatches one action through it.

Not the react/follow/unfollow migration — that moves three known verbs and
proves nothing new; the registry already handles `nmp.publish`. Not snapshot
schema work — necessary but larger, and pointless until a non-social caller
exists to shape it. The registration seam is the smallest change that converts
`dispatch_action` from "single-entry social dispatch" into the substrate API
the project name promises. Until an out-of-tree module dispatches successfully,
review #15 will reprint this section verbatim.
