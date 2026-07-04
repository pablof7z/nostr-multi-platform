---
type: noun-entry
slug: notefeeditem
name: "NoteFeedItem"
origin: extracted
source_refs:
  - transcript:2391-2393
---

# NoteFeedItem

The renamed TimelineEventCard — same fields (id, author_pubkey, kind, created_at, content, content_tree, relay_provenance), carries reposted_by: Option<RepostAttribution>, minus only relation_counts (removed by the collapse) plus a new hosted_group field. Lives in nmp-note-feed behind nmp-feed-session.
