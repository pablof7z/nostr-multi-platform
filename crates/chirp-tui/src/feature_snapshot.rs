use serde_json::Value;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct FeatureSnapshot {
    pub accounts: Vec<AccountLine>,
    pub active_account: String,
    pub outbox: Vec<OutboxLine>,
    pub outbox_summary: SummaryLine,
    pub relay_edit_rows: Vec<RelayEditLine>,
    pub wallet: WalletLine,
    pub dm_conversations: Vec<DmConversationLine>,
    pub group_messages: Vec<MessageLine>,
    pub discovered_groups: Vec<GroupLine>,
    pub follow_count: usize,
    pub settings_hub: SummaryLine,
    pub author_profile: Option<ProfileLine>,
    pub thread: Option<ThreadLine>,
}

impl FeatureSnapshot {
    #[must_use] 
    pub fn from_payload(payload: &str) -> Self {
        let Ok(value) = serde_json::from_str::<Value>(payload) else {
            return Self::default();
        };
        let projections = value
            .get("v")
            .and_then(|v| v.get("projections"))
            .or_else(|| value.get("projections"));
        Self::from_projections(projections)
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
            outbox_summary: summary_from(projections.get("outbox_summary")),
            relay_edit_rows: relay_edit_rows_from(projections),
            wallet: wallet_from(projections.get("wallet")),
            dm_conversations: dm_from(projections),
            group_messages: messages_from(projection(projections, "nmp.nip29.group_chat")),
            discovered_groups: groups_from(projections),
            follow_count: follow_count_from(projections),
            settings_hub: settings_hub_from(projections.get("settings_hub")),
            author_profile: profile_from(projections.get("author_view")),
            thread: thread_from(projections.get("thread_view")),
        }
    }

    #[must_use] 
    pub fn is_empty(&self) -> bool {
        self.accounts.is_empty()
            && self.outbox.is_empty()
            && self.relay_edit_rows.is_empty()
            && self.wallet.status.is_empty()
            && self.dm_conversations.is_empty()
            && self.group_messages.is_empty()
            && self.discovered_groups.is_empty()
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AccountLine {
    pub id: String,
    pub display: String,
    pub npub: String,
    pub signer: String,
    pub active: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OutboxLine {
    pub handle: String,
    pub title: String,
    pub status_label: String,
    pub preview: String,
    pub can_retry: bool,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct RelayEditLine {
    pub url: String,
    pub role_label: String,
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

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ProfileLine {
    pub pubkey: String,
    pub display: String,
    pub about: String,
    pub note_count: String,
    pub action_label: String,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ThreadLine {
    pub focused_event_id: String,
    pub state: String,
    pub previous_label: String,
    pub next_label: String,
    pub item_count: usize,
}

fn accounts_from(projections: &Value) -> Vec<AccountLine> {
    projections
        .get("accounts")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| AccountLine {
            id: string_field(row, "id"),
            display: first_nonempty(row, &["display_name", "displayName", "npub"]),
            npub: string_field(row, "npub"),
            signer: first_nonempty(row, &["signer_label", "signerLabel", "signer_kind"]),
            active: bool_field(row, "is_active") || bool_field(row, "isActive"),
        })
        .collect()
}

fn outbox_from(projections: &Value) -> Vec<OutboxLine> {
    projections
        .get("publish_outbox")
        .or_else(|| projections.get("publishOutbox"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| OutboxLine {
            handle: string_field(row, "handle"),
            title: string_field(row, "title"),
            status_label: first_nonempty(row, &["status_label", "statusLabel", "status"]),
            preview: string_field(row, "preview"),
            can_retry: bool_field(row, "can_retry") || bool_field(row, "canRetry"),
        })
        .collect()
}

fn relay_edit_rows_from(projections: &Value) -> Vec<RelayEditLine> {
    projections
        .get("relay_edit_rows")
        .or_else(|| projections.get("relayEditRows"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| RelayEditLine {
            url: string_field(row, "url"),
            role_label: first_nonempty(row, &["role_label", "roleLabel", "role"]),
        })
        .collect()
}

fn wallet_from(wallet: Option<&Value>) -> WalletLine {
    let Some(wallet) = wallet else {
        return WalletLine::default();
    };
    WalletLine {
        status: string_field(wallet, "status"),
        relay_url: first_nonempty(wallet, &["relay_url", "relayUrl"]),
        wallet_npub: first_nonempty(wallet, &["wallet_npub", "walletNpub"]),
        balance_msats: wallet
            .get("balance_msats")
            .or_else(|| wallet.get("balanceMsats"))
            .and_then(Value::as_u64),
    }
}

fn dm_from(projections: &Value) -> Vec<DmConversationLine> {
    projection(projections, "nmp.nip17.dm_inbox")
        .and_then(|dm| dm.get("conversations"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| {
            let messages = messages_from(Some(row));
            DmConversationLine {
                peer_pubkey: first_nonempty(row, &["peer_pubkey", "peerPubkey"]),
                peer_display: first_nonempty(row, &["peer_short_npub", "peerShortNpub"]),
                latest: messages
                    .last()
                    .map(|m| m.content.clone())
                    .unwrap_or_default(),
                messages,
            }
        })
        .collect()
}

fn messages_from(value: Option<&Value>) -> Vec<MessageLine> {
    value
        .and_then(|v| v.get("messages"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| MessageLine {
            id: string_field(row, "id"),
            author: first_nonempty(row, &["sender_pubkey", "senderPubkey", "pubkey"]),
            content: string_field(row, "content"),
            outgoing: bool_field(row, "is_outgoing") || bool_field(row, "isOutgoing"),
        })
        .collect()
}

fn groups_from(projections: &Value) -> Vec<GroupLine> {
    projection(projections, "nmp.nip29.discovered_groups")
        .and_then(|groups| groups.get("groups"))
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .map(|row| GroupLine {
            host_relay_url: first_nonempty(row, &["host_relay_url", "hostRelayUrl"]),
            group_id: first_nonempty(row, &["group_id", "groupId"]),
            name: optional_string(row, "name")
                .unwrap_or_else(|| first_nonempty(row, &["group_id", "groupId"])),
            about: string_field(row, "about"),
            member_count: number_field(row, "member_count") + number_field(row, "memberCount"),
            open: bool_field(row, "open"),
        })
        .collect()
}

fn follow_count_from(projections: &Value) -> usize {
    projection(projections, "nmp.follow_list")
        .and_then(|f| f.get("follows"))
        .and_then(Value::as_array)
        .map_or(0, Vec::len)
}

fn profile_from(value: Option<&Value>) -> Option<ProfileLine> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    let profile = value.get("profile").unwrap_or(value);
    Some(ProfileLine {
        pubkey: {
            let outer = first_nonempty(value, &["pubkey"]);
            if outer.is_empty() {
                string_field(profile, "pubkey")
            } else {
                outer
            }
        },
        display: string_field(profile, "display"),
        about: string_field(profile, "about"),
        note_count: first_nonempty(value, &["note_count_display", "noteCountDisplay"]),
        action_label: value
            .get("primary_action")
            .or_else(|| value.get("primaryAction"))
            .map(|a| string_field(a, "label"))
            .unwrap_or_default(),
    })
}

fn thread_from(value: Option<&Value>) -> Option<ThreadLine> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    Some(ThreadLine {
        focused_event_id: first_nonempty(value, &["focused_event_id", "focusedEventId"]),
        state: string_field(value, "state"),
        previous_label: first_nonempty(value, &["previous_count_label", "previousCountLabel"]),
        next_label: first_nonempty(value, &["next_count_label", "nextCountLabel"]),
        item_count: value
            .get("items")
            .and_then(Value::as_array)
            .map_or(0, Vec::len),
    })
}

fn summary_from(value: Option<&Value>) -> SummaryLine {
    value.map_or_else(SummaryLine::default, |v| SummaryLine {
        title: string_field(v, "title"),
        subtitle: string_field(v, "subtitle"),
    })
}

fn settings_hub_from(value: Option<&Value>) -> SummaryLine {
    value.map_or_else(SummaryLine::default, |v| SummaryLine {
        title: "Settings".to_string(),
        subtitle: first_nonempty(v, &["relays_subtitle", "relaysSubtitle"]),
    })
}

fn projection<'a>(projections: &'a Value, key: &str) -> Option<&'a Value> {
    projections
        .get(key)
        .or_else(|| projections.get(key.replace("_", "").as_str()))
}

fn first_nonempty(value: &Value, keys: &[&str]) -> String {
    keys.iter()
        .find_map(|key| optional_string(value, key))
        .unwrap_or_default()
}

fn string_field(value: &Value, key: &str) -> String {
    optional_string(value, key).unwrap_or_default()
}

fn optional_string(value: &Value, key: &str) -> Option<String> {
    value
        .get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .map(str::to_string)
}

fn bool_field(value: &Value, key: &str) -> bool {
    value.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn number_field(value: &Value, key: &str) -> u64 {
    value.get(key).and_then(Value::as_u64).unwrap_or_default()
}
