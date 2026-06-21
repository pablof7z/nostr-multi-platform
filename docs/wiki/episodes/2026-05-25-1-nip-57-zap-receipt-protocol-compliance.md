---
type: episode-card
date: 2026-05-25
session: 7b06d382-8fc2-4d52-bef5-f4d90e38cb2a
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/7b06d382-8fc2-4d52-bef5-f4d90e38cb2a.jsonl
salience: root-cause
status: active
subjects:
  - nmp-nip57
  - nip-57-zap-receipt
  - bech32-lnurl
supersedes: []
related_claims: []
source_lines:
  - 4944-5101
captured_at: 2026-06-18T05:23:10Z
---

# Episode: NIP-57 zap receipt protocol compliance — bech32 lnurl tag, callback param, receipt correlation

## Prior State

Zap requests embedded the raw lightning address (e.g. pablof7z@primal.net) as the lnurl tag; the LNURL callback URL omitted the &lnurl= parameter; wait_for_zap_receipt accepted any kind:9735 for the recipient (false positives). As a result, kind:9735 zap receipts were never published by LN providers.

## Trigger

User observed that despite successful NWC payments, no zap receipts appeared on relays. Root-cause analysis identified three separate failures: (1) NIP-57 Appendix B requires the lnurl tag to be a bech32-encoded HTTPS URL, not a raw address; (2) the LNURL callback must include &lnurl=<bech32> so the provider can associate payment with the Nostr account; (3) receipt detection was correlating on any kind:9735 for the recipient rather than on the bolt11 tag.

## Decision

Added url_to_bech32_lnurl() to nmp-nip57, inject_lnurl_tag() to inject a correct bech32 lnurl tag into kind:9734, appended &lnurl=<bech32> to the LNURL callback URL, and rewrote wait_for_zap_receipt to correlate on the bolt11 tag in kind:9735. D6 doctrine violation (.expect()) also fixed to .map_err().

## Consequences

- Zap receipts now appear on relays — live smoke test confirmed kind:9735 receipt 01ffc0a389715fcb with matching bolt11
- nmp-nip57 now depends on the bech32 crate for LUD-01 compliance
- Receipt correlation is now deterministic rather than best-effort

## Open Tail

- The bech32 lnurl tag injection is best-effort (non-fatal if encoding fails) — may silently drop the tag for malformed addresses

## Evidence

- transcript lines 4944-5101

