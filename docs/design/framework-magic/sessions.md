# Framework Magic §C12 — Account Switch as State

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/product-spec/subsystems.md` §7.4 (`SessionState`); `docs/design/subscription-compilation/recompilation.md` §4.2 trigger A4 (`ActiveAccountChanged`); `docs/design/subscription-compilation/intro.md` §2.3 (account scope binding); `docs/aim.md` §6 doctrine 7.

## C12. Account switch is a state transition; views rebind without imperative dance

**Statement.** Switching the active account is a single dispatched action. After the dispatch, every `InterestScope::ActiveAccount`-scoped view (a "following timeline", "my profile", "my mentions", etc.) re-resolves against the new account's context — new follow-set, new mailbox set, new mute list, new signer — without the application issuing CLOSE/REQ frames, tearing down view handles, or rebuilding any UI. The view handles remain valid; their payloads update.

**Framework does:**

- `SessionState` (`subsystems.md` §7.4 lines 107–125) carries `accounts: Vec<Account>` and `active: Option<String>` as plain state fields. A `SwitchActiveAccount { pubkey }` action mutates `active`; the mutation is the only state change.
- `Trigger::ActiveAccountChanged { from, to }` (`subscription-compilation/recompilation.md` §4.2) fires as a consequence of the state change. The planner re-runs `interests()` on every `ViewModule` whose registered interest carries `InterestScope::ActiveAccount` (`subscription-compilation/intro.md` §2.1 line 60 + §2.3); `InterestScope::Account(specific)` and `InterestScope::Global` interests are untouched.
- The compiler diffs the new plan against the old; per-relay CLOSE/REQ frames fire only for the *delta* (e.g., previous account's follows that are not in new account's follows close their slices; new follows open new slices).
- View payloads recompute via the same `on_event_replaced` / `on_event_inserted` cascade the kernel uses for any state change; the platform shadow's `useFollowingTimeline()` etc. emit a new payload.
- The signer attached to operations dispatched after the switch is the new active account's signer (per `IdentityModule` routing in `kernel-substrate.md` §6).

**App writes:** one dispatch: `dispatch(AppAction::SwitchActiveAccount { pubkey })`. The app's "switch account" UI is a button that fires that dispatch. No log-out / log-in dance, no view-tree rebuild, no manual REQ reissue, no clearing of caches — the framework handles all of it as a single tick of the actor's event loop.

**Failure mode prevented:** `product-spec/overview-and-dx.md` §3.3 **bug #5** ("Two account contexts having overlapping mutable state"). Plus the operationally common bug where an app tears down its view tree on account switch — losing scroll position, in-flight composes, draft state — because it doesn't trust the framework to re-derive correctly. C12 makes the trust structural: the view handles remain valid; the app cannot accidentally observe the old account's data on the new account's views.

**Test:** `c12_account_switch_rebinds_views_without_imperative_dance`. The test:

1. **Setup:** seed two accounts in `SessionState.accounts` — Alice (follows `[X, Y]`) and Bob (follows `[Y, Z]`). Pre-seed mailboxes: X→r1, Y→r2, Z→r3. Set Alice active.
2. **Initial open:** open `FollowingTimelineView` (no fields — derives from active account); assert the planner opens REQs on `{r1, r2}`; assert the payload emits with follow set `{X, Y}`.
3. **Dispatch switch:** `dispatch(AppAction::SwitchActiveAccount { pubkey: bob_pk })`. The test makes no other calls; the harness drains the action ledger and the planner trigger queue.
4. **Assert delta wire frames:** exactly two frames emitted by the planner — `CLOSE` for the r1 slice (X drops; X is not in Bob's follows), `REQ` for the r3 slice (Z appears; Z is in Bob's follows). The r2 slice is untouched (Y is in both follows).
5. **Assert view handle stability:** the `FollowingTimelineView` handle from step 2 is **the same handle**; it has not been torn down. Its payload has been re-emitted once, now reflecting Bob's follow set `{Y, Z}`.
6. **Assert signer rebinding:** dispatch a `SendNote { content: "hello" }`; assert the signed event's `pubkey = bob_pk` (the new active account's signer was used), without any explicit signer parameter on the `SendNote` action.
7. **Assert specific-scoped views untouched:** before step 3, also open `ProfileView { pubkey: charlie_pk }` (an `InterestScope::Account(charlie)`-equivalent — actually Global since it names an explicit author). Assert this view's payload is not re-emitted after the switch; its underlying REQ stays alive on the same relay; no delta frames touch it. This is the symmetric assertion: the switch affects *only* `ActiveAccount`-scoped interests, per `subscription-compilation/recompilation.md` §4.2 line 113.
8. **Assert no overlap:** read the audit log of any per-account domain-store namespace (e.g., Alice's drafts) and assert Bob cannot read it. The kernel's domain-store isolation per account is the structural enforcement (`kernel-substrate.md` §8 "Domain stores are isolated" and the per-account scoping in domain key prefixes).

**Milestone owner:** **[PENDING M8]**. M8 is the multi-account session milestone. M2 already lands the `Trigger::ActiveAccountChanged` shape (`subscription-compilation/recompilation.md` §4.2 line 109: "M2 establishes the trigger; M8 wires the multi-account state machine"). Test checked in as `#[ignore = "pending M8 multi-account state machine"]`. Sub-paths 4 and 7 are testable as soon as M2 lands (single-account boot fires the trigger once with `from: None, to: Some(active)` per the M2 design); the rest needs M8.

## Why this is one bullet, not several

The eight sub-paths assert different facets of one observable contract: *after the switch dispatch, every consequence is a derived re-emission, never an imperative reissue.* The kernel-substrate (`kernel-substrate.md` §8) ensures domain-store isolation; the planner (`subscription-compilation/recompilation.md` §4.2) ensures interest re-resolution; the identity machinery (`kernel-substrate.md` §6) ensures signer rebinding. The contract bullet covers all three as one because they are observed together: an app that does `dispatch(SwitchActiveAccount)` and then attempts any operation gets a correctly-rebound system; partial rebinding is a regression.

## Doctrine alignment

C12 is the most direct demonstration of cardinal doctrine **D4** ("single writer per fact; caches derive"). The "fact" is `SessionState.active`. The "caches" are every active-account-scoped view, every signer binding, every relay-routing decision. The framework's job is to make sure every cache derives mechanically; the app's job is to write the fact once.

It also discharges `aim.md` §6 doctrine 7: "Sessions are state, switching is an action. No imperative 'log out, then log in, then reload' dance." That sentence is the contract C12 holds in place.

## Cross-references

- `docs/design/subscription-compilation/intro.md` §2.3 — `InterestScope::ActiveAccount` resolution at compile time, not registration time.
- `docs/design/subscription-compilation/recompilation.md` §4.2 trigger A4 — the actor-message shape of the `ActiveAccountChanged` trigger.
- `docs/design/kernel-substrate.md` §8 — module composition rules, specifically domain-store isolation.
- `docs/product-spec/subsystems.md` §7.4 — `SessionState` field shapes.

## Interaction with C11

C11 covers *onboarding*: adding an account to `SessionState.accounts`. C12 covers *switching*: changing which account in that list is `active`. The two are independent: an app can onboard without switching, or switch among already-onboarded accounts without onboarding. The framework guarantees both.

The full sequence (onboard → switch → use) is exercised by C11 sub-path 2(e): create a new identity, switch to it, sign an event. That test crosses both contract bullets and is the canonical end-to-end demonstration.

## What this chapter does not cover

- **The login UI itself.** The app provides the button; the contract specifies what the dispatch guarantees.
- **The account-switcher view payload.** That is a view module (`AccountListView` or similar in `nmp-core`'s built-ins per `subsystems.md` §7.4); its spec/payload is owned by the view catalog, not the contract.
- **Background account state** (per-account sync watermarks, per-account action ledger). Those are per-account scopes inside the storage backend; the contract does not specify the scoping mechanism, only that the switch does not leak state across.
- **Logging out / removing an account.** A `RemoveAccount` action exists in the long-term catalog (`subsystems.md` §7.4 implied); its contract surface is a separate potential bullet, not in v1's 13. Removal cleanly through the same `IdentityModule::destroy` path (kernel-substrate.md §6 line 341).
