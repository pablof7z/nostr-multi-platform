//! ADR-0065 family dispatchers for publish / contacts / relay / action-ledger.
//!
//! These four `dispatch_*` functions fan incoming sub-enum commands out to the
//! individual verb handlers defined in `cmd_publish.rs`.  Extracted from
//! `cmd_publish.rs` to keep that file under the LOC ceiling.

use crate::actor::{ActionLedgerCommand, ContactsCommand, RelayCommand, PublishCommand};
use crate::relay::OutboundMessage;

use super::ActorContext;
use super::cmd_publish::{
    ack_action_stage, add_relay, cancel_publish, clear_active_follows_feed,
    declare_active_follows_feed, follow_many, follow_or_unfollow, publish_profile,
    publish_raw_event, publish_signed_event, publish_unsigned_event,
    publish_unsigned_event_to_relays, reconnect_relays_cmd, record_action_failure,
    record_action_success, remove_relay, retry_publish, set_relay_info,
};

/// `PublishCommand` family dispatch.
pub(super) fn dispatch_publish(
    cmd: PublishCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        PublishCommand::RawEvent { kind, tags, content, target, signer_pubkey, correlation_id } =>
            publish_raw_event(kind, tags, content, target, signer_pubkey, correlation_id, ctx),
        PublishCommand::Profile { fields, correlation_id } =>
            publish_profile(fields, correlation_id, ctx),
        PublishCommand::UnsignedEvent { event: unsigned, correlation_id, signer_pubkey } =>
            publish_unsigned_event(unsigned, correlation_id, signer_pubkey, ctx),
        PublishCommand::UnsignedEventToRelays { event, relays, correlation_id, signer_pubkey } =>
            publish_unsigned_event_to_relays(event, relays, correlation_id, signer_pubkey, ctx),
        PublishCommand::SignedEvent { raw, target, correlation_id } =>
            publish_signed_event(raw, target, correlation_id, ctx),
        PublishCommand::RetryPublish { handle } => retry_publish(handle, ctx),
        PublishCommand::CancelPublish { correlation_id } => cancel_publish(correlation_id, ctx),
    }
}

/// `ContactsCommand` family dispatch.
pub(super) fn dispatch_contacts(
    cmd: ContactsCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        ContactsCommand::Follow { pubkey, correlation_id } =>
            follow_or_unfollow(pubkey, true, correlation_id, ctx),
        ContactsCommand::Unfollow { pubkey, correlation_id } =>
            follow_or_unfollow(pubkey, false, correlation_id, ctx),
        ContactsCommand::FollowMany { pubkeys, correlation_id } =>
            follow_many(pubkeys, correlation_id, ctx),
        ContactsCommand::DeclareActiveFollowsFeed { acquisition_kinds } =>
            declare_active_follows_feed(acquisition_kinds, ctx),
        ContactsCommand::ClearActiveFollowsFeed => clear_active_follows_feed(ctx),
    }
}

/// `RelayCommand` family dispatch.
pub(super) fn dispatch_relay(
    cmd: RelayCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        RelayCommand::AddRelay { url, role } => add_relay(url, role, ctx),
        RelayCommand::RemoveRelay { url } => remove_relay(url, ctx),
        RelayCommand::ReconnectRelays => reconnect_relays_cmd(ctx),
        RelayCommand::SetRelayInfo { relay_url, doc_json } =>
            set_relay_info(relay_url, doc_json, ctx),
    }
}

/// `ActionLedgerCommand` family dispatch.
pub(super) fn dispatch_action_ledger(
    cmd: ActionLedgerCommand,
    ctx: &mut ActorContext<'_>,
) -> Option<Vec<OutboundMessage>> {
    match cmd {
        ActionLedgerCommand::Ack(correlation_id) => ack_action_stage(correlation_id, ctx),
        ActionLedgerCommand::RecordFailure { correlation_id, reason } =>
            record_action_failure(correlation_id, reason, ctx),
        ActionLedgerCommand::RecordSuccess { correlation_id, result_json } =>
            record_action_success(correlation_id, result_json, ctx),
    }
}
