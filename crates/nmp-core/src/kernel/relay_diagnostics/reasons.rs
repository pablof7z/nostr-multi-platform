//! Machine-token relay connection reasons derived from [`RelayAttribution`].
//!
//! Raw tokens are emitted on the wire; shells format them for display.

use nmp_planner::plan::{HintOrigin, InterestAttribution, RelayAttribution, UserConfiguredCategory};
use serde::{Deserialize, Serialize};

/// Cap for `author_pubkeys` in one `RelayConnectionReason`. The UI renders
/// "alice, bob, … +142 (150 total)" using `author_total` when the capped list
/// is shorter than the full set.
const AUTHOR_CAP: usize = 8;

/// One entry in the `reasons` list on a [`super::RelayDiagnosticsRow`].
///
/// `kind` is a stable machine tag; all other fields carry raw structured
/// payload — shells derive display strings from them (aim.md §4.5).
///
/// Structured payload per reason type:
/// - `author_pubkeys` / `author_total` for outbox (`"nip65"`) and interest
///   rows (capped at [`AUTHOR_CAP`], with exact total so the UI can render "+N").
/// - `kinds` for interest rows: raw kind numbers the shell formats (e.g.
///   `"kind:0, kind:1"`).  Non-empty for `"interest"` variants only.
/// - `source_event_id` for hint rows (the originating event id hex, when known).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct RelayConnectionReason {
    /// Stable machine tag: `"nip65"` | `"hint"` | `"account_read"` |
    /// `"account_write"` | `"indexer"` | `"app_relay"` | `"debug"` |
    /// `"bootstrap"` | `"blocked"` | `"interest"`.
    pub(crate) kind: String,
    /// Semantic hue key reusing the existing tone vocabulary:
    /// `"ok"` | `"warn"` | `"accent"` | `"muted"` | `"error"`.
    pub(crate) tone: String,
    /// Hex pubkeys (capped at [`AUTHOR_CAP`]). Non-empty for outbox and
    /// author-shaped interest reasons; empty for app_relay / hint / blocked.
    pub(crate) author_pubkeys: Vec<String>,
    /// Exact author total (>= `author_pubkeys.len()`). Zero when not applicable.
    pub(crate) author_total: u32,
    /// Raw kind numbers for interest reasons. Non-empty for `"interest"`
    /// variants only; empty for all other reason kinds.
    pub(crate) kinds: Vec<u32>,
    /// Hint origin event id (hex) when known; `None` for non-hint reasons.
    pub(crate) source_event_id: Option<String>,
}

/// Build the connection-reason list for one relay row.
///
/// `attr` is the [`RelayAttribution`] snapshot captured from
/// `SubscriptionLifecycle::current_plan_attribution()` before the blocked-relay
/// post-pass. `None` means no compile has run yet (empty reasons list).
///
/// Order: blocked (sentinel) → outbox → hints → app-relay sub-categories →
/// interest. The product spec says App relay → Outbox → Hint → Interest, but
/// the blocked sentinel must be first when present so the shell can surface the
/// icon/tone before checking `connection_tone`.
///
/// Capping and tone assignment happens here — the planner stays noun-free (D0).
/// Shells derive display labels from the raw `kind`, `author_total`, and `kinds`
/// fields; no English prose is emitted by this function.
pub(crate) fn build_reasons(
    attr: Option<&RelayAttribution>,
    is_blocked: bool,
) -> Vec<RelayConnectionReason> {
    let mut out = Vec::new();

    if is_blocked {
        out.push(RelayConnectionReason {
            kind: "blocked".to_string(),
            tone: "muted".to_string(),
            author_pubkeys: Vec::new(),
            author_total: 0,
            kinds: Vec::new(),
            source_event_id: None,
        });
    }

    let Some(attr) = attr else { return out };

    // NIP-65 outbox authors.
    let outbox_total = attr.outbox_authors.len();
    if outbox_total > 0 {
        let author_pubkeys: Vec<String> = attr
            .outbox_authors
            .iter()
            .take(AUTHOR_CAP)
            .cloned()
            .collect();
        out.push(RelayConnectionReason {
            kind: "nip65".to_string(),
            tone: "accent".to_string(),
            author_total: outbox_total as u32,
            author_pubkeys,
            kinds: Vec::new(),
            source_event_id: None,
        });
    }

    // Relay hints (Hint + Provenance + DM relay origins).
    if !attr.hints.is_empty() {
        // Surface the first event id we can find, if any.
        let source_event_id = attr.hints.iter().find_map(|h| match h {
            HintOrigin::EventTag { event_id } => Some(event_id.clone()),
            HintOrigin::Provenance { event_id } => Some(event_id.clone()),
            HintOrigin::UserConfigured => None,
        });
        out.push(RelayConnectionReason {
            kind: "hint".to_string(),
            tone: "warn".to_string(),
            author_pubkeys: Vec::new(),
            author_total: 0,
            kinds: Vec::new(),
            source_event_id,
        });
    }

    // User-configured sub-categories (App relay, Account read/write, etc.).
    for cat in &attr.user_configured {
        let kind = match cat {
            UserConfiguredCategory::AccountRead => "account_read",
            UserConfiguredCategory::AccountWrite => "account_write",
            UserConfiguredCategory::Indexer => "indexer",
            UserConfiguredCategory::AppRelay => "app_relay",
            UserConfiguredCategory::Debug => "debug",
            UserConfiguredCategory::Bootstrap => "bootstrap",
        };
        out.push(RelayConnectionReason {
            kind: kind.to_string(),
            tone: "ok".to_string(),
            author_pubkeys: Vec::new(),
            author_total: 0,
            kinds: Vec::new(),
            source_event_id: None,
        });
    }

    // Per-interest provenance (one reason per InterestAttribution entry).
    for ia in &attr.interests {
        let reason = interest_reason(ia);
        out.push(reason);
    }

    out
}

/// Build one `"interest"` reason from an [`InterestAttribution`] entry.
fn interest_reason(ia: &InterestAttribution) -> RelayConnectionReason {
    let total = ia.authors.len();
    let author_pubkeys: Vec<String> = ia.authors.iter().take(AUTHOR_CAP).cloned().collect();
    let mut kinds: Vec<u32> = ia.kinds.iter().copied().collect();
    kinds.sort_unstable();
    kinds.dedup();
    RelayConnectionReason {
        kind: "interest".to_string(),
        tone: "ok".to_string(),
        author_pubkeys,
        author_total: total as u32,
        kinds,
        source_event_id: None,
    }
}
