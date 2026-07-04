---
type: noun-entry
slug: release-manifest
name: "release manifest"
origin: extracted
source_refs:
  - transcript:243-243
---

# release manifest

The release-readiness source of truth (release/nmp-release.toml); the dry-run runs `cargo package -p` over every public_crate, and external consumers pin public crates by git rev + version.
