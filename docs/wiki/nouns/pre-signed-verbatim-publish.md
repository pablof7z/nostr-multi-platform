---
type: noun-entry
slug: pre-signed-verbatim-publish
name: "Pre-signed verbatim publish"
origin: extracted
source_refs:
  - transcript:1052-1053
  - transcript:992-992
---

# Pre-signed verbatim publish

Publishing an already-signed event without re-signing, routed via the event's own pubkey outbox. Needed for protocol-owned events (e.g. Marmot/MLS wire events); WRITE-005 restricts it to those protocol seams so it can't be used as a general app write door.
