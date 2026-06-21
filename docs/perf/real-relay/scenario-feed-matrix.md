---
scenario: feed-matrix
verdict: PASS
generated_at: 1782010046
relays: ["wss://relay.damus.io", "wss://nos.lol", "wss://relay.primal.net", "wss://purplepag.es"]
---

# Scenario — declared feed matrix

## Verdict: PASS

Fetched real kind:3 for `pablo-provided` from `wss://relay.damus.io` with 1054 followees; sampled 24.

Assertions:

- primary social declaration `[1]` compiled to acquisition kinds `[1,6]` and never `[1,6]` as app-owned primary policy.
- sampled real follow set became the exact compiled REQ author set.
- mutating the real follow set by `- 000000000652e452ee68a01187fb08c899496cb46cb51d1aa0803d063acedba7` / `+ deadbeef00000000000000000000000000000000000000000000000000000000` changed the author filter and plan id `1876f7169a938457` -> `0f7e1ea6737072d1`.
- live social query returned 12 real kind:1/kind:6 events matching the compiled authors/kinds.
- relay-set kind:30023 feed compiled to app relays with no authors filter; live no-author query returned 12 real kind:30023 events.
- live kind:20 query returned 12 events; NIP-68 picture adapter rendered 5 rows from parsed feed data.
- live kind:16 query returned 40 events; 0 claimed a kind:20 target via `k` tag or embedded event.
- caller-owned custom ranking/filtering ran over 12 real events and produced 12 bounded feed rows with page limit 2.

Kind:16 picture repost observation is reported as evidence, not a hard public-relay invariant: absence means this relay sample did not serve that shape within budget, not that the adapter path is green by itself.
