//! Pins the recommended developer flow for creating a Nostr event with NMP:
//! per-NIP builder → `ActorCommand::PublishUnsignedEvent` → kernel signs +
//! routes via the NIP-65 outbox resolver (D3).
//!
//! Two layers covered together:
//!
//! 1. **Builder wire shape** (this file) — `nmp_nip23::Article::new(...)
//!    .build(...)` produces a wire-form `UnsignedEvent` with the expected
//!    kind + canonical tag order. Pure, no actor.
//! 2. **Compile-shape handoff** (this file) — the `UnsignedEvent` plugs
//!    directly into `ActorCommand::PublishUnsignedEvent(_)` so apps don't
//!    re-wrap or convert.
//! 3. **Sign + publish runtime** (`nmp-core::actor::commands::tests`) — the
//!    actor's `publish_unsigned_event` handler signs with the active
//!    identity and queues the kind:30023 wire frame via the outbox
//!    resolver. Driven directly against `Kernel` + `IdentityRuntime` rather
//!    than through the actor's bounded command channel so the test stays
//!    deterministic.

use nmp_core::testing::ActorCommand;
use nmp_nip23::Article;

#[test]
fn build_article_unsigned_event_has_expected_wire_shape() {
    // Pure builder — no actor, no signing, no clock. This is the only part
    // an app touches before handing off to PublishUnsignedEvent.
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
    // Apps in production write:
    //
    //   let unsigned = Article::new(d).title(t).content(c).build(pk, ts)?;
    //   app.tx.send(ActorCommand::PublishUnsignedEvent(unsigned))?;
    //
    // Or via the FFI: `nmp_app_publish_unsigned_event(app, json_ptr)` where
    // `json_ptr` is serde_json::to_string(&unsigned).
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
    let cmd = ActorCommand::PublishUnsignedEvent(unsigned);
    // Confirm the variant carries the kind through unchanged — extracting
    // by pattern-match also doubles as a compile-time shape lock.
    if let ActorCommand::PublishUnsignedEvent(u) = cmd {
        assert_eq!(u.kind, 30023);
    } else {
        panic!("expected PublishUnsignedEvent variant");
    }
}

#[test]
fn unsigned_event_round_trips_through_ffi_json_shape() {
    // The FFI entry-point (`nmp_app_publish_unsigned_event`) takes a JSON
    // string of `UnsignedEvent` and routes it through the same command.
    // Apps that aren't in-process Rust (Swift, Kotlin) hit this path.
    let unsigned = Article::new("ffi-shape")
        .title("Wire")
        .content("hello")
        .build("pk", 1_700_000_300)
        .expect("article builder");
    let json = serde_json::to_string(&unsigned).expect("UnsignedEvent serialises");
    let decoded: nmp_core::substrate::UnsignedEvent =
        serde_json::from_str(&json).expect("UnsignedEvent round-trips");
    assert_eq!(decoded, unsigned);
}
