---
type: research-record
date: 2026-07-04
session: dcc80382-bcc0-45ea-8b9c-1a2fc741f872
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/dcc80382-bcc0-45ea-8b9c-1a2fc741f872.jsonl
source_lines: 6565-6584
agent_attribution: ios-render-fixer
has_preregistered_criteria: true
has_method: true
has_structured_report: true
characterization: "Render-fix reverification of PR #73: 4/6 bugs FIXED with screenshot proof (a11y 0→138 elements, pbpaste hashtag tap test), 2/6 honestly STILL BROKEN root-caused to NMP#3016, CI green, merge recommendation"
captured_at: 2026-07-04T18:32:03Z
---

Render-fix reverification of PR #73: 4/6 bugs FIXED with screenshot proof (a11y 0→138 elements, pbpaste hashtag tap test), 2/6 honestly STILL BROKEN root-caused to NMP#3016, CI green, merge recommendation

---

<teammate-message teammate_id="ios-render-fixer" color="pink" summary="4/6 reverified fixed, 2/6 honestly still broken, CI green">
Reverification complete on PR #73 (branch tip 183950221a04c1f4891bf8bb05a8bdeac8931abb). Honest per-bug result:

FIXED — confirmed with dedicated after-screenshots + live interaction where possible:
- #62 (link): screenshot shows the link colored/underlined/truncated, flowing inline. Confirmed as a real accessibility `button` (identifier `inline-link`), not dead text.
- #63 (video): screenshot shows a clean "Play" button, not the reported unavailable/slash icon. Real button element (`inline-video-play-button`).
- #64 (hashtags): screenshot shows 3 pill chips flowing inline (not one-per-line). Also live-tap-tested: tapped the `#nostr` chip via the accessibility tree, then read the simulator pasteboard directly (`xcrun simctl pbpaste`) and got back `#nostr` — proves the tap is wired end-to-end, not just styled.
- #67 (a11y): fresh `snapshot_ui` capture on Home feed returned 138 elements (was 0 before). JSON dump committed as evidence.

STILL BROKEN — said so honestly on the issues, did NOT claim these are fixed:
- #65 (quote card): Chirp-side card shape is now correct (real `ShortNoteEmbed`, not the old stub) but the resolved envelope still comes back with empty author name/content for this specific note, even though that same author resolves fine everywhere else in the app. This is a Rust-side embed-resolver data bug, not fixable from Chirp. Screenshot shows the card still only displays avatar + short pubkey.
- #66 (article): still shows the raw image URL as the headline instead of the title. `ArticleEmbed` is correctly wired and its code is right — the Rust resolver hands Swift the wrong field value. Screenshot confirms unchanged.

Both #65/#66 root causes are filed as pablof7z/nostr-multi-platform#3016 with exact repro steps and suspected cause (FlatBuffers field mismatch for article; missing author-resolution chain for shortNote quotes).

Posted a comment with the specific after-screenshot (or still-broken screenshot) on every one of issues #62, #63, #64, #65, #66, #67 individually.

CI on #73: all 7 checks green (cargo build+test workspace, iOS/Android UniFFI binding drift checks, chirp-tui smoke, architecture scan ratchet, generated-config drift checks) — the workspace build+test was pending when you messaged, I watched it to completion and it passed.

Recommendation: #73 is safe to merge for #62/#63/#64/#67. #65 and #66 should stay open against nostr-multi-platform#3016 until the Rust resolver is fixed — merging #73 doesn't complete those two, and I did not use "Closes" keywords for them.
