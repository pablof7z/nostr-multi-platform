# Codex review — #1493 P4 follow-feed kinds persist + Android imperative openTimeline removal

## Finding (P4 Finding 1)
Android `KernelModel.kt` called `bridge.openTimeline()` imperatively after
`signInNsec`/`createAccount`/`switchAccount` — a per-platform post-identity
sequence (native deciding policy; D7). iOS leaves it to the View-driven path
(`HomeFeedView.task`).

## Root cause (latent on BOTH platforms)
`open_contact_feed` (the "open home feed" verb) dropped the host-declared
follow-feed `kinds` when no account was active (`toast_no_account`). Both shells
mount the home-feed view at launch, firing this verb BEFORE sign-in, so
`kernel.follow_feed_kinds` stayed empty; the post-sign-in
`reconcile_follow_feed_after_identity_change` then registered an EMPTY
follow-feed → no timeline until app restart. Android masked it with the
imperative openTimeline; iOS (View-driven only) exhibited the latent bug.

## Fix (one coherent PR)
1. Kernel (`open_contact_feed`): store host-declared kinds UNCONDITIONALLY via
   `set_follow_feed_kinds` (account or not). With no account,
   `register_follow_feed_for_active_account` early-returns — only the kinds are
   primed; the sign-in reconcile reads them back. Empty kinds still clear
   (close_contact_feed semantics unchanged).
2. Android (`KernelModel.kt`): remove the imperative post-identity
   `bridge.openTimeline()`; rely on `TimelineScreen.LaunchedEffect`, matching iOS.
3. iOS: no change — already View-driven; the kernel fix repairs iOS's latent bug.

## Codex verdict: APPROVE, no concrete bugs
- No prior-account follow leak (registration reads the NEW active pk's contacts
  cache; logout/switch reconcile clears stale interests).
- Priming kinds with no account is safe (register early-returns; empty kinds
  clears+returns).
- Android removal closes the gap: even if Compose's LaunchedEffect does not
  re-fire, the persisted kinds let the sign-in reconcile register the feed.

## Tests
- New: `open_contact_feed_before_signin_persists_kinds_for_later_reconcile`
  (declare kinds pre-account → sign in → kind:3 → REQ emitted).
- `cargo test -p nmp-core --lib` (1624 passed), `contact_feed`/`follow_feed`/
  `t168` groups green, `doctrine_lint_smoke` (74 passed), Android
  `:app:compileDebugKotlin` green.
