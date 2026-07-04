---
type: noun-entry
slug: available-unavailable
name: "@available(*, unavailable)"
origin: extracted
source_refs:
  - transcript:950-966
---

# @available(*, unavailable)

A Swift compiler attribute (normally used for platform/version availability) whose `unavailable` variant says the symbol can never be referenced from Swift source on any platform — enforced as a hard compiler error, not a warning. In this project, applied to generated FlatBuffers slow byte-vector accessors to make misuse a compile error pointing at the fast `withUnsafePointerToPayload` accessor instead.
