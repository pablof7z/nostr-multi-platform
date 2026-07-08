# Retired Documentation Notes

This directory holds short searchable notes for removed docs and removed API
surfaces. Current design guidance stays in the owning docs, ADRs, and code.

Do not use this directory to supersede current docs. If current guidance is
wrong, edit the owning document in place.

Current retired breadcrumbs:

- `op-centric-feed-architecture.md` — the demolished `RootIndexedFeed`/
  `NoteFeedItem`/`AttributionPayload` reply-rollup design (#3082/#3086).
  Superseded by [`docs/perf/composite-feed-architecture.md`](../perf/composite-feed-architecture.md).
- `removed-v2-traits.md` — removed trait-family / generated-app design names.
- `removed-api-surface.md` — removed generator and retired action transport notes.
- `lmdb-watermarks.md` — removed persisted-watermark and claim-register schema.
- `dispatch-actions-json.md` — old public JSON action namespace catalog.
- `codegen-v6.md` — removed v6 codegen plan.
- `ffi-hardening-m10-5.md` — removed M10.5 FFI hardening design tree.
