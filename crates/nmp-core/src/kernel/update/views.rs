use super::super::{
    truncate, AccountSummary, Kernel, MentionProfilePayload, Profile, ProfileCard, StoredEvent,
    TimelineItem,
};
use super::helpers::{hex64_to_bytes32, is_hex64_lower, nmp_store_to_kernel_stored, parse_repost_inner};

impl Kernel {
    /// Look up the `StoredEvent` that resolves a `claim_event`
    /// `primary_id`. Hex-64 keys (event id form) index `self.events`
    /// directly; coordinate keys (`kind:pubkey:d_tag`) scan
    /// `self.events.values()` for the matching addressable triple.
    ///
    /// d-tags may legally contain `:` (rare but spec-allowed); the
    /// split is bounded to the first two colons so a d-tag like
    /// `"foo:bar"` round-trips correctly.
    pub(super) fn lookup_for_primary_id(&self, key: &str) -> Option<StoredEvent> {
        // Try the in-memory timeline cache first (kind:1 / kind:6 are inserted
        // here by `ingest_timeline_event`). The addressable / unknown-kind
        // path below needs to query the EventStore which returns owned
        // values, so the function standardizes on owned `StoredEvent` for
        // both branches.
        if is_hex64_lower(key) {
            if let Some(e) = self.events.get(key) {
                return Some(e.clone());
            }
            // Other kinds (kind:30023 articles, kind:9802 highlights, ...)
            // are persisted via `verify_and_persist` into `self.store` but
            // NOT mirrored into `self.events`. Fall back to the EventStore
            // so the `claimed_events` projection surfaces ALL kinds.
            let id_bytes = hex64_to_bytes32(key)?;
            return self
                .store
                .get_by_id(&id_bytes)
                .ok()
                .flatten()
                .map(nmp_store_to_kernel_stored);
        }
        let mut parts = key.splitn(3, ':');
        let kind = parts.next().and_then(|s| s.parse::<u32>().ok())?;
        let pubkey = parts.next()?;
        let d_tag = parts.next()?;
        // Addressable lookup: try the EventStore's indexed
        // `(pubkey, kind, d_tag) → current addressable` path first; fall
        // back to scanning the in-memory cache for the (rare) case where an
        // addressable-kind event also landed in `self.events`.
        if let Some(pubkey_bytes) = hex64_to_bytes32(pubkey) {
            if let Ok(Some(e)) =
                self.store
                    .get_param_replaceable(&pubkey_bytes, kind, d_tag.as_bytes())
            {
                return Some(nmp_store_to_kernel_stored(e));
            }
        }
        self.events
            .values()
            .find(|e| {
                e.kind == kind
                    && e.author == pubkey
                    && e.tags
                        .iter()
                        .any(|t| t.len() >= 2 && t[0] == "d" && t[1] == d_tag)
            })
            .cloned()
    }

    pub(in crate::kernel) fn timeline_item(&self, event: &StoredEvent) -> TimelineItem {
        let profile = self.profile_for_pubkey(&event.author);
        // aim.md §2: picture URL stays `Option<String>`. No identicon
        // placeholder is substituted in NMP; presentation layers choose
        // the missing-picture strategy.
        let author_picture_url = profile
            .and_then(|p| p.picture_url.as_deref())
            .filter(|url| !url.is_empty())
            .map(str::to_owned);
        // NIP-18 kind:6: the repost's `content` field carries the
        // verbatim stringified inner event JSON. We resolve it once here
        // so the shell binds `nav_target_id` / `repost_inner_content`
        // verbatim and never touches the JSON.
        //
        // D1 best-effort: when `content` is empty or malformed JSON,
        // the shell-visible fallbacks (`event.id`, `""`) match prior
        // behaviour — the "Repost" badge alone communicates state.
        let is_repost = event.kind == 6;
        let (nav_target_id, repost_inner_content) = if is_repost {
            let (inner_id, inner_content) = parse_repost_inner(&event.content);
            (
                inner_id.unwrap_or_else(|| event.id.clone()),
                inner_content.unwrap_or_default(),
            )
        } else {
            (event.id.clone(), String::new())
        };
        TimelineItem {
            id: event.id.clone(),
            author_pubkey: event.author.clone(),
            author_picture_url,
            // NIP-57 — pre-extracted lightning address / LNURL from the
            // author's kind:0 (or `None` when no kind:0 has arrived or
            // it carried no lud16/lud06). Surfaced here so the shell zap
            // button toggles enabled/disabled without a separate profile
            // lookup. Rust decides zapability.
            author_lnurl: profile.and_then(|p| p.lnurl.clone()),
            // Author display name baked into the snapshot item so the renderer
            // has it without depending on the `claimed_profiles` claim
            // lifecycle. Empty string → `None` at this projection boundary
            // (aim.md §2), mirroring `mention_profiles_from_items`.
            author_display_name: profile.map(|p| p.display.clone()).filter(|d| !d.is_empty()),
            kind: event.kind,
            content: truncate(&event.content, 1_200),
            // NIP-18 kind:6: outer `content` is the stringified inner-event
            // JSON, so we must NOT use it directly as the preview — that
            // ships raw `{"id":"...` to the consumer. Instead derive the
            // preview from the already-extracted `repost_inner_content`
            // (flat-map newlines, truncate at 180 chars). Fall back to
            // "Repost" when the inner content is unavailable or empty —
            // this covers both the empty-outer-content case (NIP-18 allows
            // omitting it) and the malformed-JSON case (D1 best-effort).
            // Non-repost path is byte-identical to the old behaviour.
            content_preview: if is_repost {
                let inner = repost_inner_content.trim();
                if inner.is_empty() {
                    "Repost".to_string()
                } else {
                    truncate(&inner.replace('\n', " "), 180)
                }
            } else {
                truncate(&event.content.replace('\n', " "), 180)
            },
            // aim.md §2 — raw Unix seconds; the presentation layer
            // formats the relative-time label.
            created_at: event.created_at,
            relay_count: event.relay_count,
            is_repost,
            nav_target_id,
            repost_inner_content,
        }
    }

    pub(in crate::kernel) fn profile_card(&self) -> ProfileCard {
        match self.active_account.as_deref() {
            Some(pk) => self.profile_card_for(pk, "Waiting for kind:0 from indexer"),
            None => self.profile_card_for("", "Waiting for kind:0 from indexer"),
        }
    }

    pub(in crate::kernel) fn profile_card_for(
        &self,
        pubkey: &str,
        placeholder_about: &str,
    ) -> ProfileCard {
        let profile = self.profile_for_pubkey(pubkey);
        // aim.md §2 — picture URL stays `Option<String>` (no identicon
        // placeholder substituted in NMP).
        let picture_url = profile
            .and_then(|p| p.picture_url.as_deref())
            .filter(|url| !url.is_empty())
            .map(str::to_owned);
        let display_name = profile
            .map(|profile| profile.display.clone())
            .filter(|display| !display.is_empty());
        ProfileCard {
            pubkey: pubkey.to_string(),
            display_name,
            picture_url,
            nip05: profile
                .map(|profile| profile.nip05.clone())
                .unwrap_or_default(),
            about: profile.map_or_else(
                || placeholder_about.to_string(),
                |profile| truncate(&profile.about.replace('\n', " "), 220),
            ),
            // NIP-57 — pre-extracted lightning address / LNURL from
            // kind:0 (lud16 preferred over lud06). `None` when no
            // kind:0 has arrived OR the metadata had no lnurl.
            lnurl: profile.and_then(|p| p.lnurl.clone()),
        }
    }

    pub(super) fn profile_for_pubkey(&self, pubkey: &str) -> Option<&Profile> {
        // Single-mechanism (ADR-0045 Rev 2, #1193): the `local_profile_intents`
        // overlay was retired. Locally-published kind:0 profiles now land in
        // `self.profiles` via `verify_and_persist` + `ingest_profile` at publish
        // time (identical to the relay ingest arm), so this read needs no
        // overlay merge.
        self.profiles.get(pubkey)
    }

    // V-112 (ADR-0042): profile_action_for() deleted — it was called only from
    // the deleted author_view() projection builder. Follow/unfollow actions now
    // flow through the chirp nmp-app-chirp ActionModule seam directly.

    /// Returns the accounts list enriched with profile picture URLs and
    /// real display names from cached kind:0 metadata. The base
    /// `AccountSummary` (built in the identity layer) doesn't see profile
    /// data; we patch here. Per aim.md §2 the patched fields stay
    /// `Option<String>` — when kind:0 carries no display name or no
    /// picture, the field stays `None` and the presentation layer chooses
    /// its own fallback.
    pub(in crate::kernel) fn accounts_enriched(&self) -> Vec<AccountSummary> {
        let (accounts, _) = self.account_snapshot();
        accounts
            .iter()
            .cloned()
            .map(|mut acc| {
                if let Some(profile) = self.profile_for_pubkey(&acc.id) {
                    let real_picture = profile.picture_url.as_deref().filter(|url| !url.is_empty());
                    acc.picture_url = real_picture.map(str::to_owned);
                    if !profile.display.is_empty() {
                        acc.display_name = Some(profile.display.clone());
                    }
                }
                acc
            })
            .collect()
    }

    // V-112 (ADR-0042): author_view(), author_items(), thread_view(),
    // thread_items(), thread_root_id() deleted. View state and item lists now
    // live in the per-app FlatFeed registered by nmp_app_chirp_open_author_feed
    // / nmp_app_chirp_open_thread_feed.

    /// Build the `mention_profiles` projection from a slice of timeline
    /// items. Maps `author_pubkey -> MentionProfilePayload` joining
    /// against the kind:0 profile cache. First writer wins on collision
    /// (mirroring the Swift `Dictionary(uniquingKeysWith:)` it replaces).
    /// Per aim.md §2, every payload field that depends on kind:0 is
    /// `Option<String>` — `None` when no kind:0 has arrived for this
    /// author.
    pub(in crate::kernel) fn mention_profiles_from_items(
        &self,
        items: &[TimelineItem],
    ) -> std::collections::HashMap<String, MentionProfilePayload> {
        let mut out: std::collections::HashMap<String, MentionProfilePayload> =
            std::collections::HashMap::new();
        for item in items {
            out.entry(item.author_pubkey.clone()).or_insert_with(|| {
                let profile = self.profile_for_pubkey(&item.author_pubkey);
                let display_name = profile.map(|p| p.display.clone()).filter(|d| !d.is_empty());
                let picture_url = profile
                    .and_then(|p| p.picture_url.as_deref())
                    .filter(|url| !url.is_empty())
                    .map(str::to_owned);
                MentionProfilePayload {
                    pubkey: item.author_pubkey.clone(),
                    display_name,
                    picture_url,
                }
            });
        }
        out
    }
}
