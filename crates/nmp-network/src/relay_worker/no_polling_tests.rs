#[test]
fn relay_worker_uses_readiness_not_fixed_read_timeouts() {
    let worker = include_str!("mod.rs");
    let ready = include_str!("io_ready.rs");
    let socket = include_str!("socket_io.rs");
    let production = format!("{worker}\n{ready}\n{socket}");

    for forbidden in [
        "RELAY_READ_TIMEOUT",
        "set_read_timeout",
        "Duration::from_millis(50)",
    ] {
        assert!(
            !production.contains(forbidden),
            "relay worker regressed to polling pattern: {forbidden}"
        );
    }

    assert!(
        !worker.contains(".try_recv()") && !socket.contains(".try_recv()"),
        "socket/control drains may use try_recv only inside the readiness helper"
    );

    assert!(
        ready.contains("Poll::new()") && ready.contains("Waker::new"),
        "relay worker should block on socket readiness and control-channel wakeups"
    );
}
