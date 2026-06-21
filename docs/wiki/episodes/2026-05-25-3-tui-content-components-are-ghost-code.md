---
type: episode-card
date: 2026-05-25
session: e7a1d168-3c58-4438-a544-aa645850c388
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/e7a1d168-3c58-4438-a544-aa645850c388.jsonl
salience: architecture
status: active
subjects:
  - tui-content-registry
  - registry-completeness
supersedes: []
related_claims: []
source_lines:
  - 361-395
  - 414-418
captured_at: 2026-06-18T05:40:09Z
---

# Episode: TUI content components are ghost code — exist on disk but invisible to registry

## Prior State

The web registry TypeScript files (content.ts, user.ts, relay.ts) were assumed to accurately represent all platform implementations that exist in the codebase.

## Trigger

Audit discovered 6 TUI content components (~1,398 LOC across content-core, content-minimal, content-mention-chip, content-media-grid, content-quote-card, content-view) that exist as Rust source files on disk and are declared in registry.toml, but are NOT imported or exposed in the web registry TS files. TUI user components ARE properly wired.

## Decision

Wire the 6 missing TUI content component entries into web/registry/src/registry/content.ts so they become discoverable and installable via the registry.

## Consequences

- TUI content components will appear in the web registry for the first time
- TUI login-block and relay-list genuinely do not exist on disk — these remain gaps
- The registry.toml ↔ web TS sync is now known to be manual and error-prone

## Open Tail

- Whether to add a CI check that compares registry.toml declarations against web TS imports to prevent future ghost-code drift
- Web platform remains entirely empty (0 components, 0 files) — product decision needed on scope

## Evidence

- transcript lines 361-395
- transcript lines 414-418

