use serde_json::Value;

use crate::bridge::UpdatePayload;
use crate::feature_snapshot_json::{
    accounts_from, configured_relays_from, dm_from, follow_count_from, groups_from, messages_from,
    outbox_from, outbox_summary_from, projection, publish_history_from, settings_hub_from,
    string_field, wallet_from,
};

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureSnapshot {
    pub accounts: Vec<AccountLine>,
    pub active_account: String,
    pub outbox: Vec<OutboxLine>,
    pub outbox_summary: SummaryLine,
    /// Settled publish history from `projections.publish_queue`.
    pub history: Vec<PublishHistoryLine>,
    pub configured_relays: Vec<RelayEditLine>,
    pub wallet: WalletLine,
    pub dm_conversations: Vec<DmConversationLine>,
    pub group_messages: Vec<MessageLine>,
    pub discovered_groups: Vec<GroupLine>,
    pub follow_count: usize,
    pub settings_hub: SummaryLine,
    // V-112 (ADR-0042): author_profile (from deleted author_view projection) and
    // thread (from deleted thread_view projection) removed. Profile/thread note
    // lists read from dynamic nmp.feed.author.* / nmp.feed.thread.* flat-feed
    // projections decoded by SharedSnapshot.
    //
    // ADR-0063 (#1671 Lane G): `resolved_profiles` removed. Profile data is now
    // sourced exclusively from `AppState::ref_profiles` (a `RefProfileStore`
    // merged from the `refs.profile` row-delta projection). `FeatureSnapshot`
    // carries no profile map; shells read via `AppState::profile(pubkey)`.
}

impl FeatureSnapshot {
    /// Build from a live transport payload (FlatBuffers typed-first path).
    ///
    /// All data comes from typed sidecars (ADR-0037 / ADR-0044 PR-B):
    /// - Kernel built-in projections are decoded via the Tier-2 typed codec
    ///   functions promoted to `pub` in `nmp_core::typed_projections`.
    /// - Host-registered projections (nmp.nip17, nmp.nip29, nmp.nip02,
    ///   nmp.nip47) are decoded via their respective crate's public
    ///   `decode_*` function.
    ///
    /// When a sidecar entry is absent or fails to decode (ADR-0037 Commitment 4),
    /// the corresponding field returns its zero/`None` value — no `payload:Value`
    /// fallback.  The JSON fixture path retains the old `from_snapshot_value`
    /// approach because fixtures are test/dev artefacts only and are not on the
    /// hot path.
    #[must_use]
    pub fn from_transport_payload(payload: &UpdatePayload) -> Self {
        match payload {
            UpdatePayload::FlatBuffers(bytes) => {
                crate::feature_snapshot_typed::feature_snapshot_from_flatbuffer(bytes)
            }
            UpdatePayload::JsonFixture(json) => {
                let Ok(value) = serde_json::from_str::<Value>(json) else {
                    return Self::default();
                };
                let snapshot = value.get("v").unwrap_or(&value);
                Self::from_snapshot_value(snapshot)
            }
        }
    }

    #[must_use]
    pub fn from_json_fixture(payload: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return Self::default();
        };
        let snapshot = value.get("v").unwrap_or(&value);
        Self::from_snapshot_value(snapshot)
    }

    // -----------------------------------------------------------------------
    // JSON fixture / legacy path (test/dev artefacts only)
    // -----------------------------------------------------------------------

    fn from_snapshot_value(value: &Value) -> Self {
        Self::from_projections(value.get("projections"))
    }

    #[must_use]
    pub fn from_projections(projections: Option<&Value>) -> Self {
        let Some(projections) = projections else {
            return Self::default();
        };
        Self {
            accounts: accounts_from(projections),
            active_account: string_field(projections, "active_account"),
            outbox: outbox_from(projections),
            outbox_summary: outbox_summary_from(projections.get("outbox_summary")),
            history: publish_history_from(projections),
            configured_relays: configured_relays_from(projections),
            wallet: wallet_from(projections.get("wallet")),
            dm_conversations: dm_from(projections),
            group_messages: messages_from(projection(projections, "nmp.nip29.group_events")),
            discovered_groups: groups_from(projections),
            follow_count: follow_count_from(projections),
            settings_hub: settings_hub_from(projections.get("settings_hub")),
        }
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
            && self.outbox.is_empty()
            && self.history.is_empty()
            && self.configured_relays.is_empty()
            && self.wallet.status.is_empty()
            && self.dm_conversations.is_empty()
            && self.group_messages.is_empty()
            && self.discovered_groups.is_empty()
    }
}

// ---------------------------------------------------------------------------
// Data types
// ---------------------------------------------------------------------------

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountLine {
    pub id: String,
    pub display: String,
    pub npub: String,
    pub signer: String,
    pub active: bool,
}

/// Shell-side signer label derived from the raw `signer_kind` wire token.
///
/// The kernel used to ship a pre-rendered `signer_label` String, but that was a
/// presentation artifact removed from the wire (#1712, D7/D27). The TUI is a
/// presentation shell, so it owns this formatting. Unknown kinds fall back to
/// the raw token.
pub fn signer_label_for_kind(kind: &str) -> String {
    match kind {
        "local" => "Local key".to_string(),
        "nip46" => "NIP-46".to_string(),
        other => other.to_string(),
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutboxLine {
    pub handle: String,
    pub title: String,
    pub status_label: String,
    pub preview: String,
    pub can_retry: bool,
    pub relays: Vec<OutboxRelayLine>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutboxRelayLine {
    pub relay_url: String,
    pub status_label: String,
    pub reason: String,
    pub message: String,
}

/// One settled publish row from `projections.publish_queue`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct PublishHistoryLine {
    pub event_id: String,
    pub kind: u32,
    /// Pre-formatted kind label (e.g. `"Note"`, `"Reaction"`).
    pub title: String,
    /// Terminal status reported by the kernel.
    pub status: String,
    /// Rust-owned retry decision.
    pub can_retry: bool,
    pub relays: Vec<HistoryRelayLine>,
}

/// One relay's verdict on a settled publish. Mirrors the kernel's
/// `RelayAckOutcome` shape one-to-one.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct HistoryRelayLine {
    pub relay_url: String,
    /// `"ok"` for an accepted relay, `"failed"` otherwise.
    pub status: String,
    /// Per-relay selection rationale captured at publish time (e.g.
    /// `"NIP-65 write relay"`). Empty for older serialised rows.
    pub relay_reason: String,
    /// Failure reason for `"failed"` rows; empty for `"ok"`.
    pub message: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelayEditLine {
    pub url: String,
    pub role: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct WalletLine {
    pub status: String,
    pub relay_url: String,
    pub wallet_npub: String,
    pub balance_msats: Option<u64>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct DmConversationLine {
    pub peer_pubkey: String,
    pub peer_display: String,
    pub latest: String,
    pub messages: Vec<MessageLine>,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MessageLine {
    pub id: String,
    pub author: String,
    pub content: String,
    pub outgoing: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct GroupLine {
    pub host_relay_url: String,
    pub group_id: String,
    pub name: String,
    pub about: String,
    pub member_count: u64,
    pub open: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SummaryLine {
    pub title: String,
    pub subtitle: String,
}

/// Shared by the typed (`feature_snapshot_typed`) and JSON paths: render the
/// `settings_hub` relay-count subtitle.
pub(crate) fn relay_count_subtitle(count: u64) -> String {
    match count {
        0 => "No relays configured".to_string(),
        1 => "1 relay".to_string(),
        n => format!("{n} relays"),
    }
}
