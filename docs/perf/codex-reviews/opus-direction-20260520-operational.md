# Opus Direction Review — Operational Gaps (2026-05-20)

Third review of the day. Reviews 1 and 2 covered strategy/structure (relay
transport risk; per-app projection + bespoke FFI; missing actions layer). This
review ignores all of that and asks a narrower question: **even with a perfect
architecture, what would make NMP fail in production?** Four operational axes,
with code evidence.

## 1. Error observability — surfaced, but stringly-typed at the FFI boundary

The good news: errors *do* reach a surface. `RelayEvent::Failed` carries an
error string (`relay_worker/mod.rs:31`); the kernel stamps it onto
`RelayHealth.last_error` (`requests/relay_lifecycle.rs:50`) and CLOSED/NOTICE
frames into `last_error`/`last_notice` (`ingest/closed.rs:135-173`,
`ingest/mod.rs:74,205`); publish failures collapse to a `last_error_toast`
(`kernel/mod.rs:329`) projected into every snapshot (`kernel/update.rs:125`).
A bounded 80-line log ring also rides the snapshot (`kernel/status.rs:378-388`).
`last_error` correctly *clears* on successful reconnect (`relay_lifecycle.rs:22`),
so the field is not stale after recovery.

The gap: **every error is a `String` by the time it crosses into Swift.** Rich
internal enums exist — `PublishEngineError` (`publish/engine.rs:44`),
`StoreError` (`store/types/errors.rs:38`), `SignerError`, `PlannerError` — but
all are flattened (`describe_engine_error`, `publish_engine_wire.rs:43`) before
the snapshot. iOS therefore cannot programmatically branch on error *class*: it
cannot distinguish "relay denied you (403, permanent)" from "relay rate-limited
you (retry later)" from "your event was malformed" without string-matching.
There is no error *code*, no severity, no machine-readable category. For an SDK
whose whole premise is reuse across apps, the absence of a typed error contract
at the FFI boundary is a real operational gap — every consuming app re-invents
fragile string heuristics.

## 2. Relay lifecycle correctness — the strongest area

`relay_worker/mod.rs` is genuinely solid. Mid-session drops trigger
exponential backoff (3s→300s, `RELAY_RECONNECT_DELAY_INITIAL/MAX`) with
per-URL deterministic jitter to avoid thundering-herd (`jittered_backoff`,
:109). 401/403 are classified permanent and stop retrying (`is_permanent_error`,
:119). Keepalive ping/pong detects half-open sockets (:304-326). Pending
writes survive a reconnect in the worker's `VecDeque` (:387-399,
`wait_before_reconnect` keeps draining control_rx). Subscriptions re-establish:
`connected_urls` is the reconnect-replay discriminator (`actor/mod.rs:493`) and
test coverage is real — `nip77_reconnect_resumes_from_watermark.rs`,
`m8_subscription_lifecycle.rs`, `relay_worker/tests.rs`. This axis is
production-grade; nothing to flag.

## 3. Local state persistence — half-finished durable resume (HEADLINE)

This is the sharpest finding. `EventStore` has a real persistence story: both
`MemEventStore` and `LmdbEventStore` exist, selected by an FFI-supplied
`storage_path` or `NMP_LMDB_PATH` (`kernel/mod.rs:429-447`). Cached events and
profiles survive restart under `--features lmdb-backend`.

**`PublishStore` does not.** The only impl is `InMemoryPublishStore`
(`publish/traits.rs:251`), and `Kernel::with_storage_path` hard-wires it
unconditionally — `Arc::new(crate::publish::InMemoryPublishStore::new())` at
`kernel/mod.rs:467`, with no feature gate and no LMDB alternative anywhere in
the repo (`grep "impl PublishStore for"` → one hit).

The cruel part: the *resume machinery is fully built*.
`PublishEngine::resume_from_store` (`publish/engine.rs:135`) is wired into the
`Start` path (`kernel/publish_engine.rs:316-334`, called once per `Start`
command). But it calls `store.load_pending()` on a store that is empty *by
definition* after a process restart. So: **offline-composed publish intents are
silently lost on app kill/relaunch.** A user who drafts a note offline, the
process gets reaped by iOS, and the note is gone — with no error, because
`load_pending()` honestly returns `Ok(vec![])`. The contract is invisible: a
developer reading `resume_from_store` would reasonably assume durability that
does not exist. Either implement `LmdbPublishStore` or document loudly that
publish intents are session-scoped.

## 4. Stuck-state recovery — one silent-death vector

The actor loop (`actor/mod.rs:505`) has no unbounded blocking: relay reads use
a 50ms timeout, `recv_timeout(compute_wait(...))` bounds the event lane,
parked remote-sign ops carry a `deadline` (`pending_sign.rs:43,52`). Relay
event handling is wrapped in `catch_unwind` (:605) so a panic on malicious
network bytes cannot kill the kernel — it logs, toasts, and continues.

The deliberate asymmetry: the **command drain is NOT wrapped** (:602-604 —
"commands are internally generated, so a panic there is a genuine bug that must
stay visible"). Defensible doctrine, but worth naming as the one stuck-state
vector: a panic inside `dispatch_command` kills the actor thread, and from
Swift's side this manifests only as the update channel going permanently
silent — no error, no toast, no crash report from the FFI layer. There is no
liveness watchdog on the actor thread. If a command-path bug ever ships, the
operator sees a frozen UI with zero diagnostic signal. A heartbeat/last-tick
field in the snapshot would convert this silent death into an observable one.
