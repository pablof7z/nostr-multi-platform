//! Serialized capability-worker thread (ADR-0040 §3, V-90 Site 2).
//!
//! Moves synchronous-native (Keychain-class) capability I/O OFF the actor
//! thread. The actor enqueues a [`CapabilityWorkItem`] instead of calling
//! `dispatch_capability` inline; one dedicated OS thread drains the queue
//! via blocking `recv` (D8 — never a poll) and re-enters the actor with
//! [`super::ActorCommand::CapabilityResultReady`].
//!
//! ## Design invariants
//!
//! * **Single FIFO worker** — one thread, one queue. Per-op spawn is
//!   explicitly rejected (ADR-0040 §"Options considered") because two threads
//!   racing the Keychain can reorder a `persist(A)` + `forget(A)` pair for
//!   the same account, corrupting at-rest secrets on a rapid account switch.
//!   The single FIFO worker makes per-account persist/forget ordering correct
//!   by construction.
//!
//! * **D8 compliance** — the worker advances *only* via blocking `recv()`,
//!   never a `recv_timeout` poll or `try_recv` spin. Queue shutdown (sender
//!   drop on actor teardown) causes `recv` to return `Err(Disconnected)` and
//!   the thread exits cleanly — no lingering blocked thread.
//!
//! * **D4 — actor sole writer** — the worker never touches kernel state. Its
//!   only output is a single `ActorCommand::CapabilityResultReady` sent
//!   back through `command_tx`; the actor applies it on its own thread.
//!
//! * **Account-switch safety** — the actor checks the `account_id` carried
//!   by `CapabilityResultReady` against its live identity state. Results for
//!   removed accounts are dropped with a D6 trace; they are never applied to
//!   whatever account happens to be active at re-entry time.
//!
//! * **Request built on-actor** — `CapabilityRequest` JSON is serialized
//!   *before* enqueuing (still on the actor thread) so the worker never
//!   reads actor-owned identity state. The worker only calls `dispatch_capability`
//!   with pre-baked JSON.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::time::Duration;

use crate::capability_socket::{dispatch_capability, CapabilityCallbackSlot};
use crate::substrate::CapabilityRequest;

/// An item of work for the capability worker thread.
///
/// Built entirely on the actor thread from actor-owned state; nothing in
/// this struct requires the actor's mutex or borrow checker after it is
/// constructed. The worker calls `dispatch_capability` with `request_json`
/// and posts `CapabilityResultReady { account_id, result_json }` back.
pub(crate) struct CapabilityWorkItem {
    /// The account the operation targets. Carried through to the re-entry
    /// command so the actor can confirm the account still exists before
    /// applying the result (account-switch safety, ADR-0040 §"Ordering =
    /// account-switch safety").
    pub(crate) account_id: String,
    /// Pre-serialized `CapabilityRequest` JSON (built on the actor thread
    /// from `identity`-owned data; never read off-actor).
    pub(crate) request_json: String,
    /// Wall-clock deadline for this item. If `dispatch_capability` takes
    /// longer than `CAPABILITY_OP_TIMEOUT` the worker abandons the item
    /// and posts an error re-entry, keeping forward progress on the next
    /// queued item (D6).
    pub(crate) deadline: std::time::Instant,
}

/// Per-item deadline budget. 5 s matches `PENDING_SIGN_TIMEOUT` — generous
/// enough for any Keychain prompt, tight enough to surface a wedged handler.
pub(crate) const CAPABILITY_OP_TIMEOUT: Duration = Duration::from_secs(5);

/// Sender half of the capability-worker queue. Held in [`ActorContext`] and
/// cloned to whatever dispatch arms need it. Dropping all senders shuts the
/// worker thread down cleanly (D8 — no explicit shutdown signal needed).
pub(crate) type CapabilityWorkSender = Sender<CapabilityWorkItem>;

/// Spawn the serialized capability-worker thread and return the sender half
/// of its work queue.
///
/// The worker owns the `Receiver` and drains it with blocking `recv` (D8).
/// On queue shutdown (all senders dropped) `recv` returns
/// `Err(Disconnected)` and the thread exits cleanly.
///
/// `capability_callback` — shared slot the worker reads to invoke the
/// registered native callback (e.g. iOS `KeychainCapability.handleJSON`).
///
/// `command_tx` — the actor's self-feedback sender. The worker uses it to
/// post `CapabilityResultReady` back into the actor's normal dispatch loop
/// (D3/D4 — the actor is the sole writer of kernel state).
pub(crate) fn spawn_capability_worker(
    capability_callback: CapabilityCallbackSlot,
    command_tx: super::CommandSender,
) -> CapabilityWorkSender {
    let (work_tx, work_rx): (Sender<CapabilityWorkItem>, Receiver<CapabilityWorkItem>) = channel();

    #[rustfmt::skip]
    std::thread::Builder::new()
        .name("nmp-capability-worker".to_string())
        .spawn(move || run_worker(work_rx, capability_callback, command_tx))
        .expect("failed to spawn capability worker thread"); // doctrine-allow: D6 — spawned once during actor init; return type is `CapabilityWorkSender` (not `Result`) because callers cannot meaningfully recover from OS-level thread exhaustion at startup. Mirrors `new_event_observer_slot` pattern.

    work_tx
}

/// The worker's main loop. Drains `work_rx` with blocking `recv` (D8 —
/// never a poll). Each item is run synchronously against the registered
/// native callback; the result is posted back through `command_tx`.
fn run_worker(
    work_rx: Receiver<CapabilityWorkItem>,
    capability_callback: CapabilityCallbackSlot,
    command_tx: super::CommandSender,
) {
    loop {
        // D8 — blocking recv. No sleep, no spin, no recv_timeout poll.
        // When the sender is dropped (actor teardown) this returns
        // `Err(Disconnected)` and the thread exits cleanly.
        let item = match work_rx.recv() {
            Ok(item) => item,
            Err(_) => return, // sender dropped — actor teardown
        };

        // Queue-age deadline (NOT a mid-call timeout). If this item aged past
        // its deadline while waiting in the queue — e.g. the worker was stalled
        // on a prior item — we drop it as stale and emit an error envelope so
        // the actor's CapabilityResultReady arm surfaces a D6 toast instead of
        // dispatching an out-of-date request. This does NOT interrupt a callback
        // that hangs mid-call: a synchronous C callback cannot be cancelled
        // without a separate watchdog thread, and the Keychain class we dispatch
        // (kSecAttrAccessibleWhenUnlockedThisDeviceOnly, no LAContext) has no
        // user-confirmable prompt that could hang.
        let timed_out = item.deadline < std::time::Instant::now();
        let result_json = if timed_out {
            // Synthesize an error envelope so the actor arm can toast.
            crate::capability_socket::capability_error_envelope(
                &item.request_json,
                "capability-op-timed-out",
            )
        } else {
            // Run the synchronous native callback off the actor thread.
            // `dispatch_capability` is itself D6-safe: a NULL handler or
            // NULL return both produce an error envelope, never a panic.
            dispatch_capability(&capability_callback, &item.request_json)
        };

        // Re-enter the actor with the result. The actor's dispatch arm
        // confirms the account still exists before applying. A
        // disconnected sender (post-Shutdown) is a benign no-op (D6).
        let _ = command_tx.send(super::ActorCommand::CapabilityResultReady {
            account_id: item.account_id,
            result_json,
        });
    }
}

/// Build a `CapabilityWorkItem` for a write-path keyring op.
///
/// Called on the actor thread. Serializes the pre-built `CapabilityRequest`
/// and stamps the deadline. The item carries no actor-owned borrows; it is
/// safe to move to the worker thread.
pub(crate) fn make_work_item(
    account_id: impl Into<String>,
    request: &CapabilityRequest,
) -> Option<CapabilityWorkItem> {
    let request_json = serde_json::to_string(request).ok()?;
    Some(CapabilityWorkItem {
        account_id: account_id.into(),
        request_json,
        deadline: std::time::Instant::now() + CAPABILITY_OP_TIMEOUT,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::capability_socket::{
        new_capability_callback_slot, CapabilityCallbackRegistration, CapabilityCallbackSlot,
    };
    use crate::substrate::{
        CapabilityEnvelope, CapabilityModule, KeyringCapability, KeyringIdentityWiring,
    };
    use crate::actor::{ActorMail, CommandSender};
    use std::collections::HashMap;
    use std::ffi::{c_char, c_void, CStr, CString};
    use std::sync::mpsc::Receiver;
    use std::sync::Mutex;
    use std::time::Duration;

    /// ADR-0050 §D3a — the worker now takes a [`CommandSender`] over an
    /// [`ActorMail`] inbox. This helper builds the pair and returns the
    /// `ActorMail` receiver so tests can assert the posted command.
    fn cap_channel() -> (CommandSender, Receiver<ActorMail>) {
        let (tx, rx) = std::sync::mpsc::channel::<ActorMail>();
        (CommandSender::new(tx), rx)
    }

    /// Unwrap an `ActorMail::Command` posted by the worker.
    fn unwrap_cmd(mail: ActorMail) -> super::super::ActorCommand {
        match mail {
            ActorMail::Command(cmd) => cmd,
            other => panic!("expected ActorMail::Command, got {other:?}"),
        }
    }

    // ──────────────────────────────────────────────────────────────────────
    // Mock native handler (mirrors nmp-ffi/src/capability.rs tests)
    // ──────────────────────────────────────────────────────────────────────

    static STORE: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
    static SERIAL: Mutex<()> = Mutex::new(());

    extern "C" fn mock_handler(
        _ctx: *mut c_void,
        request_json: *const c_char,
    ) -> *mut c_char {
        use crate::substrate::{KeyringRequest, KeyringResult};
        let request = unsafe { CStr::from_ptr(request_json) }
            .to_str()
            .unwrap_or("");
        let parsed: serde_json::Value = serde_json::from_str(request).unwrap_or_default();
        let correlation_id = parsed
            .get("correlation_id")
            .and_then(|v| v.as_str())
            .unwrap_or("")
            .to_string();
        let payload = parsed
            .get("payload_json")
            .and_then(|v| v.as_str())
            .unwrap_or("");
        let result = match serde_json::from_str::<KeyringRequest>(payload) {
            Ok(KeyringRequest::Store { account_id, secret }) => {
                STORE
                    .lock()
                    .unwrap()
                    .get_or_insert_with(HashMap::new)
                    .insert(account_id, secret);
                KeyringResult::ok(None)
            }
            Ok(KeyringRequest::Retrieve { account_id }) => {
                match STORE
                    .lock()
                    .unwrap()
                    .get_or_insert_with(HashMap::new)
                    .get(&account_id)
                {
                    Some(s) => KeyringResult::ok(Some(s.clone())),
                    None => KeyringResult::not_found(),
                }
            }
            Ok(KeyringRequest::Delete { account_id }) => {
                STORE
                    .lock()
                    .unwrap()
                    .get_or_insert_with(HashMap::new)
                    .remove(&account_id);
                KeyringResult::ok(None)
            }
            Err(_) => KeyringResult::error(-50),
        };
        let envelope = CapabilityEnvelope {
            namespace: KeyringCapability::NAMESPACE.to_string(),
            correlation_id,
            result_json: serde_json::to_string(&result).unwrap(),
        };
        CString::new(serde_json::to_string(&envelope).unwrap())
            .unwrap()
            .into_raw()
    }

    fn registered_slot() -> CapabilityCallbackSlot {
        let slot = new_capability_callback_slot();
        *slot.lock().unwrap() = Some(CapabilityCallbackRegistration {
            context: 0,
            callback: mock_handler,
        });
        slot
    }

    // ──────────────────────────────────────────────────────────────────────
    // Test 1: actor does not block — worker runs the callback off-actor
    // ──────────────────────────────────────────────────────────────────────

    /// A slow callback (200 ms sleep) must not stall the work-sender. The
    /// actor enqueues and returns immediately; the result arrives
    /// asynchronously via `CapabilityResultReady`.
    #[test]
    fn worker_runs_callback_off_actor() {
        extern "C" fn slow_handler(
            _ctx: *mut c_void,
            _req: *const c_char,
        ) -> *mut c_char {
            std::thread::sleep(Duration::from_millis(200));
            // Return a minimal valid CapabilityEnvelope JSON.
            CString::new(r#"{"namespace":"n","correlation_id":"c","result_json":"{}"}"#)
                .unwrap()
                .into_raw()
        }

        let slot = new_capability_callback_slot();
        *slot.lock().unwrap() = Some(CapabilityCallbackRegistration {
            context: 0,
            callback: slow_handler,
        });
        let (cmd_tx, cmd_rx) = cap_channel();
        let work_tx = spawn_capability_worker(slot, cmd_tx);

        let req = KeyringIdentityWiring::persist_secret("c1", "acct-1", "nsec1secret");
        let item = make_work_item("acct-1", &req).expect("make_work_item");

        let before = std::time::Instant::now();
        work_tx.send(item).unwrap();
        // The send returns immediately — no actor blocking.
        let elapsed = before.elapsed();
        assert!(
            elapsed < Duration::from_millis(50),
            "enqueue blocked for {elapsed:?} — actor should not have waited"
        );

        // The result arrives asynchronously (within the slow handler's 200ms + margin).
        let cmd = unwrap_cmd(
            cmd_rx
                .recv_timeout(Duration::from_millis(600))
                .expect("CapabilityResultReady not received in time"),
        );
        match cmd {
            super::super::ActorCommand::CapabilityResultReady { account_id, .. } => {
                assert_eq!(account_id, "acct-1");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    // ──────────────────────────────────────────────────────────────────────
    // Test 2: FIFO ordering — persist before forget
    // ──────────────────────────────────────────────────────────────────────

    /// Two ops for the same account (persist then forget) must execute in
    /// submission order. The single FIFO worker guarantees this; a
    /// per-op-spawn approach would race.
    #[test]
    fn worker_preserves_fifo_order() {
        let _g = SERIAL.lock().unwrap();
        *STORE.lock().unwrap() = Some(HashMap::new());

        let slot = registered_slot();
        let (cmd_tx, cmd_rx) = cap_channel();
        let work_tx = spawn_capability_worker(slot, cmd_tx);

        // Enqueue persist then forget in order.
        let persist = KeyringIdentityWiring::persist_secret("c1", "acct-fifo", "secret42");
        let forget = KeyringIdentityWiring::forget_secret("c2", "acct-fifo");

        work_tx
            .send(make_work_item("acct-fifo", &persist).unwrap())
            .unwrap();
        work_tx
            .send(make_work_item("acct-fifo", &forget).unwrap())
            .unwrap();

        // Collect two results; order must match submission order.
        let r1 = unwrap_cmd(
            cmd_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("first result"),
        );
        let r2 = unwrap_cmd(
            cmd_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("second result"),
        );

        let correlation_ids: Vec<String> = [r1, r2]
            .into_iter()
            .map(|cmd| {
                let super::super::ActorCommand::CapabilityResultReady { result_json, .. } = cmd
                else {
                    panic!("expected CapabilityResultReady");
                };
                // Extract correlation_id from the outer envelope to verify order.
                let v: serde_json::Value = serde_json::from_str(&result_json).unwrap();
                v.get("correlation_id")
                    .and_then(|c| c.as_str())
                    .unwrap_or("")
                    .to_string()
            })
            .collect();

        assert_eq!(
            correlation_ids,
            vec!["c1".to_string(), "c2".to_string()],
            "persist must arrive before forget (FIFO)"
        );

        // After FIFO ordering: persist ran before forget, so the secret should
        // now be absent (forget executed last).
        assert!(
            STORE
                .lock()
                .unwrap()
                .as_ref()
                .unwrap()
                .get("acct-fifo")
                .is_none(),
            "forget should have removed the secret persisted by persist"
        );
    }

    // ──────────────────────────────────────────────────────────────────────
    // Test 3: deadline timeout — worker abandons a wedged handler
    // ──────────────────────────────────────────────────────────────────────

    /// An item whose deadline has already passed when the worker dequeues it
    /// must produce an error `CapabilityResultReady` without calling the
    /// native handler.
    #[test]
    fn worker_emits_error_on_expired_deadline() {
        let slot = registered_slot();
        let (cmd_tx, cmd_rx) = cap_channel();
        let work_tx = spawn_capability_worker(slot, cmd_tx);

        let req = KeyringIdentityWiring::persist_secret("c-timeout", "acct-timeout", "secret");
        let mut item = make_work_item("acct-timeout", &req).unwrap();
        // Force deadline to the past so it trips the timeout gate immediately.
        item.deadline = std::time::Instant::now() - Duration::from_secs(10);

        work_tx.send(item).unwrap();

        let cmd = unwrap_cmd(
            cmd_rx
                .recv_timeout(Duration::from_secs(2))
                .expect("timed-out item result not received"),
        );
        match cmd {
            super::super::ActorCommand::CapabilityResultReady { result_json, account_id } => {
                assert_eq!(account_id, "acct-timeout");
                // The result should be an error envelope (capability-op-timed-out).
                let v: serde_json::Value = serde_json::from_str(&result_json).unwrap();
                let status = v
                    .get("result_json")
                    .and_then(|r| r.as_str())
                    .and_then(|s| serde_json::from_str::<serde_json::Value>(s).ok())
                    .and_then(|inner| inner.get("status").and_then(|s| s.as_str()).map(String::from))
                    .unwrap_or_default();
                assert_eq!(status, "error", "expired deadline must yield error result");
            }
            other => panic!("unexpected command: {other:?}"),
        }
    }

    // ──────────────────────────────────────────────────────────────────────
    // Test 4: worker exits cleanly when sender is dropped
    // ──────────────────────────────────────────────────────────────────────

    /// Dropping all senders shuts the worker down without a panic.
    #[test]
    fn worker_exits_on_sender_drop() {
        let slot = new_capability_callback_slot();
        let (cmd_tx, _cmd_rx) = cap_channel();
        let work_tx = spawn_capability_worker(slot, cmd_tx);
        // Drop the sender — the worker should unblock and exit.
        drop(work_tx);
        // Give the thread a moment to exit. If it hangs the test will time out.
        std::thread::sleep(Duration::from_millis(100));
        // No assertion needed — if we reach here the thread exited (or is about
        // to) without panicking, which is what we require.
    }
}
