---
type: episode-card
date: 2026-06-18
session: 11850f79-923f-4a2a-a921-a4b9bec47c6c
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/11850f79-923f-4a2a-a921-a4b9bec47c6c.jsonl
salience: reversal
status: superseded
subjects:
  - presentation-formatting
  - d1-doctrine
  - aim-md-§2
supersedes:
  - 2026-05-26-2-d6-display-separation-doctrine-enforced-display
related_claims: []
source_lines:
  - 195-222
captured_at: 2026-06-18T19:42:43Z
---

# Episode: aim.md §2 confirms presentation formatting in Rust projections is a violation, not doctrine

## Prior State

In-code comments throughout nmp-core (publish_outbox, relay_diagnostics, nmp-marmot, nmp-nip29, nmp-nip01) cited "doctrine §4.4" and "V-24" / "RMP bible commandment #4" as justification for keeping English labels, SF Symbol names, bech32 strings, emoji, pluralization, and formatting helpers inside Rust projection builders and FFI serialization paths. The working assumption was that this was an intentional thin-shell policy.

## Trigger

P1 agent investigated the apparent doctrine tension and found that aim.md §2 ("Anti-patterns the framework must prevent") explicitly forbids presentation formatting in projection builders, snapshot types, and FFI paths — and names short_npub, avatar_initials, and format_ago_secs as legitimate only in TUI/CLI/tests. The in-code citations of "§4.4" are bogus: §4.4 is about NIP-65 outbox routing, not presentation.

## Decision

The #1493 audit ENFORCES aim.md §2; the "thin-shell / glyph-stays-in-Rust" comments are the violations, not the policy. All P1 findings (SF Symbols in nmp-core, npub bech32 in profile_display, author_display_name/author_picture_url redundant mirrors, bucket_age/render_subtitle, emoji/pluralization/initials in nip29 discovered, publish_outbox titles/labels, relay_diagnostics formatting) are confirmed §2 violations to be moved to shells.

## Consequences

- All five P1 slices proceed as §2 enforcement, not doctrine reversal
- P9 signer labels (signer_apps_table, signer_state_label) are also §2 violations, absorbed into P9 lane
- P4 Finding 3 (SignInScreen signerKind label) reassigned to P9 as the labels-to-shells vertical owner
- The bogus in-code "§4.4" / "V-24" citations will be removed as each slice lands
- relay_diagnostics *_tone semantic-token selectors are kept (they emit raw tokens, not colors/prose)

## Open Tail

- P1 publish_outbox and relay_diagnostics slices held behind PR #1525 (types.rs/generated-bindings collision)
- Full removal of all §2 violations across the codebase is multi-PR; some slices still in CI

## Evidence

- transcript lines 195-222

## Conversation

- Cleaned transcript (verbatim user words, abbreviated agent replies): [`transcripts/2026-06-18-1-aim-md-2-confirms-presentation-formatting.json`](transcripts/2026-06-18-1-aim-md-2-confirms-presentation-formatting.json)
- Raw transcript (verbatim user words, full agent replies): [`transcripts/raw/2026-06-18-1-aim-md-2-confirms-presentation-formatting.json`](transcripts/raw/2026-06-18-1-aim-md-2-confirms-presentation-formatting.json)
