use nostr::{JsonUtil, RelayUrl};
use serde_json::{json, Value};

use crate::projection::state::{hex_encode, InnerHandle};

/// NIP-59 gift-wrap each kind:444 welcome rumor for its invitee and publish
/// the resulting signed kind:1059 internally.
///
/// Recipient pairing: `welcome_rumors[i]` pairs with `kp_events[i].pubkey`
/// because the KP author is the invitee MDK built that welcome for. Length
/// divergence wraps every pairable rumor and skips the tail.
pub(super) fn wrap_and_publish_welcomes(
    h: &InnerHandle<'_>,
    group_relays: &[RelayUrl],
    kp_events: &[nostr::Event],
    rumors: &[nostr::UnsignedEvent],
) -> Result<Vec<String>, String> {
    let mut out = Vec::with_capacity(rumors.len());
    for (i, rumor) in rumors.iter().enumerate() {
        let Some(kp) = kp_events.get(i) else {
            break;
        };
        let receiver = kp.pubkey;
        let wrapped = h
            .service()
            .wrap_welcome(&receiver, rumor.clone())
            .map_err(|e| e.to_string())?;
        h.publish_explicit(&wrapped, group_relays);
        out.push(wrapped.as_json());
    }
    Ok(out)
}

pub(super) fn accept_welcome(
    h: &mut InnerHandle<'_>,
    welcome_id_hex: &str,
) -> Result<Value, String> {
    let wid = welcome_id_hex.to_string();
    let Some(gift) = h.take_welcome_gift_wrap(&wid) else {
        return Err(format!("no pending welcome `{wid}`"));
    };
    let (welcome, _sender) = match h.service().unwrap_and_process_welcome(&gift) {
        Ok(w) => w,
        Err(e) => {
            restore(h, &wid, gift);
            return Err(e.to_string());
        }
    };
    if let Err(e) = h.service().accept_welcome(&welcome) {
        restore(h, &wid, gift);
        return Err(e.to_string());
    }
    let group_id_hex = hex_encode(welcome.mls_group_id.as_slice());
    h.cache_group_relays(
        group_id_hex.clone(),
        welcome.group_relays.iter().cloned().collect(),
    );
    let self_update = match h.service().self_update(&welcome.mls_group_id) {
        Ok(p) => {
            let ev = p.evolution_event.as_json();
            h.publish_group_pinned(&group_id_hex, &p.evolution_event);
            p.commit().map_err(|e| e.to_string())?;
            Some(ev)
        }
        Err(_) => None,
    };
    Ok(json!({
        "group_id_hex": group_id_hex,
        "post_join_self_update_event": self_update,
    }))
}

pub(super) fn decline_welcome(
    h: &mut InnerHandle<'_>,
    welcome_id_hex: &str,
) -> Result<Value, String> {
    let wid = welcome_id_hex.to_string();
    let Some(gift) = h.take_welcome_gift_wrap(&wid) else {
        return Err(format!("no pending welcome `{wid}`"));
    };
    let (welcome, _sender) = match h.service().unwrap_and_process_welcome(&gift) {
        Ok(w) => w,
        Err(e) => {
            restore(h, &wid, gift);
            return Err(e.to_string());
        }
    };
    h.service()
        .decline_welcome(&welcome)
        .map_err(|e| e.to_string())?;
    Ok(json!({ "declined": wid }))
}

fn restore(h: &mut InnerHandle<'_>, wid: &str, gift: nostr::Event) {
    let (name, npub) = h
        .service()
        .unwrap_and_process_welcome(&gift)
        .map(|(w, s)| (w.group_name.clone(), s.to_hex()))
        .unwrap_or_default();
    h.restore_welcome(wid.to_string(), gift, name, npub);
}
