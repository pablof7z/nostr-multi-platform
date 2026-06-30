use nmp_core::actor::{ActorCommand, InterestsCommand, PublishCommand};
use nmp_core::publish::PublishTarget;
use nmp_store::RawEvent;
use nostr::{Event, PublicKey, RelayUrl};

use super::{CachedWelcome, Inner, MarmotRuntimePort};
use crate::interest::KIND_MARMOT_KEY_PACKAGE;
use crate::service::MarmotService;

/// Lock-scoped accessor passed to action/read handlers. Keeps the `Mutex`
/// guard internal so handlers cannot leak it.
pub struct InnerHandle<'a> {
    pub(in crate::projection) inner: &'a mut Inner,
    pub(in crate::projection) port: Option<&'a dyn MarmotRuntimePort>,
}

impl<'a> InnerHandle<'a> {
    pub(crate) fn service(&self) -> &MarmotService {
        &self.inner.service
    }

    pub(crate) fn record_key_package(&mut self, d_tag: String, now_secs: u64) {
        self.inner.key_package_published_at = Some(now_secs);
        self.inner.key_package_d_tag = Some(d_tag);
    }

    /// Seed / overwrite the relay-pinned relay list for a group. Called
    /// from `create_group` (envelope `relays`) and `accept_welcome` /
    /// gift-wrap ingest (`Welcome::group_relays`). Empty list is ignored
    /// (keep any prior, more-specific entry).
    pub(crate) fn cache_group_relays(&mut self, group_id_hex: String, relays: Vec<RelayUrl>) {
        if relays.is_empty() {
            return;
        }
        let relay_urls = relays
            .iter()
            .map(|relay| relay.to_string())
            .collect::<Vec<_>>();
        self.inner.group_relays.insert(group_id_hex.clone(), relays);
        self.subscribe_group_messages(&group_id_hex, relay_urls);
    }

    pub(crate) fn send_actor_command(&self, cmd: ActorCommand) {
        if let Some(port) = self.port {
            port.send_actor_command(cmd);
            return;
        }
        let Some(sender) = self.inner.actor_sender.clone() else {
            return;
        };
        let _ = sender.send(cmd);
    }

    pub(crate) fn publish_signed_explicit(&self, event: &nostr::Event, relays: &[RelayUrl]) {
        if let Some(port) = self.port {
            port.publish_signed_explicit(event, relays);
            return;
        }
        let Some(sender) = self.inner.actor_sender.clone() else {
            return;
        };
        let raw = signed_event_to_raw(event);
        let relays = relays
            .iter()
            .map(std::string::ToString::to_string)
            .collect::<Vec<_>>();
        let _ = sender.send(ActorCommand::Publish(PublishCommand::SignedEvent {
            raw,
            target: PublishTarget::explicit(
                relays,
                nmp_core::publish::PublishRouteClass::ImportedOrPresigned,
            ),
            correlation_id: None,
        }));
    }

    fn ensure_interest(
        &self,
        identity: nmp_core::subs::SubIdentity,
        interest: nmp_planner::LogicalInterest,
    ) {
        if let Some(port) = self.port {
            port.ensure_interest(identity, interest);
            return;
        }
        let Some(sender) = self.inner.actor_sender.clone() else {
            return;
        };
        let _ = sender.send(ActorCommand::Interests(InterestsCommand::EnsureInterest {
            identity,
            interest,
        }));
    }

    fn subscribe_group_messages(&self, group_id_hex: &str, relay_urls: Vec<String>) {
        for (identity, interest) in
            crate::interest::group_message_registrations(group_id_hex, relay_urls)
        {
            self.ensure_interest(identity, interest);
        }
    }

    /// The cached relay-pinned relays for a group, or `&[]` on a miss
    /// (caller fails closed on the explicit publish boundary).
    #[must_use]
    pub(crate) fn group_relays(&self, group_id_hex: &str) -> Vec<RelayUrl> {
        self.inner
            .group_relays
            .get(group_id_hex)
            .cloned()
            .unwrap_or_default()
    }

    /// Publish a signed event to the group's relay-pinned relays
    /// (`Explicit`); a cache miss now fails closed instead of falling
    /// through to the author outbox.
    /// Used for kind:445 (group message / commit) and the kind:1059
    /// gift-wrap inbox-routing approximation.
    pub(crate) fn publish_group_pinned(&self, group_id_hex: &str, event: &nostr::Event) {
        let relays = self.group_relays(group_id_hex);
        crate::projection::publish::publish_to(self, event, &relays);
    }

    /// Publish a signed event to an EXPLICIT relay set (`Explicit`; empty
    /// -> fail closed). Used by `create_group` / `invite` while a borrowed
    /// `PendingGroupChange` is still live.
    pub(crate) fn publish_explicit(&self, event: &nostr::Event, relays: &[RelayUrl]) {
        crate::projection::publish::publish_to(self, event, relays);
    }

    /// Read the user's current write-relay URLs from the shared kernel
    /// relay-edit projection. Empty when no write relays are configured.
    #[must_use]
    pub(crate) fn write_relay_urls(&self) -> Vec<String> {
        let author = self.inner.service.public_key().to_hex();
        let Some(port) = self.port else {
            return Vec::new();
        };
        port.write_relay_urls(&author, KIND_MARMOT_KEY_PACKAGE)
    }

    /// Ask the kernel to fetch peer KeyPackage events for the given pubkeys.
    pub(crate) fn request_key_package_fetch(&self, pubkeys: &[PublicKey]) -> usize {
        let mut sent = 0;
        for pk in pubkeys {
            let pk_hex = pk.to_hex();
            self.ensure_interest(
                crate::interest::key_package_lookup_identity(&pk_hex),
                crate::interest::key_package_lookup_interest(&pk_hex),
            );
            sent += 1;
        }
        sent
    }

    /// Cache an incoming gift-wrap as a pending Welcome (no MLS type held).
    pub(crate) fn cache_welcome(
        &mut self,
        id_hex: String,
        gift_wrap: Event,
        group_name: String,
        inviter_npub: String,
    ) {
        self.inner.pending_welcomes.insert(
            id_hex,
            CachedWelcome {
                gift_wrap,
                group_name,
                inviter_npub,
            },
        );
    }

    /// Look up + remove a cached pending Welcome, returning the gift-wrap
    /// `Event` so the caller can re-run the idempotent
    /// `unwrap_and_process_welcome` to obtain the `&Welcome`.
    #[must_use]
    pub(crate) fn take_welcome_gift_wrap(&mut self, id_hex: &str) -> Option<Event> {
        self.inner
            .pending_welcomes
            .remove(id_hex)
            .map(|c| c.gift_wrap)
    }

    /// Restore a previously-taken Welcome (used when accept/decline fails so
    /// the row reappears in the next snapshot for a retry).
    pub(crate) fn restore_welcome(
        &mut self,
        id_hex: String,
        gift_wrap: Event,
        group_name: String,
        inviter_npub: String,
    ) {
        self.cache_welcome(id_hex, gift_wrap, group_name, inviter_npub);
    }
}

fn signed_event_to_raw(event: &nostr::Event) -> RawEvent {
    RawEvent {
        id: event.id.to_hex(),
        pubkey: event.pubkey.to_hex(),
        created_at: event.created_at.as_secs(),
        kind: u32::from(event.kind.as_u16()),
        tags: event.tags.iter().map(|t| t.as_slice().to_vec()).collect(),
        content: event.content.clone(),
        sig: event.sig.to_string(),
    }
}
