# Design: App Extension Boundary

NMP is a reusable Nostr application substrate, not a social-client core with
app-specific nouns baked into `nmp-core`.

## Current Boundary

`nmp-core` owns generic infrastructure:

- actor runtime and reducer-owned state,
- verified event ingest and storage,
- subscription compilation and relay routing,
- publish orchestration,
- capability request/result plumbing,
- action registration and dispatch,
- snapshot and typed-projection emission,
- diagnostics and doctrine gates.

Protocol crates own reusable Nostr concepts such as NIP-17 DMs, NIP-29
groups, NIP-47 wallet actions, NIP-57 zaps, NIP-65 mailbox routing, NIP-77
coverage, and Blossom.

App crates own app domain concepts. A podcast episode, Highlighter artifact,
TENEX workspace, daily plan, or weight log belongs in that app's Rust core,
not in `nmp-core`.

Native shells render state, execute OS capabilities, and hold ephemeral
presentation state only.

## Extension Seams

The shipped seams are:

- `ActionModule` plus `register_action` for typed write intents.
- declared observed projections for event-driven, scope-bound read-model updates
  with cache replay before activation.
- `register_typed_snapshot_projection` and typed projection registration for host
  state output.
- `CapabilityModule` and capability sockets for native facts.
- `NmpAppBuilder`, `AppHost`, and `nmp-defaults::register_defaults` for
  composition.
- app/protocol-owned Rust state where the concept is not a generic Nostr
  substrate concern.

## Rule

If a feature only makes sense for one app, implement it in that app's Rust
core. If it is a reusable Nostr mechanism, implement it once in the relevant
protocol/substrate crate. `nmp-core` does not learn app nouns.
