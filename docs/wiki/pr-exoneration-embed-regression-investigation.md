---
title: PR Exoneration — Embed-Regression Investigation and Vindication
slug: pr-exoneration-embed-regression-investigation
summary: Two merged PRs (#825, #836) were wrongly suspected of causing embed-loading regression; diff-level investigation exonerated both. The real cause was data-reachability.
tags:
  - regression
  - exoneration
  - embed
  - investigation
  - PR-825
  - PR-836
volatility: cold
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# PR Exoneration — Embed-Regression Investigation and Vindication

> Two merged PRs (#825, #836) were wrongly suspected of causing embed-loading regression; diff-level investigation exonerated both. The real cause was data-reachability.

## Overview

During the backlog wave, a peer agent suspected two merged PRs — #825 (V-68 orphan ingest deletion) and #836 (V-52 single-relay browsing) — as the root cause of an embed-loading regression where embeds were stuck loading. Both were investigated with concrete diff-level evidence and exonerated. The real cause was later found by the peer to be a data-reachability issue: showcase events only exist on nos.lol, not the seeded relays (purplepag.es + primal.net). [^4edd4-163]


After both #825 and #836 were exonerated via diff-level investigation, the peer independently found the real root cause: showcase events missing from the seeded relays (purplepag.es + primal.net), only present on nos.lol. This is a data-reachability / relay-config decision, not a code bug. The exoneration evidence for #836 was posted directly on the PR itself — that's where a regression bisect would look — to save both the peer's cycles and the merged work from wrongful reversion. This procedure (post exoneration evidence on the PR a bisect would land on) is the correct channel for defending merged work from wrong hypotheses. [^4edd4-198]
## #825 Exoneration — Orphan Ingest File Deletion

The deleted files (ingest/event.rs and ingest/eose.rs) were provably uncompiled: mod event and mod eose were NOT declared in ingest/mod.rs, so the files never contributed to the build. All claim/embed functions still exist in their real compiled locations: handle_event/verify_and_persist/on_mailbox_changed in ingest/mod.rs, and claim_expansion_match_author/record_claim_expansion_hit in relay_score_record.rs. The deleted event.rs was a stale duplicate of these. cargo build -p nmp-core succeeded on master after the merge, and #825 passed the full cargo-test CI gate. [^4edd4-164]

## #836 Exoneration — Single-Relay Browsing

V-52's mem/insert.rs change is purely additive and mechanical — it only adds relay_index_add/relay_index_remove calls alongside existing provenance logic, plus a borrow-checker refactor (sources_after block) that is behaviorally identical to the old p.len(). The actual event storage (st.events.insert), the claimed_events projection, and the claim_event request path are all untouched — none of them read relay_index. V-52 cannot have broken embed-loading. [^4edd4-165]

## Process — Posting Exoneration to PRs

When a peer is running a regression bisect that will likely land on a merged PR, post the exoneration evidence directly on the PR itself — that's where the bisect will look. This saves both the peer's cycles and the merged work from wrongful reversion. V-52 exoneration evidence was posted on PR #836 with the diff-level proof. [^4edd4-166]

## Real Root Cause (Found by Peer)

The peer found the real embed-regression cause independently: showcase events missing from the seeded relays (purplepag.es + primal.net), only on nos.lol. This is a data-reachability / relay-config decision, not a code bug. Both #825 and #836 were fully vindicated. [^4edd4-167]

## See Also

