---
title: Projection Doctrine
slug: projection-doctrine
topic: projection-registry
summary: Host-declared projection subscriptions are rejected by ADR-0039 on the principle that the kernel must never know which view is open; this blanket ban conflates
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-06-13
updated: 2026-06-14
verified: 2026-06-13
compiled-from: conversation
sources:
  - session:78c8ec3a-f558-4738-98af-1f3af4978ec4
  - session:027459be-7102-4e1a-b6d4-02e8e7863642
  - session:da6b1d73-e1c8-4765-8ac7-056aa90fc154
  - session:2e5449b9-15e0-4d80-98a7-5281bda701d6
---

# Projection Doctrine

## Projection Doctrine

Host-declared projection subscriptions are rejected by ADR-0039 on the principle that the kernel must never know which view is open; this blanket ban conflates view-state leakage (a valid concern) with static interest declaration (the output-side sibling of push_interest), creating an internal inconsistency given the kernel already accepts host-declared interests for relays and events, causing all projections (including relay diagnostics) to be serialized and decoded on every tick regardless of whether the consuming screen is visible. (Previously: Host-declared projection subscriptions were rejected by ADR-0039.) However, the merged mechanism currently delivers zero optimization benefit: Chirp declares all 18 built-ins, so relay_diagnostics never stops shipping. ADR-0053 claims a drift-protection gate that does not exist: declared_projections is hand-maintained rather than generated from the codegen registry, and narrowing it would silently dark-screen affected screens. CHIRP_CONSUMED_BUILTIN_PROJECTIONS must be generated from the codegen registry (filtered to Tier-2) so it cannot drift from the decoded set. An empty declared set defaults to permitting everything with no debug_assert, lint, or one-time warn to surface the perf waste — a silent footgun that ADR-0053 promised to address as future work but never built. declare_consumed_projections is additive-only with no removal API; a mid-session call can only widen the emitted set, never narrow it, so it cannot cause a screen to go dark mid-flight. Relay diagnostics projection must ship raw timestamps, not pre-formatted relative-time strings, because per-second string churn violates aim.md §62 and guarantees perpetual re-serialization even when nothing real changed; RelayDiagnosticsRow must not embed pre-formatted relative-time strings (e.g. '3s ago') that change every wall-clock second, and the prohibition on format_ago_* helpers inside projection builders is upheld. The v0.5.0 release extends this display-formatting removal doctrine across all kernel projections: ProfileCard.npub is deprecated, and PublishOutboxItem loses created_at_display/target_summary in favor of raw created_at: uint64, enforced by a D19 doctrine-lint rule. The RelayDiagnostics Swift binding was stale after #1195/ADR-0051, missing the entire RelayDiagnosticsInfo table and info field, meaning Chirp iOS could not decode NIP-11 relay metadata. PR #1287 regenerates RelayDiagnostics.generated.swift to include the RelayDiagnosticsInfo table and info field. iOS EmbedHost.swift reimplements the Rust embed-projection resolver (switching on raw Nostr kind integers, parsing kind:0 JSON, extracting NIP-23 tags, detecting media URLs) in violation of D0 thin-shell doctrine. iOS ThreadNoteRow must use the Rust-emitted isRepost: Bool field from TimelineItem rather than re-deriving it from raw kind == 6 to avoid diverging from kernel semantics when Nostr adds new repost kinds. FlatBuffers Verifier (getCheckedRoot) must not run on the trusted in-process FFI decode path; all 35 decoders use unchecked getRoot instead. Expired pending ops must be evicted at snapshot edges (frequent wall-clock edges), not only at KP-ingest edges, to prevent perpetual hangs when no further KP events arrive. The D20 doctrine-lint rule bans OnceLock, lazy_static, and static …Mutex|RwLock|AtomicPtr holding non-const state in production nmp-* crates, with a justification-required allowlist that burns to zero. The D21 doctrine-lint rule bans correlation_id: Option<String> and String correlation params in nmp-core function/struct signatures outside the ActionTicket module. The D22 doctrine-lint rule bans calls to event-store newest-match queries from subs/recompile.rs and the planner after the presence heuristic is deleted, making the coverage ledger read the only legal floor source. An A5-style doctrine-lint extension bans notify_event_observers-bypassing store inserts outside the single ingest module, locking in the one-local-write-path invariant.

<!-- citations: [^2e544-437] [^78c8e-29] [^02745-15] [^02745-45] [^78c8e-56] [^78c8e-73] [^da6b1-75] [^78c8e-91] [^78c8e-111] [^2e544-454] [^2e544-470] -->
