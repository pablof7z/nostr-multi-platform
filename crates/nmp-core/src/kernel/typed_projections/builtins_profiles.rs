//! Wave C profile/event-cluster slice of [`Kernel::builtin_typed_projections`].
//!
//! The four map-shaped built-ins here (`mention_profiles` / `claimed_profiles` /
//! `claimed_events` / `resolved_profiles`) flatten a `pubkey`/`primary_id`-keyed
//! map to a key-sorted `[{key, value}]` vector (FlatBuffers has no map type —
//! the #1028/#1029 + nip29/zaps convention). Their DTO→Row mappings must be
//! inlined where the `pub(super)`/`pub(crate)` DTO types (`MentionProfilePayload`,
//! `ProfileCard`, `ClaimedEventDto`) are reachable — i.e. in a `kernel::`
//! descendant — but kept under the same owner and out of `mod.rs` so that file
//! stays under the LOC ceiling. Each Model is built from the SAME accessor the
//! generic JSON projection in
//! [`snapshot_projections_with_publish_cluster`](super::super::Kernel::snapshot_projections_with_publish_cluster)
//! reads, in the same tick, so the typed and JSON wire forms cannot diverge:
//!
//! - `mention_profiles`  ← `mention_profiles()`  (HashMap — sorted by key here).
//! - `claimed_profiles`  ← `claimed_profiles()`  (BTreeMap — already sorted).
//! - `claimed_events`    ← `claimed_events()`    (BTreeMap — already sorted).
//! - `resolved_profiles` ← `resolved_profiles()` (BTreeMap — already sorted).
//!
//! All four are UNCONDITIONAL: their generic JSON keys are always inserted
//! (`{}` when empty, D1), so each typed entry is always pushed too.

use super::{
    encode_claimed_events, encode_claimed_profiles, encode_mention_profiles,
    encode_resolved_profiles, ClaimedEventRow, ClaimedEventsModel, ClaimedProfilesModel,
    MentionProfileRow, MentionProfilesModel, ProfileCardModel, ResolvedProfilesModel,
    CLAIMED_EVENTS_FILE_IDENTIFIER, CLAIMED_EVENTS_SCHEMA_ID, CLAIMED_EVENTS_SCHEMA_VERSION,
    CLAIMED_PROFILES_FILE_IDENTIFIER, CLAIMED_PROFILES_SCHEMA_ID, CLAIMED_PROFILES_SCHEMA_VERSION,
    MENTION_PROFILES_FILE_IDENTIFIER, MENTION_PROFILES_SCHEMA_ID, MENTION_PROFILES_SCHEMA_VERSION,
    RESOLVED_PROFILES_FILE_IDENTIFIER, RESOLVED_PROFILES_SCHEMA_ID,
    RESOLVED_PROFILES_SCHEMA_VERSION,
};
use crate::update_envelope::TypedProjectionData;

/// Map one kernel `ProfileCard` DTO onto the shared [`ProfileCardModel`]. The DTO
/// type is `pub(super)` in `kernel::types`, so it is bound by inference (never
/// named here). Mirrors `builtins_views::profile_card_model`.
fn profile_card_model(card: &super::super::ProfileCard) -> ProfileCardModel {
    ProfileCardModel {
        pubkey: card.pubkey.clone(),
        // ADR-0032 / V-115: `npub` removed from ProfileCard; deprecated in
        // FlatBuffers schema. Shells encode bech32 themselves.
        npub: String::new(),
        display_name: card.display_name.clone(),
        picture_url: card.picture_url.clone(),
        nip05: card.nip05.clone(),
        about: card.about.clone(),
        lnurl: card.lnurl.clone(),
    }
}

impl super::super::Kernel {
    /// Encode the Wave C profile/event-cluster (Tier-2) built-ins as typed
    /// FlatBuffer sidecar entries, in `mention_profiles` → `claimed_profiles` →
    /// `claimed_events` → `resolved_profiles` order. All four are unconditional
    /// (their JSON keys are always present). Called by
    /// [`builtin_typed_projections`](super::super::Kernel::builtin_typed_projections);
    /// see that method's doc for the mechanism.
    pub(in crate::kernel) fn profiles_cluster_typed_projections(&self) -> Vec<TypedProjectionData> {
        let mut out = Vec::with_capacity(4);

        // `mention_profiles` — encoded from the SAME `mention_profiles()` map the
        // JSON path serialises. It is a `HashMap`, so sort the flattened entries
        // by key to match the BTree-ordered serde JSON map.
        let mut mention_entries: Vec<(String, MentionProfileRow)> = self
            .mention_profiles()
            .into_iter()
            .map(|(key, payload)| {
                (
                    key,
                    MentionProfileRow {
                        pubkey: payload.pubkey,
                        display_name: payload.display_name,
                        picture_url: payload.picture_url,
                    },
                )
            })
            .collect();
        mention_entries.sort_by(|a, b| a.0.cmp(&b.0));
        out.push(TypedProjectionData {
            key: MENTION_PROFILES_SCHEMA_ID.to_string(),
            schema_id: MENTION_PROFILES_SCHEMA_ID.to_string(),
            schema_version: MENTION_PROFILES_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(MENTION_PROFILES_FILE_IDENTIFIER).into_owned(),
            payload: encode_mention_profiles(&MentionProfilesModel {
                entries: mention_entries,
            }),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        // `claimed_profiles` — encoded from the SAME `claimed_profiles()` BTreeMap
        // the JSON path serialises (already key-sorted).
        let claimed_profiles = ClaimedProfilesModel {
            entries: self
                .claimed_profiles()
                .iter()
                .map(|(key, card)| (key.clone(), profile_card_model(card)))
                .collect(),
        };
        out.push(TypedProjectionData {
            key: CLAIMED_PROFILES_SCHEMA_ID.to_string(),
            schema_id: CLAIMED_PROFILES_SCHEMA_ID.to_string(),
            schema_version: CLAIMED_PROFILES_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(CLAIMED_PROFILES_FILE_IDENTIFIER).into_owned(),
            payload: encode_claimed_profiles(&claimed_profiles),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        // `claimed_events` — encoded from the SAME `claimed_events()` BTreeMap the
        // JSON path serialises (already key-sorted). `tags: Vec<Vec<String>>` is
        // carried verbatim into the `[TagRow]` shape.
        let claimed_events = ClaimedEventsModel {
            entries: self
                .claimed_events()
                .iter()
                .map(|(key, dto)| {
                    (
                        key.clone(),
                        ClaimedEventRow {
                            primary_id: dto.primary_id.clone(),
                            id: dto.id.clone(),
                            author_pubkey: dto.author_pubkey.clone(),
                            author_display_name: dto.author_display_name.clone(),
                            author_picture_url: dto.author_picture_url.clone(),
                            kind: dto.kind,
                            created_at: dto.created_at,
                            tags: dto.tags.clone(),
                            content: dto.content.clone(),
                        },
                    )
                })
                .collect(),
        };
        out.push(TypedProjectionData {
            key: CLAIMED_EVENTS_SCHEMA_ID.to_string(),
            schema_id: CLAIMED_EVENTS_SCHEMA_ID.to_string(),
            schema_version: CLAIMED_EVENTS_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(CLAIMED_EVENTS_FILE_IDENTIFIER).into_owned(),
            payload: encode_claimed_events(&claimed_events),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        // `resolved_profiles` — encoded from the SAME `resolved_profiles()`
        // BTreeMap the JSON path serialises (already key-sorted).
        let resolved_profiles = ResolvedProfilesModel {
            entries: self
                .resolved_profiles()
                .iter()
                .map(|(key, card)| (key.clone(), profile_card_model(card)))
                .collect(),
        };
        out.push(TypedProjectionData {
            key: RESOLVED_PROFILES_SCHEMA_ID.to_string(),
            schema_id: RESOLVED_PROFILES_SCHEMA_ID.to_string(),
            schema_version: RESOLVED_PROFILES_SCHEMA_VERSION,
            file_identifier: String::from_utf8_lossy(RESOLVED_PROFILES_FILE_IDENTIFIER)
                .into_owned(),
            payload: encode_resolved_profiles(&resolved_profiles),
            // ADR-0055 Rung 2: rev + state stamped by make_update after emit.
            ..Default::default()
        });

        out
    }
}
