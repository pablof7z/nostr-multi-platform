# Opus Direction Review #6 — The DX Thesis vs. the Two Half-Built Registries

**Date:** 2026-05-20
**Angles:** A (developer-experience thesis test) + C (substrate vs. framework) — treated as one finding, because the structural social bias (C) is *why* the second-app DX (A) fails.

## The claim under test

The thesis: a second non-social app needs `<100 LoC Rust + <300 LoC Swift`, built on `dispatch_action` + generic projections. Walk the actual code for a NIP-51 (kind:10003) bookmark manager. It does not hold today — and the reason is two registries that are each half-built.

## Finding 1 — `dispatch_action` is a Potemkin generic (write path)

`nmp_app_dispatch_action` (`crates/nmp-core/src/ffi/action.rs:63`) and `ActionRegistry::start` (`crates/nmp-core/src/kernel/action_registry.rs:210`) only **validate** an action and mint a correlation id. The module docs say so plainly (`action_registry.rs:24-35`): "`start()` returning a correlation id means 'the action was accepted', not 'the action ran'." `ActionRegistry::reduce` — the half that would *execute* — is `#[allow(dead_code)]` (`action_registry.rs:229`). `default_registry()` registers exactly one module: `PublishModule` (`action_registry.rs:273-277`).

Meanwhile execution still flows through ~30 bespoke FFI verbs — `nmp_app_publish_note`, `nmp_app_react`, `nmp_app_follow`, `nmp_app_unfollow`, `nmp_app_add_relay`, `nmp_app_open_author`, `nmp_app_open_thread`, `nmp_app_wallet_*` (`ffi/identity.rs`, `ffi/timeline.rs`, `ffi/wallet.rs`) — each feeding a bespoke `ActorCommand` variant: `PublishNote`, `React`, `Follow`, `Unfollow`, `WalletConnect`, `OpenAuthor` (`actor/mod.rs:120`+). The "single dispatch entrypoint" of PR #31 **coexists with, rather than replaces,** the verb-per-noun surface. Review #2 flagged the per-app/bespoke-FFI pattern; the news at review #6 is that the generic registry has landed and still does not execute.

For the bookmark app: you define `BookmarkModule: ActionModule`, register it — and nothing executes it, because no executor exists for non-`PublishModule` namespaces. The real working path is to build an `UnsignedEvent` and call `nmp_app_publish_unsigned_event` (`ffi/identity.rs:135`). That works, but it makes the `ActionModule` you wrote decorative.

## Finding 2 — there is no read-side registry at all (projection path)

`KernelUpdate` (`crates/nmp-core/src/kernel/types.rs:476-534`) is a single monolithic struct with hardcoded social fields: `profile`, `items`, `timeline`, `author_view`, `thread_view`, `accounts`, `active_account`, `publish_queue`, `bunker_handshake`, `wallet_status`. There is **no generic projection slot** — no `app_projection: serde_json::Value`, no per-namespace map.

The `ViewModule` trait exists (`substrate/view.rs:89`) with 9+ concrete impls across `nmp-nip29`, `nmp-reactions`, `nmp-core/publish` — but there is no `ViewRegistry`. Nothing drives `ViewModule::snapshot`/`on_event_inserted` into `KernelUpdate`. These impls are orphans: the exact pattern `ActionModule` had before PR #31, still unsolved on the read side. `wallet_status` did not "leak" by accident — it leaked because adding a kind to the snapshot has only one mechanism today: a new hardcoded `KernelUpdate` field, gated by a Cargo feature. That is a per-app crate dependency in disguise.

For the bookmark app: you define `BookmarkListView: ViewModule` — and nothing publishes its `Payload`. The fallback is `nmp_app_register_raw_event_observer` (`ffi/raw_event_tap.rs`) and parsing kind:10003 tags in Swift. That directly violates the bible's "no business logic in native" (`docs/aim.md:52`).

## The honest cost of app #2

Net, today app #2 is forced into one of two doctrine violations:
- **(a)** Extend `KernelUpdate` with bookmark fields inside `nmp-core` — a D0 violation, the path `wallet_status` took.
- **(b)** Move projection logic into Swift via raw-event observers — a bible violation.

The "<300 LoC Swift" claim is already empirically dead for the social proof app: `ios/NmpHighlighter` is **39,097 Swift LoC**. `nmp-highlighter-core` is 148 LoC — but `src/lib.rs` confirms it is a *placeholder scaffold* (`m11.5 Step 0`), not a built app. App #2 has not actually been built; the thesis has not been tested by the team, only by this review.

One genuine bright spot: `ActorCommand::PushInterest` (`actor/mod.rs`) + `ViewDependencies::into_logical_interest` (`substrate/view.rs:50`) *is* a working generic path — for subscriptions. A protocol crate can register a `LogicalInterest` and receive matching events without Swift involvement. It has no read-side counterpart. That asymmetry is the whole problem in miniature.

## Recommendation

Before any new NIP work, close the two registries as a matched pair: (1) wire `ActionRegistry::reduce` into the actor mailbox with a real executor (the M6 ledger), so `dispatch_action` *runs* actions and the bespoke verbs can be deleted; (2) build a `ViewRegistry` and add exactly one generic `KernelUpdate` field — `projections: BTreeMap<String, serde_json::Value>` — fed by `ViewModule::snapshot`. Until both exist, NMP is a social-client framework with a substrate-shaped FFI, and every non-social app pays the tax in the kernel or in Swift. D0 is necessary but not sufficient: it forbids naming app nouns, but the architecture still has no generic seam to *carry* them.
