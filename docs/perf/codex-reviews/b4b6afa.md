# Codex review — b4b6afa

**Merge:** docs(m11): fix codex feedback (file sizes, harness, D5, D4, rig.rs gate, OPML, counts)

## Findings

1. **copy.md §0a.3 + inventory.md §I: split math wrong** — tables added 6 DiscoverView + 3 PlayerSheet siblings = 29 total (not 27). Also AskView 322, DiscoveryCards 302, ChaptersPanel 324 need explicit soft-limit exception justification. **Fixed in follow-up commit 151c80c.**

2. **copy.md §0a.4: "Diffs must be empty" git-diff-stat check is not a proof** — git diff detects view files changed (the split itself changes them). Replace with text-reconstruct diff + xcodebuild + screenshot gate. **Fixed in follow-up commit 151c80c.**

3. **podcast-llm.md §I conflicts with §K rig.rs gate** — §I said "Settings view post-M11 / AppleIntelligence-only on iOS" while §K required rig.rs in M11 CI against a real endpoint. Fix: inject keys via env var at test time. **Fixed in follow-up commit 151c80c.**

No extra TODO/FIXME/unimplemented found. No file crossed 500 LOC hard limit. capabilities.md and podcast-core.md remained over 300 soft limit but this predated the merge. No new kernel/FFI ADR required.
