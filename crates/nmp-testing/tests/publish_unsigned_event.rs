//! Coverage for the post-PR-F publish flow: per-NIP builder →
//! `ActorCommand::PublishUnsignedEvent` → kernel signs + routes via the
//! NIP-65 outbox resolver (D3).
//!
//! PR-F (one door per capability) deleted the bespoke `extern "C"`
//! event-producing FFI surface (`nmp_app_publish_signed_event{,_to}` /
//! `nmp_app_publish_unsigned_event`). Every user / app-authored publish
//! now reaches the kernel through one of two doors:
//!
//! 1. `nmp_app_dispatch_action("nmp.publish", ...)` — the single
//!    namespace-keyed action seam for content actions (the front door).
//!    Internally it routes through `ActorCommand::PublishUnsignedEvent` /
//!    `ActorCommand::PublishSignedEvent` exactly as the deleted FFI used to.
//! 2. `NmpApp::publish_signed_explicit` — the workspace-internal pure-Rust
//!    kernel API for system-authored events the kernel signer cannot
//!    re-mint (MLS group commits, NIP-59 gift wraps). Theme A's
//!    system-authored / lifecycle exception.
//!
//! What this file still pins:
//!
//! 1. **Builder wire shape** — `nmp_nip23::Article::new(...)
//!    .build(...)` produces a wire-form `UnsignedEvent` with the expected
//!    kind + canonical tag order. Pure, no actor.
//! 2. **Compile-shape handoff** — the `UnsignedEvent` plugs directly into
//!    `ActorCommand::PublishUnsignedEvent(_)` so apps don't re-wrap or
//!    convert (the action seam's executor lands on this exact variant).
//! 3. **`ShowToast` end-to-end** — the `dispatch_action` JSON-decode path
//!    surfaces parse failures through `ActorCommand::ShowToast`; this
//!    file's coverage of that primitive is independent of which FFI door
//!    sends the toast.

use nmp_core::testing::ActorCommand;
use nmp_nip23::Article;
use std::time::Duration;

#[test]
fn build_article_unsigned_event_has_expected_wire_shape() {
    // Pure builder — no actor, no signing, no clock. This is the only part
    // an app touches before handing off to PublishUnsignedEvent (whether
    // directly inside a kernel-side executor, or indirectly through the
    // `dispatch_action` seam — both land on the same variant).
    let unsigned = Article::new("my-article")
        .title("Hello")
        .summary("a short summary")
        .image("https://example.com/cover.png")
        .published_at(1_700_000_000)
        .content("# Heading\n\nMarkdown body")
        .build("ignored-by-signer", 1_700_000_100)
        .expect("article builder produces an UnsignedEvent");
    assert_eq!(unsigned.kind, 30023);
    assert_eq!(unsigned.content, "# Heading\n\nMarkdown body");
    let keys: Vec<&str> = unsigned
        .tags
        .iter()
        .filter_map(|t| t.first())
        .map(String::as_str)
        .collect();
    assert_eq!(keys, vec!["d", "title", "image", "summary", "published_at"]);
}

#[test]
fn builder_output_plugs_directly_into_publish_unsigned_event_command() {
    // The shape pin: `ArticleBuilder::build()` returns the exact
    // `UnsignedEvent` type `ActorCommand::PublishUnsignedEvent(_)` expects.
    //
    // The post-PR-F flow inside an `nmp.publish` action executor:
    //
    //   let unsigned = Article::new(d).title(t).content(c).build(pk, ts)?;
    //   send(ActorCommand::PublishUnsignedEvent(unsigned));
    //
    // (The C / Swift / Kotlin shells reach this same variant by going
    // through `nmp_app_dispatch_action` — the deleted
    // `nmp_app_publish_unsigned_event` no longer exists.)
    //
    // The runtime sign + outbox-route behaviour is covered by the
    // `publish_unsigned_event_signs_and_publishes_arbitrary_kind` unit test
    // in `nmp-core::actor::commands::tests` (drives Kernel +
    // IdentityRuntime directly against the same handler the actor uses).
    let unsigned = Article::new("plug-in-test")
        .title("Hi")
        .content("body")
        .build("placeholder-pk", 1_700_000_200)
        .expect("article builder");
    let cmd = ActorCommand::PublishUnsignedEvent { event: unsigned, correlation_id: None };
    // Confirm the variant carries the kind through unchanged — extracting
    // by pattern-match also doubles as a compile-time shape lock.
    if let ActorCommand::PublishUnsignedEvent { event: u, .. } = cmd {
        assert_eq!(u.kind, 30023);
    } else {
        panic!("expected PublishUnsignedEvent variant");
    }
}

#[test]
fn unsigned_event_serde_round_trips_for_action_payload() {
    // The post-PR-F door for non-Rust hosts (Swift / Kotlin) is
    // `nmp_app_dispatch_action` under the `nmp.publish` namespace. The
    // payload it carries is a JSON-encoded `UnsignedEvent` — exactly what
    // this round-trip pins. Builders produce something the action seam's
    // executor will decode without loss.
    let unsigned = Article::new("dispatch-shape")
        .title("Wire")
        .content("hello")
        .build("pk", 1_700_000_300)
        .expect("article builder");
    let json = serde_json::to_string(&unsigned).expect("UnsignedEvent serialises");
    let decoded: nmp_core::substrate::UnsignedEvent =
        serde_json::from_str(&json).expect("UnsignedEvent round-trips");
    assert_eq!(decoded, unsigned);
}

// ── ShowToast end-to-end ────────────────────────────────────────────────────
//
// `ActorCommand::ShowToast` is the actor-side primitive that surfaces
// publish-path parse failures (and other recoverable errors) as kernel
// snapshot state. PR-F deleted the bespoke FFI symbol that used to be the
// only Swift caller of this primitive, but the primitive itself stays —
// the `dispatch_action` JSON-decode path emits the same `ShowToast` when a
// host hands it malformed action JSON.

/// Drain the update channel until either a snapshot containing
/// `last_error_toast` with the given substring appears or the deadline passes.
fn find_toast_in_updates(
    rx: &std::sync::mpsc::Receiver<String>,
    expected: &str,
) -> bool {
    let deadline = std::time::Instant::now() + Duration::from_secs(3);
    while std::time::Instant::now() < deadline {
        match rx.recv_timeout(Duration::from_millis(200)) {
            Ok(json) => {
                // The snapshot is a JSON envelope; we just check for the string
                // rather than parsing the full shape — avoids a hard dep on the
                // exact snapshot schema.
                if json.contains(expected) {
                    return true;
                }
            }
            // Timeout: the actor may be mid-block in relay_rx.recv_timeout
            // (up to 250ms idle wait). Keep polling until the 3-second
            // deadline — a single empty slot does NOT mean the channel is done.
            Err(_) => continue,
        }
    }
    false
}

#[test]
fn show_toast_command_surfaces_message_in_snapshot() {
    // The `dispatch_action` decoder (and every other in-actor handler that
    // needs to surface a recoverable failure) emits
    // `ActorCommand::ShowToast`; the actor folds it into
    // `kernel.set_last_error_toast`, which appears in the next snapshot
    // emission as `last_error_toast`. This test pins that primitive
    // end-to-end via `spawn_actor`.
    let (tx, rx) = nmp_core::testing::spawn_actor();
    tx.send(ActorCommand::Start {
        visible_limit: 64,
        emit_hz: 60,
    })
    .unwrap();
    // Drain all initial snapshots (relay connections generate several).
    // Use a short window — just enough to ensure the Start completes.
    let drain_deadline = std::time::Instant::now() + Duration::from_millis(300);
    while std::time::Instant::now() < drain_deadline {
        let _ = rx.recv_timeout(Duration::from_millis(50));
    }

    tx.send(ActorCommand::ShowToast {
        message: "Failed to decode action payload".to_string(),
    })
    .unwrap();

    // The actor uses `maybe_emit_after_dispatch` which emits immediately when
    // running. Poll for up to 3s to accommodate scheduler jitter.
    assert!(
        find_toast_in_updates(&rx, "Failed to decode action payload"),
        "ShowToast must cause the message to appear in the kernel snapshot"
    );

    let _ = tx.send(ActorCommand::Shutdown);
}
