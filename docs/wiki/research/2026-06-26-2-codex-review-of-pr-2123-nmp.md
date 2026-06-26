---
type: research-record
date: 2026-06-26
session: 1077a92b-e2b0-457d-870e-5e12e4f524cf
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/1077a92b-e2b0-457d-870e-5e12e4f524cf.jsonl
source_lines: 4016-4062
agent_attribution: codex
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Codex review of PR #2123 nmp-nip46 extraction: verifies behavior-preservation of deduped kind:24133 wire-builder and FrameSink bridge; FIX-FIRST verdict on error-string drift requiring correction"
captured_at: 2026-06-26T09:01:59Z
---

Codex review of PR #2123 nmp-nip46 extraction: verifies behavior-preservation of deduped kind:24133 wire-builder and FrameSink bridge; FIX-FIRST verdict on error-string drift requiring correction

---

Assistant: PR #2123 (nmp-nip46 extraction) is converging:
- **Real fix in progress** — agent registering `nmp-nip46` in `release/nmp-release.toml` (the one true CI failure).
- **Flaky Swift drift** — re-running (it was a crates.io `curl` blip, not a real drift).
- **Codex review** — running on behavior-preservation + the deduped kind:24133 wire-builder (a changed signed frame would be the one thing that silently breaks NIP-46, so that's the focus).

I'll merge once the manifest fix lands, CI is green, and codex gives GO. Then Step 2 (the event-reducer rewrite that removes the blocking `crossbeam` carry) and onward through the decoupling. Continuing automatically as signals arrive.

User: [Agent task result: Agent "Extract nmp-nip46 protocol core" finished]
Pushed. Done.

**Fix applied and pushed to `feat/2119-nmp-nip46-core` (commit f1edd940a):**

- Added a `[[public_crates]]` entry for `nmp-nip46` (`name = "nmp-nip46"`, `path = "crates/nmp-nip46"`) in `release/nmp-release.toml`, placed before the `nmp-nip47` entry to match the surrounding nip-crate ordering.
- Release-manifest check (the exact command the CI step runs, `bash ci/check-release-manifest.sh`) now passes: **"release manifest ok: 54 public crates; every workspace package classified"**, exit 0. It no longer lists `nmp-nip46` as missing.
- `cargo build -p nmp-nip46` still green.

Not merged. The Swift codegen-drift failure is the unrelated flaky crates.io network issue you flagged, left untouched.

Assistant: Manifest fix pushed (release-manifest check now passes: "54 public crates; every workspace package classified"). Let me check CI + the codex review.

User: [Tool result]: chpwd:2: command not found: _iterm2_set_profile
=== CI tally (after manifest fix + rerun) ===
   8 pass
  13 pending

=== codex review status ===
chpwd:2: command not found: _iterm2_set_profile

Assistant: CI is re-running clean so far (8 pass, 13 pending, 0 fail). Let me watch CI to completion while the codex review finishes.

User: [Tool result]: Command running in background with ID: b7b27mvo5. Output is being written to: /private/tmp/claude-501/-Users-pablofernandez-Work-nostr-multi-platform/1077a92b-e2b0-457d-870e-5e12e4f524cf/tasks/b7b27mvo5.output. You will be notified when it completes. To check interim output, use Read on that file path.

Assistant: PR #2123 manifest fix landed; CI re-running clean and being watched, codex reviewing the extraction's behavior-preservation. I'll merge once both clear, then continue the decoupling (Step 2: the event-reducer rewrite removing the blocking `crossbeam` carry). Continuing automatically when the signals arrive.

User: [Tool result]: 1	chpwd:2: command not found: _iterm2_set_profile
2	├── crossbeam-channel v0.5.15
3	├── nostr v0.44.3
4	├── serde v1.0.228
5	└── serde_json v1.0.150
6	
7	hook: PostToolUse Completed
8	codex
9	FIX-FIRST. No signed-wire BLOCKER found, but there is one behavior-preservation drift.
10	
11	Findings:
12	- SHOULD-FIX [crates/nmp-signer-broker/src/transport.rs](/private/tmp/nmp-pr2123-review/crates/nmp-signer-broker/src/transport.rs:126) and [crates/nmp-nip46/src/nostrconnect.rs](/private/tmp/nmp-pr2123-review/crates/nmp-nip46/src/nostrconnect.rs:80) - error mapping changed. `send_rpc` now wraps build failures as `build frame: nip44 encrypt: ...` instead of the prior `nip44 encrypt: ...`; nostrconnect ACK build failures also lose the prior ACK-specific strings (`nip44 encrypt ack`, `sign ack event`, `serialize ack`). Wire output is unchanged, but STEP 1 says behavior-preserving including error mapping. Map `RpcBuildError` at call sites to the exact previous strings.
13	
