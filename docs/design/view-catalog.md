# Design: Projection Catalog

The projection catalog is the set of projections registered by
protocol/defaults/app crates.

## Current Sources

- `nmp-defaults` wires the standard projection set used by default NMP apps.
- Protocol crates own protocol-specific projection payloads and reducers.
- App crates own product-specific projection payloads and reducers.
- Hosts consume snapshots and typed sidecars by projection key.

Projection keys and payload schemas are defined beside the code that owns the
projection. Do not add a central catalog row unless the projection is genuinely
part of the shared framework surface.
