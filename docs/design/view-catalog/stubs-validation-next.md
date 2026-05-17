# View Catalog: Stubs, Validation, Next Steps

[Back to Design: View Catalog](../view-catalog.md)

## 9. Stubs — view kinds to be filled in during Phase 4

For each stub: spec name, scope, deferred-to-Phase note. Full per-template detail follows the §3–§7 pattern during Phase 4 implementation.

### Contacts

Parsed kind:3 follow list for one pubkey. Payload: `ContactsView { pubkey, follows: Vec<ContactEntry>, raw_event_id }`. ContactEntry has pubkey + (resolved via projection) display name + relay hint. Phase 1.

### Mailboxes

Parsed kind:10002 for one pubkey. Payload: `MailboxesView { pubkey, inbox_relays: Vec<String>, outbox_relays: Vec<String>, raw_event_id }`. Phase 1.

### Mutes

Parsed kind:10000 for the active account. Payload: `MutesView { muted_pubkeys: Vec<PubKey>, muted_hashtags: Vec<String>, muted_events: Vec<EventId>, muted_words: Vec<String> }`. Affects projection of "should this event be filtered out of timeline?" Phase 2.

### Blossom servers

Parsed kind:10063 for one pubkey. Payload: `BlossomServersView { pubkey, servers: Vec<String> }`. Used by media upload action. Phase 6.

### Replies

Flat list of replies to one event (subset of Thread without tree structure). Payload: `RepliesView { target, replies: Vec<TimelineItem>, has_more }`. Cheaper than Thread when UI doesn't need the tree. Phase 1.

### Conversation list

List of DM conversations for the active account, with last-message preview + unread count. Payload: `ConversationListView { conversations: Vec<ConversationSummary>, total_unread: u32 }`. Phase 5.

### Zap history

Bidirectional list of zaps sent/received for the active account. Payload: `ZapHistoryView { sent: Vec<ZapEntry>, received: Vec<ZapEntry>, cursor }`. Phase 6.

### Wallet balance

Reactive balance + pending transactions for the active wallet. Payload: `WalletBalanceView { sats, pending: Vec<PendingTx>, last_synced_at_ms }`. Backed by the wallet subsystem rather than the event store directly. Phase 6.

### WoT rank

Per-pubkey trust score + reasoning. Payload: `WotRankView { pubkey, score, depth_paths: Vec<DepthPath> }`. Backed by the WoT subsystem. Phase 6.

### Search

Full-text or filter-based search over the local store. Payload: `SearchView { query, results: Vec<TimelineItem>, has_more }`. **Heavy `catch_all_filter` use** — every insert evaluates against the query. Phase 1 with explicit guardrail warning.

---

## 10. What this catalog rules out

- **Consumer-defined view kinds.** v1 owns the enum. Per spec §13 open question 6, this may relax in v2 via either enum extension (awkward) or string-keyed payloads (consumer-decoded). Decide post-v1.
- **Mutable view payloads.** View payloads are immutable snapshots. Deltas describe the difference between snapshots. No "patch this field in place from native" path exists.
- **View-on-view subscriptions.** Per `reactivity.md` §6.3.
- **Computed-from-native fields.** All formatting, all derivations live in Rust per doctrine D5.

---

## 11. Validation: scenarios that must run before view-catalog ships

Five scenarios that must pass in the `reactivity-bench` harness (per `reactivity.md` §10) before Phase 4 of the build plan exits:

1. **Profile fan-out under kind:0 arrival.** 50 timeline views, each over 1k authors, with 50% author overlap; a single kind:0 arrives for a shared author. Measure: how many views are woken (should be ≈25); per-view recompute time; total `ViewBatch` latency. Gate: p99 ≤ 5ms end-to-end.
2. **Hashtag firehose.** 1 timeline view with `catch_all_filter` on `#nostr`; 200 events/sec. Measure: per-event filter eval time; ViewBatch frequency; ViewBatch size. Gate: ≤ 60Hz, ≤ 1000 deltas/sec cumulative.
3. **Thread orphan storm.** Thread view; 1000 replies arrive in random order; 50% of parents arrive after their children. Measure: orphan promotion rate; final tree correctness; tree-build time. Gate: tree is identical to a known-good single-pass build; build time ≤ 50ms.
4. **Reactions aggregation.** 1 reactions view; 10k kind:7 events arrive in 30 seconds. Measure: `EmojiAdjusted` delta count vs ideal coalesced count. Gate: deltas/sec ≤ 60.
5. **Conversation paging.** 1 conversation view; 100 historical decryptions triggered by `Prepended`; 5 incoming decryptions interleaved. Measure: decrypt throughput; ordering correctness; UI thread stays unblocked. Gate: no actor-thread starvation observed (other view updates continue to land during decrypt batch).

Each scenario is a `tests/` integration test in `nmp-testing` that asserts on the metric. Failures block Phase 4 exit.

---

## 12. Next step: run the five scenarios above against the reactivity harness

The `reactivity-bench` harness from `reactivity.md` §10 covers the framework-wide reactivity assumptions. This view-catalog adds **five view-kind-specific scenarios** (§11 above) that must pass before declaring the catalog ready. They reuse the same harness infrastructure with view-specific scenario configs.

Order of operations, concretely:

1. Build the harness (`reactivity.md` §10) at start of Phase 1.
2. Lock in the reverse-index + projection design based on harness measurements.
3. Implement view kinds 1–9 marked "Phase 1" in §2 of this doc (Profile, Contacts, Mailboxes, Timeline, Thread, Replies, Reactions, Search; Conversation/list deferred to Phase 5).
4. Run the five scenarios in §11 against the Phase 1 views.
5. If gates pass, Phase 1 exits. If not, fix the offending view kind's `recompute` strategy and re-run.

The catalog is a contract; the harness is how we know the contract holds.
