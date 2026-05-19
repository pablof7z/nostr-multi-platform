---
scenario: 2-outbox-real-kind10002
verdict: PASS
generated_at: 1779088948
relays: ["wss://relay.damus.io"]
---

# Scenario 2 — NIP-65 outbox routing vs real kind:10002

## Verdict: PASS

Fetched author **jb55** (`82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2`) live kind:10002 from `wss://relay.damus.io`, inserted the real signed event into a `MemEventStore`, and resolved `PublishTarget::Auto` through `Nip65OutboxResolver::with_default_fallback`.

The resolved relay set is **exactly** the author's declared write-relay set (6 relay(s)) — proving NIP-65 outbox routing against live network data, not the indexer fallback.

- author: `82341f882b6eabcd2ba7f1ef90aad961cf074af15b9ef44a09f9d2a8fbfbe6a2` (jb55)
- source relay: `wss://relay.damus.io`
- declared write-relays:
- `wss://nos.lol`
- `wss://nwc.primal.net/ayvjleilmx0al7j2pqt24qed1z7a8s`
- `wss://relay.damus.io`
- `wss://relay.mostr.pub`
- `wss://relay.nos.social`
- `wss://relay.primal.net`
- resolved == declared write-set: ✅ (BTreeSet equality)
- indexer fallback (non-overlapping URLs) absent: ✅

