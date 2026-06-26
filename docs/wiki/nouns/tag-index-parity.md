---
type: noun-entry
slug: tag-index-parity
name: "tag-index parity"
origin: extracted
source_refs:
  - transcript:229-237
---

# tag-index parity

requirement to reproduce LMDB's tci/atci/ktci sub-DB index patterns in SQLite (composite indexes on tag_letter, tag_value, created_at) so scan_by_tags stays index-served and never full-table-scans
