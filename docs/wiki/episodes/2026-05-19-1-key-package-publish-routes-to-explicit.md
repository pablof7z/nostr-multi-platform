---
type: episode-card
date: 2026-05-19
session: 3ed0a030-6daf-4680-9172-992f98deb328
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/3ed0a030-6daf-4680-9172-992f98deb328.jsonl
salience: root-cause
status: active
subjects:
  - marmot-publish-key-package
  - relay-routing-doctrine
supersedes: []
related_claims: []
source_lines:
  - 10525-10540
  - 10560-10576
captured_at: 2026-06-18T04:22:03Z
---

# Episode: Key-package publish routes to explicit write-relays, not NIP-65 outbox

## Prior State

publish_key_package resolved write-relays from relay_edit_rows but then discarded them, publishing via publish_author_outbox (NIP-65 Auto/outbox routing). If the user had no kind:10002 event, the NIP-65 resolver returned empty, producing 'active account has no write-relays declared' even when write relays were configured in Settings.

## Trigger

User reported 'active account has no write-relays declared' error when pressing Publish key package despite having write relays configured. Root-cause traced: publish_key_package already had the correct relay URLs from write_relay_urls() but routed them through NIP-65 Auto, which requires a kind:10002 event to resolve targets.

## Decision

Changed publish_key_package to call publish_explicit with the already-resolved relay list from write_relay_urls() instead of publish_author_outbox. The relays are now .clone()d so they survive past the service call. Key-package events (kind:30443 + legacy kind:443) are published directly to the user's configured write-relays, bypassing the NIP-65 outbox resolver entirely.

## Consequences

- Key-package publish now works for users who have write-relays configured in Settings but no kind:10002 NIP-65 relay list event on the network
- Key packages are no longer semantically 'author outbox' traffic — they route to explicit write-relays from user configuration
- Doc comment updated from 'publish_author_outbox (Auto / NIP-65 outbox is correct for key packages)' to 'publish_explicit (to the user's configured write-relays from Settings)'
- publish_author_outbox and publish_auto are now dead code in marmot/publish.rs (compiler warnings surfaced)

## Open Tail

- Dead code in marmot/publish.rs (publish_auto, publish_author_outbox) should be removed or gated
- The 'invalid relay role — expected read | write | both' error message in normalize_role still doesn't list 'indexer' as accepted

## Evidence

- transcript lines 10525-10540
- transcript lines 10560-10576

