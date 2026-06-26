---
scenario: reduced-source-nip65-reroute
verdict: PASS
generated_at: 1782447088
relays: ["wss://relay.primal.net", "wss://nos.lol"]
---

# ReducedSource NIP-65 reroute

## Verdict: PASS

Published the active user's kind:10000 source list and Bob's kind:10002 relay list to the source relay, published Bob's kind:1 note only to the target relay, configured the app with the source relay only, then observed the note in the decoded NOFS snapshot.

- source relay: `wss://relay.primal.net`
- target relay learned from kind:10002: `wss://nos.lol`
