   Permanent`) is the engine's; retry-or-give-up is the engine's.
@@ -72,7 +73,7 @@
     kind:10002 set receive the EVENT.
   - `publish_p_tag_inbox_routing` — `#p:bob` adds bob's read relays.
   - `publish_retry_on_connection_drop` — transient → retry → OK.
-  - `publish_giveup_after_three_retries` — three transient → FailedAfterRetries.
+  - `publish_giveup_after_three_attempts` — three transient attempts → FailedAfterRetries.
   - `publish_durable_across_restart` — engine instance 1 queues + dies;
     instance 2 resumes from the same `PublishStore` and completes.
   - `publish_dedup_on_same_event_multi_relay_single_rev_per_batch` — 5 relay

tokens used
174,211
Reviewed 947dcfc.

FIX: stale retry test/doc text was already committed on master as `434bb3f`.

FIX attempted: engine D6 module comment still stale on `origin/master`; patched locally in `crates/nmp-core/src/publish/engine.rs`, but commit/push blocked:
- local `.git/index.lock`: `Operation not permitted`
- direct push attempt: `Could not resolve hostname github.com`

REPORT:
- D7/API: `RelayAck::Failed { class: AckClass }` still makes dispatcher classify policy.
- D6/FFI: `PublishEngineError` paths still need actor/FFI mapping before exposure.
- Correctness/new tests: `pending_retries` still not persisted; restart backoff is not durable.
- File-size: no hard cap hit; publish tests remain soft-cap only.

Verification: stale-string/diff checks clean for local patch; full cargo currently blocked by missing `apps/podcast/podcast-core/Cargo.toml`.


