---
type: episode-card
date: 2026-05-21
session: 4f37753c-0654-4478-9c19-e799f1b10d39
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/4f37753c-0654-4478-9c19-e799f1b10d39.jsonl
salience: reversal
status: active
subjects:
  - chirp-tui-thread-rendering
  - nip10-dag-display
supersedes: []
related_claims: []
source_lines:
  - 593-594
  - 618-619
captured_at: 2026-06-18T05:00:59Z
---

# Episode: Nostr threads are DAGs not trees — flat depth-indent replaces tree pane

## Prior State

iamb-style thread pane (tree view splitting into a side panel) was the leading candidate from chat TUI prior art, treating Nostr threads like Matrix's tree structure

## Trigger

Chat TUI research agent identified that NIP-10 e-tags produce a directed acyclic graph, not a tree — multiple parents, merge events, and no clean parent-child hierarchy. Copying iamb's thread-pane tree rendering would misrepresent Nostr's actual data model.

## Decision

Use depth-indented flat view for thread rendering (Mastodon-style), defer true tree rendering. No side-pane thread tree.

## Consequences

- Thread view is simpler to implement but cannot show merge/reply-to-multiple-parents topology visually
- Depth-indent level derives from block structure's Module grouping, not explicit reply_to tags
- Tree-pane rendering remains on the backlog as a future enhancement if DAG visualization proves valuable

## Open Tail

- Whether to add visual indicators for DAG branches (e.g. 'replying to 2 parents') within the flat view

## Evidence

- transcript lines 593-594
- transcript lines 618-619

