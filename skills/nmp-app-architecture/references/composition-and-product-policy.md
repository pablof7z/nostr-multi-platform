# Explicit Feature Composition and App-Owned Product Policy

> Canonical source: ADR-0069.
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

2. **Reusable Nostr protocol features** — one public installer from each owning
   protocol crate. Every protocol crate exposes `Config`, `Handles`, and
   `register(app, config) -> Result<Handles, RegistrationError>`. The installer
   takes only the narrow registrar traits it needs, never `AppHost`.
   - `nmp_nip50::register(app, nmp_nip50::Config::default())`.
   - `nmp_nip02::register(app, nmp_nip02::Config::default())`.
   - `nmp_replies::register(app, nmp_replies::Config::default())`.
   - `nmp_nip25::register(app, nmp_nip25::Config::default())`.
   - `nmp_nip18::register(app, nmp_nip18::Config::default())`.
   - `nmp_nip84::register(app, nmp_nip84::Config::default())`.
   - `nmp_nip29::register(app, nmp_nip29::Config::default())`.
   - `nmp_wot::register(app, nmp_wot::Config::default())`.
   - `nmp_nip51::register(app, nmp_nip51::Config { ... })`.
   - `nmp_nip17::register(app, nmp_nip17::Config::default())`.
   - `nmp_nip22::register(app, nmp_nip22::Config::default())`.
   - `nmp_nip23::register(app, nmp_nip23::Config::default())`.
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
pub fn register(app: &mut (impl AppHost + ActionRegistrar)) {
    // 1. Substrate floor — correctness, not preference.
    let _substrate = nmp_substrate::install(app, nmp_substrate::SubstrateConfig::default());

    // 2. Named protocol installers — pick what this app needs.
    let _nip50 = nmp_nip50::register(app, nmp_nip50::Config::default())
        .expect("nmp-nip50 registration must not collide");
    let _nip02 = nmp_nip02::register(app, nmp_nip02::Config::default())
        .expect("nmp-nip02 registration must not collide");
    let _replies = nmp_replies::register(app, nmp_replies::Config::default())
        .expect("nmp-replies registration must not collide");
    let _nip25 = nmp_nip25::register(app, nmp_nip25::Config::default())
        .expect("nmp-nip25 registration must not collide");
    let _nip18 = nmp_nip18::register(app, nmp_nip18::Config::default())
        .expect("nmp-nip18 registration must not collide");
    let _nip84 = nmp_nip84::register(app, nmp_nip84::Config::default())
        .expect("nmp-nip84 registration must not collide");
    let _nip29 = nmp_nip29::register(app, nmp_nip29::Config::default())
        .expect("nmp-nip29 registration must not collide");
    let _wot = nmp_wot::register(app, nmp_wot::Config::default())
        .expect("nmp-wot registration must not collide");
    let _nip51 = nmp_nip51::register(
        app,
        nmp_nip51::Config {
            search_fallback_relays: nmp_nip50::SearchFallbackRelays::default(),
        },
    )
    .expect("nmp-nip51 registration must not collide");
    let _nip17 = nmp_nip17::register(app, nmp_nip17::Config::default())
        .expect("nmp-nip17 registration must not collide");
    let _nip22 = nmp_nip22::register(app, nmp_nip22::Config::default())
        .expect("nmp-nip22 registration must not collide");
    let _nip23 = nmp_nip23::register(app, nmp_nip23::Config::default())
        .expect("nmp-nip23 registration must not collide");

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

## Ordering and the Composition Ledger (ADR-0069)

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
- A reusable protocol crate exposes public split installers such as
  `register_actions`, `register_runtime`, or `register_*_scopes` instead of the
  canonical `register(app, Config)` entry point.
- A reusable protocol installer takes `AppHost` instead of the exact narrow
  registrar traits it needs.
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
