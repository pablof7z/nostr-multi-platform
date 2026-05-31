# ADR-0041 — Relay-Settings Cluster: Strip Presentation Strings (Completing ADR-0032)

**Status:** decided  
**Date:** 2026-05-31  
**Supersedes:** (partial completion of) ADR-0032  
**Cross-references:** ADR-0021 (RelayRole naming), ADR-0032 (raw-data-projection doctrine)

---

## Context

`RelayEditRow` in `nmp-core::kernel::identity_state` stored two display strings:
- `role_label: String` — e.g., `"Both + Index"`, `"Read"`, `"Write"`
- `role_tint: String` — e.g., `"accent"`, `"info"`, `"success"`

`SettingsHubSummary` stored a pre-formatted relay count string:
- `relays_subtitle: String` — e.g., `"3 relays"`, `"No relays configured"`

Both violated ADR-0032's rule: "Rust display helpers are legitimate only in TUI render code, CLI output, and test fixtures — never inside projection builders, snapshot types, or FFI serialization paths."

ADR-0032 explicitly deferred the relay-settings cluster as a known follow-up. This ADR closes that follow-up.

## Decision

1. **Strip `role_label` and `role_tint` from `RelayEditRow`.**
   The kernel struct now holds only `url: String` and `role: String` (canonical form).

2. **Delete `relay_role_label` and `relay_role_tint` free functions.**
   They had no callers after the struct fields were removed.

3. **Add `Nip65Role` struct** (three bools: `read`, `write`, `indexer`) to `actor/relay_roles.rs`.
   `canonical_relay_role` and `has_role` now delegate to it.
   Named `Nip65Role` (not `RelayRole`) to avoid collision with `nmp_network::RelayRole` (transport-lane discriminator — see ADR-0021).

4. **Replace `SettingsHubSummary` with `{ relay_count: N }`.**
   The `settings_hub` projection now emits a raw integer count. Shells format the pluralised string locally.

5. **`relay_role_options` is the canonical label/tint lookup table.**
   It was already a legitimate static options table at the projection boundary (ADR-0032 §"static enum-label tables"). Shells join on `role` → `value` to get the display label and tint.

## Consequences

- Kernel state is minimal: no presentation strings embedded in business-logic structs.
- The `relay_role_options` projection becomes the single source of truth for role display metadata.
- Each platform shell (Swift, Kotlin, TUI, TypeScript) looks up label/tint via `relay_role_options` join or a local mapping (TUI, which ADR-0032 explicitly permits).
- Wire output: `relay_edit_rows` items shrink from 4 fields to 2; `settings_hub` payload changes shape.

## Migration

No migration required: this is a clean break. All consumers updated in the same PR.
