---
type: noun-entry
slug: regen-flatbuffers-sh
name: "regen-flatbuffers.sh"
origin: extracted
source_refs:
  - transcript:1119-1133
---

# regen-flatbuffers.sh

A script in chirp's apps/ios/scripts/ that regenerates the checked-in Swift FlatBuffers types for Chirp's typed wire decoders. Created (chirp#35) to make reproducible what was previously done by hand-running flatc --swift against the relevant .fbs schemas from the pinned NMP checkout.
