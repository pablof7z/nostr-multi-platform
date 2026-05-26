# Product Spec: Appendices

[Back to Product Specification - Nostr Multi-Platform Framework](../product-spec.md)

## Appendix A. FFI architecture in detail

### A1. Why snapshots + ViewBatch (and not other patterns)

The bible mandates snapshots over FFI. For a Nostr client with timelines of thousands of events, naive full-snapshots are wasteful. We deviate as follows:

**`AppState` is bounded by what's open.** It does not contain the event store, the gossip cache, the working set, or anything proportional to the local cache size. It contains:

- Small screen-shaped state (router, session, busy flags, toast, wallet balance summary).
- A `HashMap<ViewId, ViewPayload>` populated only for currently-open views.
- Paginated view payloads (each bounded by the UI's actual rendering capacity).

The event store, gossip cache, sync watermarks, working set, and signer state all live in the actor and **never cross FFI**.

**Two outbound variants, one ordering.** `AppUpdate` carries either a full snapshot or a batch of view deltas. Both carry a monotonic `rev` and are encoded in the same canonical FlatBuffers runtime schema. Platforms apply only updates with `rev > last_applied`; out-of-order delivery is impossible to render. Mixing `FullState` and `ViewBatch` is safe: a `FullState` at rev=N supersedes any pending `ViewBatch` with rev<N.

**The planner batches at ≤60Hz.** 500 reactions arriving in 100ms become ≤6 batched deltas, not 500 callbacks. Bible commandment 9 (no high-frequency FFI loops) is honored by construction.

**No platform polling for app data.** Lower-frequency summaries, progress, and view data flow through the same callback-driven `AppUpdate` stream. Ultra-high-frequency native surfaces must stay native-owned and report bounded summaries back to Rust; they do not introduce platform polling for framework state.

### A2. Alternatives considered

Three serious alternatives to snapshots+ViewBatch were evaluated. Each is used in production by other apps. Each was rejected (or deferred) for specific reasons.

| Alternative | Used by | Why rejected for v1 |
|---|---|---|
| **Reactive shared SQLite.** Rust writes; both sides hold read handles; reactive query libraries (GRDB / SQLDelight / Drift) re-run queries on table writes. | 1Password (Op core), Linear, Notion mobile, most local-first apps | Surrenders doctrine. Platforms now write queries, which is display-shaping logic. Pre-formatting (timestamps, npubs, sats) either moves into native (D-violation) or materializes as columns at write time (extra schema). Web fragments — wasm SQLite doesn't share with JS the way native does. Cross-platform consistency tests get harder (per-platform query results vs byte-diffable JSON). |
| **Local relay / localhost IPC.** Rust runs an in-process Nostr relay (e.g. `LocalRelay` from `nostr-relay-builder`); platform talks Nostr over WebSocket to it. | Some Tauri apps; Citrine-style Android setups conceptually | WebSocket+Nostr-JSON tax for in-process IPC. Outbox routing semantics get weird (the "relay" is local but represents many remote relays). The framework's value-add (views, actions, sessions as state) gets obscured behind a protocol that wasn't designed for it. |
| **Shared memory + signal.** Rust writes to mmap'd or shared heap; platform reads via raw pointers; FFI carries only "key X changed." | Game engines; Flutter+Skia for graphics state | Memory safety across FFI is hellish. Unsuitable for Swift/Kotlin idioms. Not portable to web. |

**The "hybrid for v2" possibility.** If Phase 9 measurement shows marshaling cost as the bottleneck on bulk-scrolling views (timeline, conversation history, search), the deliberate v2 escape is:

1. Framework owns a SQLite schema.
2. Framework scaffolds typed, parameterized reactive query bindings per platform via the CLI; platforms call `viewModel.timeline(authors).asFlow()` rather than writing SQL.
3. Schema, indexes, formatting (materialized display columns), and invalidation are owned entirely by Rust.
4. Snapshots + ViewBatch retained for small screen-shaped state and small-payload views; reactive queries used for bulk-scrolling views.
5. Web continues with message-passing (wasm SQLite doesn't bridge to JS reactively).

This is a v2 decision gated on Phase 9 data. v1 ships with snapshots+ViewBatch only.

### A3. Why `ViewBatch` from day one (vs. snapshot-only MVP)

The bible's stated default is "start with `FullState` everywhere; add granular updates only when profiling demands." We deviate because:

- Nostr timeline shape is fundamentally chatty: a single popular event arriving triggers reaction, repost, and zap-receipt events at hundreds per second.
- Full-state churn under that load is wasteful per individual update and harmful in aggregate.
- The marginal complexity of `ViewBatch` is small: it's a typed delta enum over the already-existing view payload types.
- Retrofitting `ViewBatch` later would invalidate every platform shim and reconciler implementation.

Both update variants (`FullState` and `ViewBatch`) ship in v1 over FlatBuffers. `FullState` remains the recovery path for coarse changes and platform-side state drift. It is not a JSON fallback.

---

## Appendix B. Glossary of NIPs referenced

| NIP | Purpose | Where it appears |
|---|---|---|
| 01 | Base protocol, replaceable events | §7.1 |
| 05 | DNS-based identifiers | §7.6 |
| 07 | Browser signer | §7.4 |
| 09 | Deletion events | §7.1 |
| 17 | Private DMs | §7.10 |
| 19 | bech32 entities | §7.12 |
| 23 | Long-form content | §4.5 (proof app) |
| 25 | Reactions | §6.3, §7.6 |
| 40 | Expiration | §7.1 |
| 42 | Auth | §6.4 |
| 44 | Encryption | §7.10 |
| 46 | Nostr Connect / bunker | §7.4 |
| 47 | Wallet Connect | §7.9 |
| 49 | Encrypted private key | §7.4 |
| 55 | Android external signer | §7.4 |
| 57 | Lightning zaps | §7.9 |
| 59 | Gift wrap | §7.10 |
| 60 | Cashu wallets | §7.9 |
| 61 | Nutzaps | §7.9 |
| 65 | Relay-list metadata (outbox) | §7.3 |
| 77 | Negentropy | §7.8 |
