---
type: noun-entry
slug: embedkindprojection
name: "EmbedKindProjection"
origin: extracted
source_refs:
  - transcript:355-360
---

# EmbedKindProjection

An enum in nmp-content (embed_projection/variants.rs) with a typed XProjection struct per event kind, dispatched by a single match on event.kind (resolve_embed_projection); every variant must also be wired through FlatBuffers embed_sidecar wire, platform renderers, gallery previews, and registry manifests.
