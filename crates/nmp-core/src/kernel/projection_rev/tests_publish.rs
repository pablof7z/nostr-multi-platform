//! ADR-0070 publish-cluster regressions.
//!
//! Split out of `tests.rs` so the REAL-driven scenario suite stays under the
//! file-size cap. These tests drive real kernel publish entrypoints and rely on
//! `make_update` running the projection oracle in `cfg(test)` builds.

use crate::kernel::projection_rev::ProjectionPresence;
use crate::kernel::publish_engine::OkFramePayload;
use crate::kernel::Kernel;
use crate::publish::PublishTarget;
use crate::relay::DEFAULT_VISIBLE_LIMIT;
use nmp_signer_iface::{SignedEvent, UnsignedEvent};

const WRITE_R1: &str = "wss://write-r1.test";

fn fake_signed(id: &str, author: &str, kind: u32, content: &str) -> SignedEvent {
    SignedEvent {
        id: id.to_string(),
        sig: format!("sig-{id}"),
        unsigned: UnsignedEvent {
            pubkey: author.to_string(),
            kind,
            tags: Vec::new(),
            content: content.to_string(),
            created_at: 1_700_000_000,
        },
    }
}

fn seed_kind10002(kernel: &mut Kernel, author_pubkey: &str, write_urls: &[&str]) {
    kernel.seed_kind10002_for_test(author_pubkey, write_urls);
}

fn ok_payload<'a>(event_id: &'a str, accepted: bool, reason: &'a str) -> OkFramePayload<'a> {
    OkFramePayload {
        event_id,
        ok: accepted,
        message: reason,
    }
}

fn emit(kernel: &mut Kernel) {
    let _ = kernel.make_update(true);
}

fn live_state(kernel: &Kernel, key: &str) -> (u64, ProjectionPresence) {
    let state = kernel.projection_state(key);
    (state.rev, state.presence)
}

#[test]
fn publish_outbox_rev_advances_when_in_flight_relay_state_changes() {
    let author = "22".repeat(32);
    let mut kernel = Kernel::new(DEFAULT_VISIBLE_LIMIT);
    seed_kind10002(&mut kernel, &author, &[WRITE_R1]);
    let signed = fake_signed("11".repeat(32).as_str(), &author, 1, "hello");

    let outbound = kernel.run_publish_engine_at(&signed, &[], PublishTarget::Auto, None, 1_000);
    assert_eq!(
        outbound.len(),
        1,
        "publish should target the seeded write relay"
    );

    emit(&mut kernel);
    let (outbox_before, outbox_presence_before) = live_state(&kernel, "publish_outbox");
    let (summary_before, summary_presence_before) = live_state(&kernel, "outbox_summary");
    assert_eq!(outbox_presence_before, ProjectionPresence::Unchanged);
    assert_eq!(summary_presence_before, ProjectionPresence::Unchanged);

    let _ = kernel.handle_publish_ok_at(
        WRITE_R1,
        ok_payload(&signed.id, false, "io: transient outage"),
        1_010,
    );

    let (outbox_after, outbox_presence_after) = live_state(&kernel, "publish_outbox");
    assert!(
        outbox_after > outbox_before,
        "transient relay failure changes publish_outbox in-flight payload and must \
         advance publish_engine_ver; before={outbox_before} after={outbox_after}"
    );
    assert_eq!(outbox_presence_after, ProjectionPresence::Changed);

    let (summary_after, summary_presence_after) = live_state(&kernel, "outbox_summary");
    assert!(
        summary_after > summary_before,
        "outbox_summary derives from the same engine in-flight snapshot"
    );
    assert_eq!(summary_presence_after, ProjectionPresence::Changed);

    emit(&mut kernel);
}
