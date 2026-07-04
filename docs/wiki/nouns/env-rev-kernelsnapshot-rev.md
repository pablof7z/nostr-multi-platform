---
type: noun-entry
slug: env-rev-kernelsnapshot-rev
name: "env.rev (KernelSnapshot.rev)"
origin: extracted
source_refs:
  - transcript:3687-3688
---

# env.rev (KernelSnapshot.rev)

A frame-level counter that bumps on every emitted kernel snapshot tick; hosts gate on `env.rev > rev` to skip stale frames. Frozen env.rev means the host received no new frames.
