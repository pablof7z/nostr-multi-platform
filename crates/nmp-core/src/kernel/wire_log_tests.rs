/// Tests for the `wire_log` module.
///
/// # Gate-decision coverage
///
/// `claim_log_enabled()` caches the result in a `OnceLock<bool>` on first call,
/// making env-var-based gate tests order-dependent and unreliable across test
/// threads. We sidestep this by folding the enabled flag into `write_wire_line`
/// itself: `log_wire` calls `write_wire_line(&mut stderr, claim_log_enabled(), &ev)`,
/// and tests pass `true`/`false` explicitly. This means the production gate
/// decision (the one-liner in `log_wire`) is code-reviewable, while the I/O
/// behaviour of both paths is fully exercised here.
#[cfg(test)]
use super::wire_log::{write_wire_line, WireLogEvent};

#[test]
fn gate_false_produces_no_output() {
    // When the gate flag is false, write_wire_line must write nothing.
    // This is the production disabled-path behaviour exercised directly.
    let event = WireLogEvent::ReqEmit {
        sub_id: "sub-1",
        relay_url: "wss://relay.example.com",
        phase: "phase1",
        author: "aabbcc",
        has_hint: false,
    };
    let mut buf: Vec<u8> = Vec::new();
    write_wire_line(&mut buf, false, &event);
    assert!(buf.is_empty(), "disabled gate must produce no output");
}

#[test]
fn gate_true_produces_output() {
    // When the gate flag is true, write_wire_line must emit a non-empty line.
    // This is the production enabled-path behaviour exercised directly.
    let event = WireLogEvent::ReqEmit {
        sub_id: "sub-1",
        relay_url: "wss://relay.example.com",
        phase: "phase1",
        author: "aabbcc",
        has_hint: false,
    };
    let mut buf: Vec<u8> = Vec::new();
    write_wire_line(&mut buf, true, &event);
    assert!(!buf.is_empty(), "enabled gate must produce output");
}

#[test]
fn env_set_emits_one_line_per_event() {
    let events = [
        WireLogEvent::ReqEmit {
            sub_id: "sub-1",
            relay_url: "wss://r1.example.com",
            phase: "phase1",
            author: "aa",
            has_hint: false,
        },
        WireLogEvent::EoseRx {
            sub_id: "sub-1",
            relay_url: "wss://r1.example.com",
            matched: true,
        },
        WireLogEvent::EventRx {
            sub_id: "sub-1",
            relay_url: "wss://r1.example.com",
            event_id: "deadbeef",
            author: "aabb",
        },
    ];

    let mut buf: Vec<u8> = Vec::new();
    for ev in &events {
        write_wire_line(&mut buf, true, ev);
    }

    let output = String::from_utf8(buf).expect("valid UTF-8");
    let lines: Vec<&str> = output.lines().collect();
    assert_eq!(
        lines.len(),
        3,
        "exactly one line emitted per event; got:\n{output}"
    );
}

#[test]
fn output_line_starts_with_nmp_wire() {
    let event = WireLogEvent::ScoreUpdate {
        author: "aabb",
        relay_url: "wss://relay.example.com",
        delta: "+3",
        new_weight: 0.75,
    };

    let mut buf: Vec<u8> = Vec::new();
    write_wire_line(&mut buf, true, &event);

    let output = String::from_utf8(buf).expect("valid UTF-8");
    for line in output.lines() {
        assert!(
            line.starts_with("nmp.wire "),
            "line must start with 'nmp.wire '; got: {line:?}"
        );
    }
}

#[test]
fn serialized_event_has_expected_schema_for_w9_grep() {
    // Protects W9's grep-based acceptance tests from field-naming drift.
    // Parses the JSON payload and asserts the discriminant + key fields that
    // W9 greps for are present with exactly the expected values.
    let event = WireLogEvent::ReqEmit {
        sub_id: "test",
        relay_url: "wss://r.example.com",
        phase: "phase1",
        author: "deadbeef",
        has_hint: false,
    };

    let mut buf: Vec<u8> = Vec::new();
    write_wire_line(&mut buf, true, &event);

    let output = String::from_utf8(buf).expect("valid UTF-8");
    let line = output.lines().next().expect("at least one line");
    let json_str = line
        .strip_prefix("nmp.wire ")
        .expect("line must start with 'nmp.wire '");

    let v: serde_json::Value = serde_json::from_str(json_str).expect("payload must be valid JSON");

    assert_eq!(
        v["type"], "ReqEmit",
        "discriminant field must be 'ReqEmit'; got: {}",
        v["type"]
    );
    assert_eq!(
        v["phase"], "phase1",
        "phase field must be 'phase1'; got: {}",
        v["phase"]
    );
    assert_eq!(
        v["relay_url"], "wss://r.example.com",
        "relay_url field mismatch; got: {}",
        v["relay_url"]
    );
}
