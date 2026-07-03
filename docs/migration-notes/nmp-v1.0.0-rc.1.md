# nmp-v1.0.0-rc.1 Migration Note

This note is the consumer-facing migration checklist for the `nmp-v1.0.0-rc.1`
release tag — the first real published NMP release candidate. It complements
the durable target guide in `docs/migration.md`; the target guide explains the
final shape, while this file names the concrete breaks a pinned consumer must
handle when crossing this release.

## Deleted Or Renamed Crates And APIs

- Do not depend on `nmp-defaults` or a generated defaults bundle. App Rust code
  is the composition root and must explicitly install substrate, selected
  protocol crates, and app-owned modules.
- Do not use the deleted C/JNI `nmp-ffi` public binding surface as app API.
  Native consumers should move to the UniFFI objects and generated Swift/Kotlin
  bindings over `nmp-native-runtime`.
- Do not depend on `nmp-uniffi` (deleted #2763). The blessed pattern is an
  app-owned UniFFI facade over `nmp-uniffi-support`.
- App feeds use the declared-feed path. Replace specialized/default feed
  openers with `FeedKey::app(...)`, `feed::events()`,
  `source::active_user().follows()`, and `app.feeds().open_spec(...)`.

## Projection Keys And Schema IDs

- Profile UI reads keyed `refs.profile` rows. Do not read
  `resolved_profiles`, `claimed_profiles`, or `mention_profiles`.
- Event/embed UI reads authoritative `refs.event` rows and derived
  `refs.event.envelopes` render envelopes. Do not read
  `claimed_event_embeds` or parse whole-map event projections in the shell.
- App feed output keys are app-owned projection keys such as
  `FeedKey::app("app.example.home")`. Do not use framework-owned singleton
  feed strings.

## Dispatch Envelopes And Actions

- Shells should not spell action namespaces or JSON payloads by hand for app
  writes. Use generated action builders that return `DispatchEnvelope` bytes.
- Browser writes go through `dispatch_bytes` / `handle_dispatch_bytes` with a
  finished FlatBuffers envelope (`NMPD`), not a wasm-only JSON write vocabulary.
- Native Swift/Kotlin writes go through the generated UniFFI dispatch-byte
  doorway.

## UniFFI And Binding Changes

- Swift/Kotlin consumers construct and hold the UniFFI runtime object instead
  of storing raw native pointers.
- Feed sessions return opaque handles. Hosts pass the handle back for
  `load_older` and `close`.
- Generated bindings are checked in and drift-gated. Regenerate bindings from
  this release when updating a pinned native consumer.
- Public npm packages publish under the `@nmpis` scope
  (`@nmpis/runtime-web`, `@nmpis/components-web`), not `@nmp`.
- This is the first release where internal `nmp-*` crate dependencies carry
  real version requirements; `cargo publish`/`cargo add nmp-core@1.0.0-rc.1`
  now resolves against crates.io instead of requiring workspace path deps.

## Consumer Checklist

- Re-pin NMP, then run `nmp upgrade --to 1.0.0-rc.1` in the app root so
  app-module `nmp-*` dependencies move to the release tag shape.
- Replace defaults-bundle setup with an explicit Rust composition root.
- Replace raw feed/open-interest code with the declared-feed API and app-owned
  `FeedKey`.
- Replace legacy projection mirrors with `refs.profile`, `refs.event`, and
  `refs.event.envelopes`.
- Replace handwritten action namespace dispatch with generated
  `DispatchEnvelope` builders.
- Replace C/JNI public binding calls with UniFFI runtime calls or app-owned
  shell glue that wraps the UniFFI/native runtime API.
- Run the app crate tests, native/web binding generation checks, and
  `cargo test -p nmp-testing --test doctrine_lint_smoke` before treating the
  re-pin as complete.
