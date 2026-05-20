use super::*;
use crate::substrate::SignedEvent;

#[derive(Clone, Debug, Deserialize)]
pub(super) struct NostrEvent {
    pub(super) id: String,
    pub(super) pubkey: String,
    pub(super) created_at: u64,
    pub(super) kind: u32,
    pub(super) tags: Vec<Vec<String>>,
    pub(super) content: String,
    /// Schnorr signature (hex). Present in all valid NIP-01 events.
    /// Default to empty string so legacy test fixtures without `sig` still parse.
    #[serde(default)]
    pub(super) sig: String,
}

#[derive(Default, Deserialize)]
pub(super) struct ProfileContent {
    pub(super) name: Option<String>,
    pub(super) display_name: Option<String>,
    #[serde(rename = "displayName")]
    pub(super) display_name_camel: Option<String>,
    pub(super) picture: Option<String>,
    pub(super) nip05: Option<String>,
    pub(super) about: Option<String>,
}

pub(super) fn parse_profile(event: &NostrEvent) -> Profile {
    let parsed = serde_json::from_str::<ProfileContent>(&event.content).unwrap_or_default();
    let display = parsed
        .display_name
        .or(parsed.display_name_camel)
        .or(parsed.name)
        .filter(|value| !value.trim().is_empty())
        .unwrap_or_else(|| short_pubkey_display(&event.pubkey));
    Profile {
        event_id: event.id.clone(),
        created_at: event.created_at,
        avatar_initials: initials(&display),
        avatar_color: avatar_color(&event.pubkey),
        display,
        picture_url: parsed.picture.filter(|value| value.starts_with("http")),
        nip05: parsed.nip05.unwrap_or_default(),
        about: parsed.about.unwrap_or_default(),
    }
}

pub(super) fn parse_profile_intent(event: &SignedEvent) -> Option<Profile> {
    if event.unsigned.kind != 0 {
        return None;
    }
    let event = signed_event_to_nostr(event);
    Some(parse_profile(&event))
}

pub(super) fn signed_event_to_nostr(event: &SignedEvent) -> NostrEvent {
    NostrEvent {
        id: event.id.clone(),
        pubkey: event.unsigned.pubkey.clone(),
        created_at: event.unsigned.created_at,
        kind: event.unsigned.kind,
        tags: event.unsigned.tags.clone(),
        content: event.unsigned.content.clone(),
        sig: event.sig.clone(),
    }
}

pub(super) fn diff_items(
    previous: &[TimelineItem],
    current: &[TimelineItem],
) -> (Vec<TimelineItem>, Vec<TimelineItem>, Vec<String>) {
    let previous_by_id = previous
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<HashMap<_, _>>();
    let current_by_id = current
        .iter()
        .map(|item| (item.id.as_str(), item))
        .collect::<HashMap<_, _>>();

    let inserted = current
        .iter()
        .filter(|item| !previous_by_id.contains_key(item.id.as_str()))
        .cloned()
        .collect::<Vec<_>>();
    let updated = current
        .iter()
        .filter(|item| {
            previous_by_id
                .get(item.id.as_str())
                .map(|previous| *previous != *item)
                .unwrap_or(false)
        })
        .cloned()
        .collect::<Vec<_>>();
    let removed = previous
        .iter()
        .filter(|item| !current_by_id.contains_key(item.id.as_str()))
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();

    (inserted, updated, removed)
}

pub(super) fn ratio(numerator: usize, denominator: usize) -> f64 {
    if denominator == 0 {
        0.0
    } else {
        numerator as f64 / denominator as f64
    }
}

pub(crate) fn is_hex_pubkey(value: &str) -> bool {
    value.len() == 64 && value.chars().all(|c| c.is_ascii_hexdigit())
}

pub(crate) fn is_hex_id(value: &str) -> bool {
    is_hex_pubkey(value)
}

pub(super) fn parse_relay_list(
    event_id: &str,
    created_at: u64,
    tags: &[Vec<String>],
) -> AuthorRelayList {
    let mut list = AuthorRelayList {
        event_id: event_id.to_string(),
        created_at,
        ..AuthorRelayList::default()
    };
    let mut seen = HashSet::new();

    for tag in tags {
        if tag.first().map(String::as_str) != Some("r") {
            continue;
        }
        let Some(url) = tag.get(1).filter(|url| url.starts_with("wss://")) else {
            continue;
        };
        let marker = tag.get(2).map(String::as_str).unwrap_or("both");
        let key = format!("{url}:{marker}");
        if !seen.insert(key) {
            continue;
        }
        match marker {
            "read" => list.read_relays.push(url.clone()),
            "write" => list.write_relays.push(url.clone()),
            _ => list.both_relays.push(url.clone()),
        }
    }

    list
}

pub(super) fn event_references(event: &StoredEvent, event_id: &str) -> bool {
    event.tags.iter().any(|tag| {
        tag.first().map(String::as_str) == Some("e") && tag.get(1).is_some_and(|id| id == event_id)
    })
}

pub(super) fn referenced_event_ids(event: &StoredEvent) -> BTreeSet<String> {
    event
        .tags
        .iter()
        .filter_map(|tag| {
            if tag.first().map(String::as_str) == Some("e") {
                tag.get(1).filter(|id| is_hex_id(id)).cloned()
            } else {
                None
            }
        })
        .collect()
}

pub(super) fn root_event_id(event: &StoredEvent) -> Option<String> {
    marked_event_ref(event, "root")
}

pub(super) fn first_event_ref(event: &StoredEvent) -> Option<String> {
    event.tags.iter().find_map(|tag| {
        if tag.first().map(String::as_str) == Some("e") {
            tag.get(1).filter(|id| is_hex_id(id)).cloned()
        } else {
            None
        }
    })
}

pub(super) fn marked_event_ref(event: &StoredEvent, marker: &str) -> Option<String> {
    event.tags.iter().find_map(|tag| {
        if tag.first().map(String::as_str) == Some("e")
            && tag.get(3).map(String::as_str) == Some(marker)
        {
            tag.get(1).filter(|id| is_hex_id(id)).cloned()
        } else {
            None
        }
    })
}

pub(super) fn short_hex(value: &str) -> String {
    if value.len() < 12 {
        value.to_string()
    } else {
        format!("{}..{}", &value[..6], &value[value.len() - 6..])
    }
}

pub(super) fn short_pubkey_display(value: &str) -> String {
    if value.len() < 16 {
        value.to_string()
    } else {
        format!("npub {}..{}", &value[..8], &value[value.len() - 8..])
    }
}

pub(super) fn truncate(value: &str, limit: usize) -> String {
    let mut out = String::new();
    for ch in value.chars().take(limit) {
        out.push(ch);
    }
    if value.chars().count() > limit {
        out.push_str("...");
    }
    out
}

pub(super) fn initials(display: &str) -> String {
    let mut chars = display.chars().filter(|ch| ch.is_alphanumeric());
    let first = chars.next().unwrap_or('.');
    let second = chars.next().unwrap_or('.');
    format!("{}{}", first, second).to_uppercase()
}

pub(super) fn avatar_color(pubkey: &str) -> String {
    let bytes = pubkey.as_bytes();
    let r = bytes.first().copied().unwrap_or(0x55);
    let g = bytes.get(7).copied().unwrap_or(0x88);
    let b = bytes.get(15).copied().unwrap_or(0xaa);
    format!("#{r:02x}{g:02x}{b:02x}")
}

pub(super) fn format_timestamp(created_at: u64) -> String {
    let Some(system_time) = UNIX_EPOCH.checked_add(Duration::from_secs(created_at)) else {
        return created_at.to_string();
    };
    let datetime: DateTime<Local> = DateTime::<Local>::from(system_time);
    datetime.format("%b %-d %H:%M").to_string()
}

pub(super) fn now_hms() -> String {
    let now = SystemTime::now();
    let datetime: DateTime<Local> = DateTime::<Local>::from(now);
    datetime.format("%H:%M:%S").to_string()
}
