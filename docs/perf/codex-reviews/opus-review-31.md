# Opus Direction Review #31 — 2026-05-21

Reviewed at master `afeeece5` (PR #93). PR #94/#95 had not landed in the
reviewed tree — findings below are from code, not the brief.

## 1. Biggest architectural risk: the action seam is bypassed by its own kernel

`dispatch_action` is documented as the "universal action entry point," but
`ActorCommand` (actor/mod.rs) has **40 variants** and only ~5 namespaces route
through `dispatch_action` (`nmp.publish`, `chirp.react|follow|unfollow`,
`nmp.nip29.join_request`). `OpenAuthor`, `OpenThread`, `AddRelay`, `SignInNsec`,
`CreateAccount`, `WalletConnect` and 30 others remain bespoke `extern "C"`
symbols. The substrate isn't failing from dead traits — it's failing because
the *live* surface grew somewhere else. Every new feature still has two doors:
a generic one nobody is forced through, and a bespoke FFI symbol that is
faster to ship. Until the bespoke door is closed, `dispatch_action` will stay a
minority path and the "single universal seam" claim is aspirational.

## 2. ViewModule: delete it

`substrate/mod.rs` already makes the case in its own doc comment: "what never
shipped is a kernel-side registry that stores `dyn Trait` objects." No
`ViewRegistry` exists. The one live impl (`Nip10ModularTimelineView`) is reached
by static dispatch, never via the trait. 19/20 impls have zero consumer. This is
the exact profile of the deleted `ModuleRegistry`. Keep `ViewDependencies`
(genuinely used by the planner bridge); delete the `ViewModule` trait and its
test-only impls. `IdentityModule` is in the same category — one never-driven impl.

## 3. What NMP shouldn't do: `ActionPlan` is still alive and still dead

`ActionModule::start` still returns `Result<ActionPlan<Step>, ActionRejection>`;
the registry adapter constructs an `ActionPlan` only to have every field
discarded at dispatch. The in-flight collapse to `Result<(), _>` must land — it
is the correct call. Don't add `ActionStatus`/`Step` machinery the runtime
never reads.

## 4. Highest-priority next: ship NIP-57 zaps end-to-end

`nmp-nip57` already has `build`, `decode`, `domain`, `view`, `bolt11`. The only
gaps are an `ActionModule` (kind:9734 zap request), an executor, and a Chirp
surface. It is the shortest path to proving the action seam carries a *second*
real protocol-crate action with user value — and zaps are the feature most
visibly missing from a Nostr client. NIP-17 is higher user ROI but needs a
crate from scratch; defer it one cycle.

## 5. Surprise: the thin-shell rule is being violated in present tense

`KernelBridge.swift::publishProfile` builds a kind:0 event dict in Swift and
calls `nmp_app_publish_unsigned_event` — protocol logic in the shell. iOS Chirp
is 8,120 LoC of Swift. Either enforce the rule with a CI grep for event-kind
literals in Swift, or retire the rule honestly.
