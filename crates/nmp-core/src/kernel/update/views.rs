use super::super::requests::external_id_from_key;
#[cfg(test)]
use super::super::TimelineItem;
use super::super::{truncate, AccountSummary, Kernel, ProfileCard, StoredEvent};
#[cfg(test)]
use super::helpers::parse_repost_inner;
use super::helpers::{hex64_to_bytes32, is_hex64_lower, nmp_store_to_kernel_stored};
use crate::substrate::ProfileView;

impl Kernel {
    /// Look up the `StoredEvent` that resolves an event-ref
    /// `primary_id`. Hex-64 keys (event id form) index `self.events`
    /// directly; coordinate keys (`kind:pubkey:d_tag`) scan
    /// `self.events.values()` for the matching addressable triple;
    /// `i:<external-id>` NIP-73 external refs (#1654) scan for the matching
    /// `["i", <external-id>]` tag.
    ///
    /// d-tags may legally contain `:` (rare but spec-allowed); the
    /// split is bounded to the first two colons so a d-tag like
    /// `"foo:bar"` round-trips correctly.
    pub(in crate::kernel) fn lookup_for_primary_id(&self, key: &str) -> Option<StoredEvent> {
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
        // NIP-73 external ref `i:<external-id>` (#1654): the referencing event
        // is identified by a `["i", <external-id>]` tag, not by id or
        // coordinate. The EventStore has no `#i` secondary index, so resolve it
        // by scanning the in-memory timeline cache (where the resolver's
        // one-shot `#i` fetch deposits the referencing event on arrival) — the
        // same scan-fallback shape the addressable arm uses. Newest-by-cache-
        // order wins on the (rare) multi-match.
        if let Some(external_id) = external_id_from_key(key) {
            return self
                .events
                .values()
                .find(|e| {
                    e.tags
                        .iter()
                        .any(|t| t.len() >= 2 && t[0] == "i" && t[1] == external_id)
                })
                .cloned();
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

    #[cfg(test)] // only called from kernel/tests.rs
    pub(in crate::kernel) fn timeline_item(&self, event: &StoredEvent) -> TimelineItem {
        let profile = self.profile_for_pubkey(&event.author);
        // aim.md §2: picture URL stays `Option<String>`. No identicon
        // placeholder is substituted in NMP; presentation layers choose
        // the missing-picture strategy.
        let author_picture_url = profile
            .as_ref()
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
            author_lnurl: profile.as_ref().and_then(|p| p.lnurl.clone()),
            // Author display name baked into the snapshot item so the renderer
            // has it without depending on the `refs.profile` claim
            // lifecycle. Empty string → `None` at this projection boundary
            // (aim.md §2), mirroring `mention_profiles_from_items`.
            author_display_name: profile
                .as_ref()
                .map(|p| p.display.clone())
                .filter(|d| !d.is_empty()),
            kind: event.kind,
            content: truncate(&event.content, 1_200),
            // NIP-18 kind:6: outer `content` is the stringified inner-event
            // JSON, so we must NOT use it directly as the preview — that
            // ships raw `{"id":"...` to the consumer. Derive the preview from
            // the already-extracted `repost_inner_content` (flat-map newlines,
            // truncate at 180 chars). It is the empty string when the inner
            // content is unavailable or empty (NIP-18 allows omitting it, or the
            // inner JSON was malformed — D1 best-effort). The kernel ships NO
            // display prose for the empty case (#1683, D7/D27 / aim.md §2): the
            // `is_repost` flag is on the wire, so presentation owns any "Repost"
            // label — it never lived in this raw preview. Non-repost path is
            // byte-identical to the old behaviour.
            content_preview: if is_repost {
                truncate(&repost_inner_content.trim().replace('\n', " "), 180)
            } else {
                truncate(&event.content.replace('\n', " "), 180)
            },
            // aim.md §2 — raw Unix seconds; the presentation layer
            // formats the relative-time label.
            created_at: event.created_at,
            relay_count: event.relay_count,
            relay_provenance: super::super::provenance::relay_urls_for_event(
                &*self.store,
                &event.id,
            ),
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
            .as_ref()
            .and_then(|p| p.picture_url.as_deref())
            .filter(|url| !url.is_empty())
            .map(str::to_owned);
        let display_name = profile
            .as_ref()
            .map(|profile| profile.display.clone())
            .filter(|display| !display.is_empty());
        ProfileCard {
            pubkey: pubkey.to_string(),
            display_name,
            name: profile.as_ref().and_then(|p| p.name.clone()),
            raw_display_name: profile.as_ref().and_then(|p| p.raw_display_name.clone()),
            display_name_camel: profile.as_ref().and_then(|p| p.display_name_camel.clone()),
            picture_url,
            banner: profile.as_ref().and_then(|p| p.banner.clone()),
            website: profile.as_ref().and_then(|p| p.website.clone()),
            nip05: profile
                .as_ref()
                .map(|profile| profile.nip05.clone())
                .unwrap_or_default(),
            about: profile.as_ref().map_or_else(
                || placeholder_about.to_string(),
                |profile| truncate(&profile.about.replace('\n', " "), 220),
            ),
            // NIP-57 — pre-extracted lightning address / LNURL from
            // kind:0 (lud16 preferred over lud06). `None` when no
            // kind:0 has arrived OR the metadata had no lnurl.
            lud16: profile.as_ref().and_then(|p| p.lud16.clone()),
            lud06: profile.as_ref().and_then(|p| p.lud06.clone()),
            lnurl: profile.as_ref().and_then(|p| p.lnurl.clone()),
        }
    }

    pub(crate) fn profile_for_pubkey(&self, pubkey: &str) -> Option<ProfileView> {
        // ADR-0057 PR 2 — profiles are capability-owned (`nmp_nip01::ProfileCache`
        // behind `Arc<dyn ProfileLookup>`). The cache uses interior mutability so
        // this read hands back an owned `ProfileView` (no borrow leaks out of the
        // lock). Single-mechanism (ADR-0045 Rev 2, #1193): locally-published
        // kind:0 profiles land in the same cache via the unified ingest chokepoint
        // (`verify_and_persist` → `Kind0Parser`), identical to the relay path — so
        // this read needs no overlay merge.
        self.profile_lookup().profile(pubkey)
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
}
