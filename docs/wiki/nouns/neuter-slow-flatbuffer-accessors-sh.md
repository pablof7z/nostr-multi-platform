---
type: noun-entry
slug: neuter-slow-flatbuffer-accessors-sh
name: "neuter-slow-flatbuffer-accessors.sh"
origin: extracted
source_refs:
  - transcript:1172-1183
  - transcript:1620-1624
---

# neuter-slow-flatbuffer-accessors.sh

An idempotent script in chirp's apps/ios/scripts/ that finds every FlatbufferVector<UInt8> accessor with a fast sibling (withUnsafePointerTo<Name>) in Generated/*.generated.swift and prepends an @available(*, unavailable, ...) annotation, turning any future per-byte-copy misuse into a compile error.
