use std::collections::BTreeMap;

use serde_json::{json, Map, Value};

use nmp_content::wire::{
    decode_claimed_event_embeds, EMBED_SIDECAR_PROJECTION_KEY, EMBED_SIDECAR_SCHEMA_ID,
};
use nmp_core::refs::{RefProfileStore, REFS_PROFILE_KEY};
use nmp_core::{
    decode_snapshot_envelope, decode_snapshot_typed_projections,
    display::{short_npub, to_npub},
    typed_projections::{
        decode_accounts, decode_claimed_events, decode_relay_role_options, decode_signer_state,
        AccountsModel, ClaimedEventRow, ClaimedEventsModel, ProfileCardModel,
        RelayRoleOptionsModel, SignerStateModel, ACCOUNTS_SCHEMA_ID, CLAIMED_EVENTS_SCHEMA_ID,
        RELAY_ROLE_OPTIONS_SCHEMA_ID, SIGNER_STATE_SCHEMA_ID,
    },
    TypedProjectionData,
};

/// The JSON projection key the native shells read for resolved profiles.
/// ADR-0063 (#1671): the map is now SOURCED from the `refs.profile` row-delta
/// projection (merged into a stateful [`RefProfileStore`]); the retired
/// `resolved_profiles` whole-map projection is no longer decoded.
const PROFILES_JSON_KEY: &str = "resolved_profiles";

pub(crate) fn snapshot_json_from_update_frame(
    bytes: &[u8],
    ref_profiles: &mut RefProfileStore,
) -> Result<String, String> {
    let envelope = decode_snapshot_envelope(bytes).map_err(|err| err.to_string())?;
    let typed = decode_snapshot_typed_projections(bytes).map_err(|err| err.to_string())?;

    // ADR-0063 (#1671): merge this frame's `refs.profile` row-delta batch into
    // the stateful store (the sole app-side mirror, D4), then materialise the
    // current full set. A malformed sidecar is a fail-closed no-op inside the
    // store (prior rows retained).
    if let Some(entry) = find_projection(&typed, REFS_PROFILE_KEY)? {
        ref_profiles.apply_sidecar(&entry.payload, envelope.session_id, envelope.snapshot_epoch);
    }

    let mut projections = Map::new();
    projections.insert(
        PROFILES_JSON_KEY.to_string(),
        refs_profiles_json(&ref_profiles.profiles()),
    );
    projections.insert(
        CLAIMED_EVENTS_SCHEMA_ID.to_string(),
        claimed_events_json(find_projection(&typed, CLAIMED_EVENTS_SCHEMA_ID)?)?,
    );
    projections.insert(
        ACCOUNTS_SCHEMA_ID.to_string(),
        accounts_json(find_projection(&typed, ACCOUNTS_SCHEMA_ID)?)?,
    );
    projections.insert(
        RELAY_ROLE_OPTIONS_SCHEMA_ID.to_string(),
        relay_role_options_json(find_projection(&typed, RELAY_ROLE_OPTIONS_SCHEMA_ID)?)?,
    );
    projections.insert(
        EMBED_SIDECAR_PROJECTION_KEY.to_string(),
        claimed_event_embeds_json(find_projection(&typed, EMBED_SIDECAR_SCHEMA_ID)?)?,
    );
    if let Some(entry) = find_projection(&typed, SIGNER_STATE_SCHEMA_ID)? {
        projections.insert(
            SIGNER_STATE_SCHEMA_ID.to_string(),
            signer_state_json(&decode_signer_state(&entry.payload)?),
        );
    }

    serde_json::to_string(&json!({
        "schema_version": 1,
        "running": envelope.running,
        "projections": projections,
    }))
    .map_err(|err| format!("snapshot json encode failed: {err}"))
}

fn find_projection<'a>(
    typed: &'a [TypedProjectionData],
    schema_id: &str,
) -> Result<Option<&'a TypedProjectionData>, String> {
    let matches: Vec<&TypedProjectionData> = typed
        .iter()
        .filter(|entry| entry.schema_id == schema_id || entry.key == schema_id)
        .collect();
    match matches.as_slice() {
        [] => Ok(None),
        [entry] => Ok(Some(*entry)),
        _ => Err(format!("duplicate typed projection for {schema_id}")),
    }
}

/// Render the materialised `refs.profile` set (ADR-0063 #1671 — the resolve_ref
/// output, merged in the caller's [`RefProfileStore`]) as the native shells'
/// resolved-profiles JSON map. Replaces the retired `resolved_profiles`
/// whole-map projection decode.
fn refs_profiles_json(profiles: &BTreeMap<String, ProfileCardModel>) -> Value {
    let mut out = Map::with_capacity(profiles.len());
    for (key, card) in profiles {
        let pubkey = if card.pubkey.is_empty() {
            key.as_str()
        } else {
            card.pubkey.as_str()
        };
        out.insert(key.clone(), profile_card_json(card, pubkey));
    }
    Value::Object(out)
}

fn profile_card_json(card: &ProfileCardModel, pubkey: &str) -> Value {
    let npub = to_npub(pubkey);
    json!({
        "pubkey": pubkey,
        "display_name": card.display_name,
        "about": empty_string_as_null(&card.about),
        "picture_url": card.picture_url,
        "nip05": empty_string_as_null(&card.nip05),
        "lnurl": card.lnurl,
        "npub": npub,
        "npub_short": short_npub(pubkey),
    })
}

fn claimed_events_json(entry: Option<&TypedProjectionData>) -> Result<Value, String> {
    let Some(entry) = entry else {
        return Ok(Value::Object(Map::new()));
    };
    let model = decode_claimed_events(&entry.payload)?;
    Ok(claimed_events_model_json(&model))
}

fn claimed_events_model_json(model: &ClaimedEventsModel) -> Value {
    let mut out = Map::with_capacity(model.entries.len());
    for (key, row) in &model.entries {
        out.insert(key.clone(), claimed_event_row_json(row));
    }
    Value::Object(out)
}

fn claimed_event_row_json(row: &ClaimedEventRow) -> Value {
    json!({
        "primary_id": row.primary_id,
        "id": row.id,
        "author_pubkey": row.author_pubkey,
        "author_display_name": row.author_display_name,
        "author_picture_url": row.author_picture_url,
        "kind": row.kind,
        "created_at": row.created_at,
        "tags": row.tags,
        "content": row.content,
    })
}

fn accounts_json(entry: Option<&TypedProjectionData>) -> Result<Value, String> {
    let Some(entry) = entry else {
        return Ok(Value::Array(Vec::new()));
    };
    let model = decode_accounts(&entry.payload)?;
    Ok(accounts_model_json(&model))
}

/// Shell-side signer label derived from the raw `signer_kind` wire token. The
/// kernel no longer ships a pre-rendered `signer_label` (#1712, D7/D27 —
/// presentation artifact); the gallery shell derives it.
fn signer_label_for_kind(kind: &str) -> &str {
    match kind {
        "local" => "Local key",
        "nip46" => "NIP-46",
        other => other,
    }
}

fn accounts_model_json(model: &AccountsModel) -> Value {
    let accounts = model
        .accounts
        .iter()
        .map(|row| {
            json!({
                "id": row.id,
                "pubkey": row.id,
                "npub": row.npub,
                "display_name": row.display_name,
                "signer_kind": row.signer_kind,
                "status": row.status,
                "signer_label": signer_label_for_kind(&row.signer_kind),
                "signer_is_remote": row.signer_is_remote,
                "is_active": row.is_active,
                "active": row.is_active,
                "picture_url": row.picture_url,
            })
        })
        .collect();
    Value::Array(accounts)
}

fn relay_role_options_json(entry: Option<&TypedProjectionData>) -> Result<Value, String> {
    let Some(entry) = entry else {
        return Ok(Value::Array(Vec::new()));
    };
    let model = decode_relay_role_options(&entry.payload)?;
    Ok(relay_role_options_model_json(&model))
}

fn relay_role_options_model_json(model: &RelayRoleOptionsModel) -> Value {
    Value::Array(
        model
            .options
            .iter()
            .map(|option| {
                // `label` removed from the wire (#1678, D7) — shells map
                // value→label themselves.
                json!({
                    "value": option.value,
                    "tint": option.tint,
                    "is_default": option.is_default,
                })
            })
            .collect(),
    )
}

fn claimed_event_embeds_json(entry: Option<&TypedProjectionData>) -> Result<Value, String> {
    let Some(entry) = entry else {
        return Ok(Value::Object(Map::new()));
    };
    let decoded = decode_claimed_event_embeds(&entry.payload)?;
    let mut out = Map::with_capacity(decoded.len());
    for (primary_id, env) in &decoded {
        out.insert(
            primary_id.clone(),
            json!({
                "uri": env.uri,
                "primary_id": env.primary_id,
                "depth": env.render_context.depth,
                "max_depth": env.render_context.max_depth,
                "collapsed": env.collapsed,
                "collapse_reason": env.collapse_reason,
                "projection": env.projection,
            }),
        );
    }
    Ok(Value::Object(out))
}

fn signer_state_json(model: &SignerStateModel) -> Value {
    json!({
        "signer_kind": model.signer_kind,
        "state": model.state,
        "reason": model.reason,
        "is_ready": model.is_ready,
        "is_awaiting_approval": model.is_awaiting_approval,
        "is_reconnecting": model.is_reconnecting,
        "is_unavailable": model.is_unavailable,
        "is_failed": model.is_failed,
    })
}

fn empty_string_as_null(value: &str) -> Value {
    if value.is_empty() {
        Value::Null
    } else {
        Value::String(value.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use nmp_core::refs::{encode_ref_row_delta_batch, RefRow, RefRowDeltaBatch};
    use nmp_core::typed_projections::encode_profile;
    use nmp_core::{encode_snapshot_frame, SnapshotEnvelope, TypedProjectionData};

    #[test]
    fn empty_typed_snapshot_decodes_to_gallery_shape() {
        let frame = encode_snapshot_frame(
            &SnapshotEnvelope {
                running: true,
                update_kind: "ViewBatch".to_string(),
                ..Default::default()
            },
            &[],
        );

        let mut store = RefProfileStore::new();
        let value: Value =
            serde_json::from_str(&snapshot_json_from_update_frame(&frame, &mut store).expect("decode"))
                .expect("json");

        assert_eq!(value["schema_version"], 1);
        assert_eq!(value["running"], true);
        assert_eq!(value["projections"]["resolved_profiles"], json!({}));
        assert_eq!(value["projections"]["claimed_events"], json!({}));
        assert_eq!(value["projections"]["accounts"], json!([]));
        assert_eq!(value["projections"]["relay_role_options"], json!([]));
        assert_eq!(value["projections"]["claimed_event_embeds"], json!({}));
        assert!(value["projections"].get("signer_state").is_none());
    }

    #[test]
    fn refs_profile_row_delta_surfaces_in_resolved_profiles_json() {
        // ADR-0063 (#1671): a `refs.profile` baseline row carrying a fresh KPRF
        // card must merge into the store and surface under the
        // `resolved_profiles` JSON key the native shells read.
        let pubkey = "1111111111111111111111111111111111111111111111111111111111111111";
        let card_payload = encode_profile(&ProfileCardModel {
            pubkey: pubkey.to_string(),
            display_name: Some("Refs Name".to_string()),
            picture_url: Some("https://example.com/refs.png".to_string()),
            ..Default::default()
        });
        let batch = encode_ref_row_delta_batch(&RefRowDeltaBatch {
            namespace: "profile".to_string(),
            baseline: true,
            rows: vec![RefRow::changed(pubkey, 1, card_payload)],
        });
        let frame = encode_snapshot_frame(
            &SnapshotEnvelope {
                running: true,
                update_kind: "ViewBatch".to_string(),
                session_id: 1,
                ..Default::default()
            },
            &[TypedProjectionData {
                key: REFS_PROFILE_KEY.to_string(),
                schema_id: REFS_PROFILE_KEY.to_string(),
                schema_version: 1,
                file_identifier: String::new(),
                payload: batch,
                ..Default::default()
            }],
        );

        let mut store = RefProfileStore::new();
        let value: Value = serde_json::from_str(
            &snapshot_json_from_update_frame(&frame, &mut store).expect("decode"),
        )
        .expect("json");

        let entry = &value["projections"]["resolved_profiles"][pubkey];
        assert_eq!(entry["display_name"], "Refs Name");
        assert_eq!(entry["picture_url"], "https://example.com/refs.png");
        assert_eq!(entry["pubkey"], pubkey);
    }

    #[test]
    fn refs_profile_clear_drops_row_from_resolved_profiles_json() {
        // ADR-0063 (#1671): snapshot_json materialises the FULL current
        // RefProfileStore set each frame. A subsequent `refs.profile` CLEAR
        // (release-on-scroll-off) must DROP the row from `resolved_profiles` —
        // the materialised set is the sole source of truth (D4), no stale row.
        let pubkey = "2222222222222222222222222222222222222222222222222222222222222222";
        let card_payload = encode_profile(&ProfileCardModel {
            pubkey: pubkey.to_string(),
            display_name: Some("Soon Gone".to_string()),
            ..Default::default()
        });

        let mut store = RefProfileStore::new();

        // Frame 1: baseline carrying the resolved card — present.
        let add_frame = encode_snapshot_frame(
            &SnapshotEnvelope {
                running: true,
                update_kind: "ViewBatch".to_string(),
                session_id: 1,
                ..Default::default()
            },
            &[TypedProjectionData {
                key: REFS_PROFILE_KEY.to_string(),
                schema_id: REFS_PROFILE_KEY.to_string(),
                schema_version: 1,
                file_identifier: String::new(),
                payload: encode_ref_row_delta_batch(&RefRowDeltaBatch {
                    namespace: "profile".to_string(),
                    baseline: true,
                    rows: vec![RefRow::changed(pubkey, 1, card_payload)],
                }),
                ..Default::default()
            }],
        );
        let added: Value = serde_json::from_str(
            &snapshot_json_from_update_frame(&add_frame, &mut store).expect("decode add"),
        )
        .expect("json");
        assert_eq!(
            added["projections"]["resolved_profiles"][pubkey]["display_name"],
            "Soon Gone",
            "row must be present after the baseline add"
        );

        // Frame 2: a CLEAR row-delta (release) for the same key — the row must
        // be GONE from the materialised set, not retained as stale.
        let clear_frame = encode_snapshot_frame(
            &SnapshotEnvelope {
                running: true,
                update_kind: "ViewBatch".to_string(),
                session_id: 1,
                ..Default::default()
            },
            &[TypedProjectionData {
                key: REFS_PROFILE_KEY.to_string(),
                schema_id: REFS_PROFILE_KEY.to_string(),
                schema_version: 1,
                file_identifier: String::new(),
                payload: encode_ref_row_delta_batch(&RefRowDeltaBatch {
                    namespace: "profile".to_string(),
                    baseline: false,
                    rows: vec![RefRow::cleared(pubkey, 2)],
                }),
                ..Default::default()
            }],
        );
        let cleared: Value = serde_json::from_str(
            &snapshot_json_from_update_frame(&clear_frame, &mut store).expect("decode clear"),
        )
        .expect("json");
        assert!(
            cleared["projections"]["resolved_profiles"]
                .get(pubkey)
                .is_none(),
            "a refs.profile CLEAR must drop the row from resolved_profiles; got {:?}",
            cleared["projections"]["resolved_profiles"]
        );
    }

    #[test]
    fn profile_card_json_adds_gallery_display_fields() {
        let card = ProfileCardModel {
            pubkey: "0000000000000000000000000000000000000000000000000000000000000000".to_string(),
            display_name: Some("Alice".to_string()),
            about: String::new(),
            picture_url: None,
            nip05: String::new(),
            lnurl: None,
            ..Default::default()
        };

        let value = profile_card_json(&card, &card.pubkey);

        assert_eq!(value["pubkey"], card.pubkey);
        assert_eq!(value["display_name"], "Alice");
        assert!(value["npub"].as_str().unwrap_or("").starts_with("npub1"));
        assert!(value["npub_short"]
            .as_str()
            .unwrap_or("")
            .starts_with("npub1"));
        assert!(value["about"].is_null());
        assert!(value["nip05"].is_null());
    }
}
