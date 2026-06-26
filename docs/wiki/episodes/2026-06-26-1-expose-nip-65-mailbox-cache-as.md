---
type: episode-card
date: 2026-06-26
session: f5b01ea3-5ad8-4098-bcb1-d90be8d1f124
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/f5b01ea3-5ad8-4098-bcb1-d90be8d1f124.jsonl
salience: product
status: active
subjects:
  - nip65-mailbox-cache
  - app-core-interface
  - substrate-composition
supersedes: []
related_claims: []
source_lines:
  - 21-25
  - 259-311
  - 391-434
  - 657-678
  - 791-808
captured_at: 2026-06-26T11:40:19Z
---

# Episode: Expose NIP-65 mailbox cache as public app-core read handle

## Prior State

The NIP-65 (kind:10002) mailbox cache was internal to NMP substrate composition — no public read path. Apps needing relay data either used legacy runtimes (e.g., Highlighter's HighlighterCore.fetchRelaysForPubkey) or spun up their own relay clients.

## Trigger

Issue #2085 explicit requirement: Highlighter's iOS relay-import cutover cannot eliminate the legacy HighlighterCore dependency without a "public, app-core-safe way to read the NMP-owned kind:10002/NIP-65 mailbox cache". Blocking pablof7z/hl#95 mobile NMP cutover.

## Decision

Expose the cache as a public read handle `NmpDefaultRuntimeHandles::mailbox_cache` (Arc<dyn MailboxCache>) returned by register_defaults_with_handles. Threaded through the composition tier: nmp_substrate_defaults::install_on_app_host → register_substrate → register_defaults_with_handles. Returns the same Arc instance wired to the kind:10002 parser writer, router, and NIP-19 encoder (instance identity provable via Arc::ptr_eq). Preserves read/write/both role shape via ParsedRelayList. Avoids new FFI surface or event-history leakage.

## Consequences

- Apps (Highlighter, others) can now read cached NIP-65 relay lists without legacy runtimes or custom relay clients, enabling pure NMP relay-import workflows
- Instance identity is guaranteed: the returned handle is the exact Arc wired to parser writer, routing factory, and encoder reader — no drift
- Read/write/both role separation is preserved via snapshot returning ParsedRelayList with read_set/write_set methods
- Establishes a pattern: substrate-internal caches can be exposed through the app-core handle surface (NmpDefaultRuntimeHandles) when producer/consumer instance identity is load-bearing
- No new FFI symbols or unsafe boundaries created — exposes only Arc<dyn MailboxCache> trait, not internals

## Open Tail

- Highlighter's implementation via this handle (pablof7z/hl#95) — outcome pending
- Whether other substrate caches (profile_cache, contacts_cache) should expose handles via the same pattern — architectural precedent set but not yet generalized

## Evidence

- transcript lines 21-25
- transcript lines 259-311
- transcript lines 391-434
- transcript lines 657-678
- transcript lines 791-808

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-26-1-expose-nip-65-mailbox-cache-as.json`](transcripts/2026-06-26-1-expose-nip-65-mailbox-cache-as.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-26-1-expose-nip-65-mailbox-cache-as.json`](transcripts/raw/2026-06-26-1-expose-nip-65-mailbox-cache-as.json)
