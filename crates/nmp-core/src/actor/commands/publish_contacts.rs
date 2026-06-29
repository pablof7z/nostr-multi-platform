//! Contact-list publish handlers for kind:3 follow edits.

use crate::actor::commands::identity::{sign_active_nonblocking, IdentityRuntime};
use crate::actor::commands::publish_failures::{fail_publish, toast_no_account};
use crate::actor::commands::publish_finalize::finalize_before_sign;
use crate::actor::pending_sign::{ParkedOp, ParkedSignerOps};
use crate::kernel::Kernel;
use crate::publish::PublishTarget;
use crate::relay::OutboundMessage;
use nmp_signer_iface::UnsignedEvent;

/// Add (`add == true`) or remove a follow from the active account's kind:3
/// set and re-publish the full list (NIP-02 replaceable).
pub(crate) fn follow(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    pubkey: &str,
    add: bool,
    correlation_id: Option<String>,
    parked_ops: &mut ParkedSignerOps,
) -> Vec<OutboundMessage> {
    let Some(author) = identity.active_pubkey() else {
        return toast_no_account(
            kernel,
            if add { "follow" } else { "unfollow" },
            correlation_id,
        );
    };
    if !crate::kernel::is_hex_pubkey(pubkey) {
        return fail_publish(
            kernel,
            "follow: expected 64-hex pubkey".to_string(),
            correlation_id,
        );
    }
    let Some((current_tags, current_content, baseline_created_at)) =
        kernel.try_current_kind3_event_for_edit()
    else {
        return fail_publish(kernel, "follow_list_not_loaded".to_string(), correlation_id);
    };
    let tags = if add {
        crate::tags::kind3_tags_after_add(&current_tags, pubkey)
    } else {
        crate::tags::kind3_tags_after_remove(&current_tags, pubkey)
    };
    let unsigned = UnsignedEvent {
        pubkey: author,
        kind: 3,
        tags,
        content: current_content,
        created_at: kernel.now_secs().max(baseline_created_at.saturating_add(1)),
    };
    sign_and_publish_contact_edit(identity, kernel, unsigned, correlation_id, parked_ops)
}

/// Bulk-follow: merge `pubkeys` into the active account's kind:3 and
/// re-publish it exactly once.
pub(crate) fn follow_many(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    pubkeys: &[String],
    active_pubkey_hint: Option<&str>,
    correlation_id: Option<String>,
    parked_ops: &mut ParkedSignerOps,
) -> Vec<OutboundMessage> {
    let Some(author) = identity.active_pubkey() else {
        return toast_no_account(kernel, "follow_many", correlation_id);
    };
    let Some((current_tags, current_content, baseline_created_at)) =
        kernel.try_current_kind3_event_for_edit()
    else {
        return fail_publish(kernel, "follow_list_not_loaded".to_string(), correlation_id);
    };

    let self_pk: &str = &author;
    let mut merged_tags = current_tags;
    for pk in pubkeys {
        if pk.len() != 64 || !crate::kernel::is_hex_pubkey(pk) {
            continue;
        }
        if pk.as_str() == self_pk || active_pubkey_hint.map_or(false, |h| h == pk.as_str()) {
            continue;
        }
        merged_tags = crate::tags::kind3_tags_after_add(&merged_tags, pk);
    }

    let unsigned = UnsignedEvent {
        pubkey: author,
        kind: 3,
        tags: merged_tags,
        content: current_content,
        created_at: kernel.now_secs().max(baseline_created_at.saturating_add(1)),
    };
    sign_and_publish_contact_edit(identity, kernel, unsigned, correlation_id, parked_ops)
}

fn sign_and_publish_contact_edit(
    identity: &IdentityRuntime,
    kernel: &mut Kernel,
    mut unsigned: UnsignedEvent,
    correlation_id: Option<String>,
    parked_ops: &mut ParkedSignerOps,
) -> Vec<OutboundMessage> {
    finalize_before_sign(kernel, &mut unsigned);
    let mut op = match sign_active_nonblocking(identity, &unsigned) {
        Ok(op) => op,
        Err(reason) => return fail_publish(kernel, reason, correlation_id),
    };
    match op.poll() {
        Some(Ok(signed)) => kernel.publish_signed_with_correlation(&signed, &[], correlation_id),
        Some(Err(e)) => fail_publish(kernel, format!("sign failed: {e}"), correlation_id),
        None => {
            parked_ops.push(ParkedOp::publish(
                op,
                Vec::new(),
                PublishTarget::Auto,
                correlation_id,
                identity.active_sign_deadline(),
            ));
            Vec::new()
        }
    }
}
