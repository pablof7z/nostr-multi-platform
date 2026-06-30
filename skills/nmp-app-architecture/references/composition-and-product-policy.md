# Explicit Feature Composition and App-Owned Product Policy

> Canonical source: ADR-0069, amending ADR-0046 and ADR-0049.
> Enforcement: `cargo test -p nmp-testing --test doctrine_lint_smoke`.

## The Composition Contract (production)

A production NMP app root installs components **explicitly and by name**. Reading the root
must answer "what does this app do?" without reverse-engineering a preset. A maintainer should
see, without opening any other file:

1. **Substrate floor** — `nmp_substrate::install(app, SubstrateConfig::default())`. The
   correctness floor: routing, mailbox/profile/contact caches, publish resolver, external
   event forwarding, coverage gate, NIP-77 sync hooks, NIP-11 relay metadata. Non-negotiable —
   without it `PublishTarget::Auto` fail-closes and all routing returns `Unroutable`. This is
   generic Nostr-client infrastructure, not product policy. No hidden preset is provided.

2. **Reusable Nostr protocol features** — named installers from the owning protocol crates:
   - `nmp_nip50::register_search_scopes(app)` and `register_input_scopes(app)`.
   - `nmp_nip02::register_follow_actions(app)`.
   - `nmp_replies::register_actions(app)`.
   - `ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app)`.
   - `ProtocolDescriptor::register_actions(&nmp_nip18::Nip18Descriptor, app)`.
   - `ProtocolDescriptor::register_actions(&nmp_nip84::Nip84Descriptor, app)`.
   - `nmp_nip29::register_input_scopes(app)`.
   - `nmp_wot::register_runtime(app)`.
   - `nmp_nip51::{register_mute_runtime, register_bookmark_runtime,
     register_bookmark_set_runtime, register_web_bookmark_runtime,
     register_search_relay_runtime_with_fallbacks}`.
   - `nmp_nip17::{register_actions, register_runtime}`.
   - `nmp_nip22::register_runtime(app)`.
   - `nmp_content::register_longform_projection(app)`.
   Non-social apps call the substrate installer plus only the protocol/runtime installers they
   actually need.

3. **App-owned features** — protocol modules and app-domain modules the app contributes.
   These live in app Rust crates, never in NMP crates.

4. **Shell capability contracts** — typed `CapabilityModule` registrations for OS handles the
   native shell executes (see `runtime-capability-shell-boundary.md`).

5. **Client identity** — one `ClientIdentity` at composition time for relay UA and optional
   NIP-89 `client` tagging.

Canonical template (`nmp-cli/templates/lib.rs.tmpl`):

```rust
pub fn register(app: &mut impl AppHost) {
    // 1. Substrate floor — correctness, not preference.
    let _substrate = nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());

    // 2. Named protocol installers — pick what this app needs.
    nmp_nip50::register_search_scopes(app);
    nmp_nip50::register_input_scopes(app);
    nmp_nip02::register_follow_actions(app);
    nmp_replies::register_actions(app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip25::Nip25Descriptor, app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip18::Nip18Descriptor, app);
    nmp_core::substrate::ProtocolDescriptor::register_actions(&nmp_nip84::Nip84Descriptor, app);
    nmp_nip29::register_input_scopes(app);
    let _wot = nmp_wot::register_runtime(app);
    let _mute = nmp_nip51::register_mute_runtime(app);
    let _bookmarks = nmp_nip51::register_bookmark_runtime(app);
    nmp_nip51::register_bookmark_set_runtime(app);
    nmp_nip51::register_web_bookmark_runtime(app);
    let _search_relays = nmp_nip51::register_search_relay_runtime_with_fallbacks(
        app,
        nmp_nip50::SearchFallbackRelays::default(),
    );
    nmp_nip17::register_actions(app);
    nmp_nip17::register_runtime(app);
    let _comments = nmp_nip22::register_runtime(app);
    nmp_content::register_longform_projection(app);

    // 3. App-owned modules — app nouns stay here, never in nmp-core (D0).
    app.register_action(MyActionModule);
}
```

## Deleted Aggregate Vocabulary

`crates/nmp-defaults`, `nmp_defaults`, `NmpDefaults`, `SearchDefaults`, `register_defaults*`,
and renamed "social stack" / "default bundle" helpers are deleted. They may appear only in
negative ratchets or explicit historical migration context. Any surviving production,
scaffold, tutorial, or test-helper path that recreates the aggregate is a **blocking finding**,
same severity as native business logic.

The doctrine smoke test (`production_starter_rejects_hidden_register_defaults_preset`)
enforces that production scaffolds, builder-guide docs, and CLI templates do not teach
`register_defaults` as the normal path. The architecture scanner flags `register_defaults(`
calls outside the definition site as a complementary cross-repo gate.

## No Replacement Bundle

Deleting `nmp-defaults` does not create permission for a renamed aggregate. A helper such as
`install_social_stack`, `register_standard_nostr`, `register_defaults_bundle`, or a hidden
test-only composition function has the same failure mode: the app root no longer answers what
the app installs. If a set of registrations is common, keep each installer owned by the crate
whose mechanism it installs and call those installers directly at the app root.

## App Product Policy Stays in App Crates

A request from one downstream app to add product behavior (relay defaults, feed seeds,
onboarding flows) to an NMP crate is evidence of demand, not permission to add app-named
helpers or product policy to shared crates (D0 corollary). Operator relay policy is declared
in an app config surface (the audited D3 opt-out — see the scanner's relay-policy handling),
not in a shared NMP crate.

## The Composition Root Is the App's Answer to D0

If an app noun or product policy can be discovered by reading the composition root, D0 is
clean. If it is buried inside a preset or a framework crate, D0 is violated. The substrate
tier is the one exception — it is generic Nostr-client infrastructure (routing, mailbox
caches, publish resolver), not product policy.

## Ordering and the Composition Ledger (ADR-0049)

All registrations must complete before `nmp_app_start`. Late wiring is recorded as
`DroppedLateWiring` and silently dropped (D6 — failures are state, not exceptions). The
kernel records every registration:

| Disposition | Meaning |
|---|---|
| `Installed` | First and only registration for this seam/key. |
| `ReplacedPrevious` | App-over-default override — expected and silent. |
| `YieldedToExisting` | Default installer yielded to a pre-existing app registration. |
| `DroppedLateWiring` | Registration arrived after `nmp_app_start` — silently dropped. |

Read the ledger via `nmp_app_composition_report(app)` (JSON). Late-wiring is the most common
composition bug and is silent by construction; the ledger is the only diagnostic surface.

Default protocol installers (`register_default_action`) **yield** to an app module registered
under the same namespace, regardless of call order — a library default can never clobber an
app registration. App-over-app namespace collision fails loudly in dev/test and is a violation
to fix, not suppress.

## Review Checklist

Block the change when:
- A production app root calls `register_defaults()` or recreates the deleted defaults bundle.
- The composition root is absent (all wiring hidden inside a helper that is not the NMP
  owner-local substrate/protocol installer call list).
- App product policy (relay URLs, seed follows, onboarding, signer perm defaults) lives in an
  NMP crate instead of the app crate.
- A new NMP crate is created to hold features specific to one downstream app.
- Named protocol installers are omitted but the app expects the features they install (silent
  misconfiguration — verify via the composition ledger).

Design questions for any new app root:
- Can a reader name every installed substrate, protocol feature, and app feature from
  `register()` alone?
- Is all product policy owned by the app crate, not any shared NMP crate?
- Are late-wiring risks eliminated (all registrations before `start()`)?
