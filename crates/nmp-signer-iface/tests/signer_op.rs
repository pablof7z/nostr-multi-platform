//! Tests for `SignerOp<T>` — the synchronous-by-default thunk wrapping every
//! signer operation that may complete asynchronously.
//!
//! `SignerOp` is the load-bearing seam between the (synchronous, `std::sync::
//! mpsc`-driven) kernel actor and the various signer backends; pending a real
//! `gift_wrap_with_signer` consumer landing, these unit tests pin the synchronous
//! `Ready` / `Pending` / `wait` / `poll` contract from inside the crate so the
//! upcoming signer-seam refactor cannot silently regress it.

use std::sync::mpsc;
use std::thread;
use std::time::Duration;

use nmp_signer_iface::{SignerError, SignerOp};

// ── Ready path ──────────────────────────────────────────────────────────

#[test]
fn ok_constructor_yields_ready_ok() {
    let op: SignerOp<u32> = SignerOp::ok(7);
    // Cheapest probe: `wait` with an arbitrary timeout returns immediately on
    // a Ready variant. Timeout is irrelevant for Ready — proves synchronous
    // resolution.
    let value = op.wait(Duration::from_millis(1)).expect("Ready(Ok) must resolve");
    assert_eq!(value, 7, "Ready value must be the input");
}

#[test]
fn err_constructor_yields_ready_err() {
    let op: SignerOp<u32> = SignerOp::err(SignerError::Rejected("user said no".into()));
    let res = op.wait(Duration::from_millis(1));
    assert!(matches!(res, Err(SignerError::Rejected(_))));
}

#[test]
fn ready_poll_returns_some_immediately() {
    let mut op: SignerOp<u32> = SignerOp::ok(42);
    let polled = op.poll();
    assert!(matches!(polled, Some(Ok(42))), "Ready poll must surface the value");
}

#[test]
fn ready_poll_is_destructive_second_poll_surfaces_backend_error() {
    // Polling `Ready` consumes the value (it's `take`n out by `mem::replace`).
    // The crate documents this by replacing the inner with a `Backend` error;
    // subsequent polls must return that error rather than panic, looping on
    // stale data, or returning `None` (which would be a livelock invitation
    // for any caller spinning until completion).
    let mut op: SignerOp<u32> = SignerOp::ok(99);
    let first = op.poll();
    assert!(matches!(first, Some(Ok(99))));

    let second = op.poll();
    assert!(
        matches!(second, Some(Err(SignerError::Backend(_)))),
        "second poll on consumed Ready must yield Backend error, got {second:?}"
    );
}

// ── Pending path: mpsc resolution ───────────────────────────────────────

#[test]
fn pending_resolves_to_ok_when_sender_sends_value() {
    // The Pending variant carries a real `mpsc::Receiver`. Drive it from a
    // worker thread to prove the resolution contract works under genuine
    // cross-thread completion — that's the production NIP-46 shape.
    let (tx, rx) = mpsc::channel::<Result<String, SignerError>>();
    let op: SignerOp<String> = SignerOp::Pending(rx);

    thread::spawn(move || {
        tx.send(Ok("signed".into()))
            .expect("worker must succeed");
    });

    // Block at most 1s — generous slack for a single mpsc send on any CI.
    let resolved = op
        .wait(Duration::from_secs(1))
        .expect("Pending must resolve to Ok");
    assert_eq!(resolved, "signed", "Pending must surface the sent value");
}

#[test]
fn pending_resolves_to_err_when_sender_sends_err() {
    let (tx, rx) = mpsc::channel::<Result<u32, SignerError>>();
    let op: SignerOp<u32> = SignerOp::Pending(rx);

    thread::spawn(move || {
        let _ = tx.send(Err(SignerError::Mismatch("pubkey drift".into())));
    });

    let resolved = op.wait(Duration::from_secs(1));
    assert!(
        matches!(resolved, Err(SignerError::Mismatch(_))),
        "Pending must surface the error variant the sender produced, got {resolved:?}"
    );
}

#[test]
fn pending_wait_times_out_when_sender_silent() {
    // No worker → no send → `wait` must hit `RecvTimeoutError::Timeout` and
    // map it to `SignerError::Timeout`. The test must NOT hang.
    let (_tx, rx) = mpsc::channel::<Result<(), SignerError>>();
    // Keep `_tx` alive (otherwise we'd hit the `Disconnected` branch instead).
    let op: SignerOp<()> = SignerOp::Pending(rx);

    let start = std::time::Instant::now();
    let res = op.wait(Duration::from_millis(50));
    let elapsed = start.elapsed();

    assert!(
        matches!(res, Err(SignerError::Timeout(_))),
        "silent Pending must time out, got {res:?}"
    );
    // Defensive: confirm we actually waited, not returned synchronously.
    assert!(
        elapsed >= Duration::from_millis(40),
        "wait must respect the timeout floor, elapsed={elapsed:?}"
    );
}

#[test]
fn pending_wait_surfaces_backend_when_sender_dropped() {
    // Dropping the sender without sending must surface `SignerError::Backend`
    // — not `Timeout`, not a panic, not a deadlock.
    let (tx, rx) = mpsc::channel::<Result<u8, SignerError>>();
    drop(tx);
    let op: SignerOp<u8> = SignerOp::Pending(rx);

    let res = op.wait(Duration::from_millis(100));
    assert!(
        matches!(res, Err(SignerError::Backend(_))),
        "dropped sender must surface Backend, got {res:?}"
    );
}

#[test]
fn pending_poll_returns_none_until_send() {
    // The non-blocking probe must return `None` while the channel is empty.
    // This is the property the kernel actor relies on to integrate signer
    // ops into its existing `try_recv` select loop without polling-sleeping.
    let (tx, rx) = mpsc::channel::<Result<u8, SignerError>>();
    let mut op: SignerOp<u8> = SignerOp::Pending(rx);

    // First poll: nothing sent yet → None.
    assert!(op.poll().is_none(), "empty channel must return None");

    // Send a value, then poll again.
    tx.send(Ok(11)).unwrap();
    // mpsc::send is fenced — the value is observable on the receiver as soon
    // as the send returns, so the next poll must yield Some(Ok(11)).
    let polled = op.poll();
    assert!(matches!(polled, Some(Ok(11))), "poll after send must surface value, got {polled:?}");
}

#[test]
fn pending_poll_surfaces_backend_when_sender_dropped_empty() {
    let (tx, rx) = mpsc::channel::<Result<u8, SignerError>>();
    drop(tx);
    let mut op: SignerOp<u8> = SignerOp::Pending(rx);

    let res = op.poll();
    assert!(
        matches!(res, Some(Err(SignerError::Backend(_)))),
        "empty + disconnected channel must surface Backend on poll, got {res:?}"
    );
}

// ── Debug impl ──────────────────────────────────────────────────────────

#[test]
fn debug_impl_does_not_leak_inner_value() {
    // `SignerOp` is held in actor-side logs and history surfaces; the Debug
    // impl must format without leaking the wrapped value (which may carry
    // signatures or plaintext). We assert the shape, not the value.
    let ok = SignerOp::<String>::ok("super-secret-signature".into());
    let s = format!("{ok:?}");
    assert!(s.contains("Ready"));
    assert!(
        !s.contains("super-secret-signature"),
        "Debug must NOT leak the wrapped value, got `{s}`"
    );

    let err: SignerOp<()> = SignerOp::err(SignerError::Rejected("nope".into()));
    let s = format!("{err:?}");
    assert!(s.contains("Ready"));
    // Errors are user-visible diagnostics by design — they're fine to print.

    let (_tx, rx) = mpsc::channel::<Result<u8, SignerError>>();
    let pending: SignerOp<u8> = SignerOp::Pending(rx);
    let s = format!("{pending:?}");
    assert!(s.contains("Pending"));
}
