# Explicit Feature Composition and App-Owned Product Policy

> Canonical source: ADR-0069, amending ADR-0046 and ADR-0049.
> Enforcement: `cargo test -p nmp-testing --test doctrine_lint_smoke`.

## The Composition Contract (production)

A production NMP app root installs components **explicitly and by name**. Reading the root
must answer "what does this app do?" without reverse-engineering a preset. A maintainer should
see, without opening any other file:

1. **Substrate floor** — `nmp_defaults::register_substrate(app, coverage_gate)`. The
   correctness floor: routing, mailbox/profile/contact caches, publish resolver, external
   event forwarding, coverage gate, NIP-77 sync hooks, NIP-11 relay metadata. Non-negotiable —
   without it `PublishTarget::Auto` fail-closes and all routing returns `Unroutable`. This is
   `MinimalPlugins`, not `DefaultPlugins`. No toggle is provided.

2. **Reusable Nostr protocol features** — named installers from `nmp-defaults`:
   - `register_social_protocol_defaults(app, search_defaults)` — NIP-02, NIP-18, NIP-25,
     NIP-29 input scopes, NIP-51/NIP-84 actions; WOT, mute, bookmark, search-relay, comment
     runtimes.
   - `register_dm_protocol_defaults(app)` — NIP-17 DM actions + inbox runtime.
   - `register_nip50_protocol_defaults(app)` — NIP-50 search and input scopes.
   - `register_longform_projection(app)` — NIP-23 kind:30023 typed projection.
   Non-social apps (podcast-player, hl, win-the-day) call `register_substrate` only and omit
   the social bundle.

3. **App-owned features** — protocol modules and app-domain modules the app contributes.
   These live in app Rust crates, never in NMP crates.

4. **Shell capability contracts** — typed `CapabilityModule` registrations for OS handles the
   native shell executes (see `runtime-capability-shell-boundary.md`).

5. **Client identity** — one `ClientIdentity` at composition time for relay UA and optional
   NIP-89 `client` tagging.

Canonical template (`nmp-cli/templates/lib.rs.tmpl`):

```rust
pub fn register(app: &mut impl AppHost) {
    let nmp_defaults::NmpDefaults { coverage_gate, search_defaults, .. } =
        nmp_defaults::NmpDefaults::default();

    // 1. Substrate floor — correctness, not preference.
    let _mailbox_cache = nmp_defaults::register_substrate(app, coverage_gate);

    // 2. Named protocol installers — pick what this app needs.
    nmp_defaults::register_nip50_protocol_defaults(app);
    let _social = nmp_defaults::register_social_protocol_defaults(app, search_defaults);
    nmp_defaults::register_dm_protocol_defaults(app);
    nmp_defaults::register_longform_projection(app);

    // 3. App-owned modules — app nouns stay here, never in nmp-core (D0).
    app.register_action(MyActionModule);
}
```

## `register_defaults()` — Banned as a Production Path

`nmp_defaults::register_defaults` survives only as compatibility/tutorial/migration surface.
Any surviving call in production code is a **blocking finding**, same severity as native
business logic. A compatibility preset is valid only when it carries: a named list of live
consumers, a named owner, a defined support window, and a deletion or formalization trigger.

The doctrine smoke test (`production_starter_rejects_hidden_register_defaults_preset`)
enforces that production scaffolds, builder-guide docs, and CLI templates do not teach
`register_defaults` as the normal path. The architecture scanner flags `register_defaults(`
calls outside the definition site as a complementary cross-repo gate.

## `nmp-defaults` — What It Is and Is Not

**Is:** a reusable installer library — callable installers an app root invokes by name. No
copy-and-edit; no code generation; no policy ownership.

**Is not:** a hidden production preset, a leaf-app policy owner, or a relay URL source. It must
not own seed follows, bootstrap relay brands, signer permission defaults, onboarding flows,
app relay policy, or product defaults. It has no platform-runtime dependency and no leaf-app
product policy. App-specific behavior requested by one downstream is evidence of demand, not
permission to add it here.

## App Product Policy Stays in App Crates

A request from one downstream app to add product behavior (relay defaults, feed seeds,
onboarding flows) to an NMP crate is evidence of demand, not permission to add app-named
helpers or product policy to shared crates (D0 corollary). Operator relay policy is declared
in an app config surface (the audited D3 opt-out — see the scanner's relay-policy handling),
not in `nmp-defaults`.

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
- A production app root calls `register_defaults()` without a compatibility label, named
  owner, and deletion gate.
- The composition root is absent (all wiring hidden inside a helper that is not the NMP
  installer library).
- App product policy (relay URLs, seed follows, onboarding, signer perm defaults) lives in an
  NMP crate instead of the app crate.
- A new NMP crate is created to hold features specific to one downstream app.
- Named protocol installers are omitted but the app expects the features they install (silent
  misconfiguration — verify via the composition ledger).

Design questions for any new app root:
- Can a reader name every installed substrate, protocol feature, and app feature from
  `register()` alone?
- Is all product policy owned by the app crate, not `nmp-defaults` or any shared NMP crate?
- Are late-wiring risks eliminated (all registrations before `start()`)?
