//! Construction of NIP-77/NIP-01 client messages via the upstream `nostr` crate.

use std::borrow::Cow;

use nostr::{ClientMessage, EventId, Filter, JsonUtil as _, SubscriptionId};
use serde_json::json;

use crate::codec::hex_encode;

pub(crate) fn req_text(sub_id: &str, filter_json: &str) -> String {
    match serde_json::from_str::<Filter>(filter_json) {
        Ok(filter) => ClientMessage::req(subscription_id(sub_id), filter).as_json(),
        Err(_) => {
            let filter = serde_json::from_str(filter_json).unwrap_or_else(|_| json!({}));
            json!(["REQ", sub_id, filter]).to_string()
        }
    }
}

pub(crate) fn ids_req_text(sub_id: &str, ids: &[[u8; 32]]) -> String {
    let ids = ids.iter().copied().map(EventId::from_byte_array);
    let filter = Filter::new().ids(ids);
    ClientMessage::req(subscription_id(sub_id), filter).as_json()
}

pub(crate) fn neg_open_text(sub_id: &str, filter: Filter, msg: &[u8]) -> String {
    ClientMessage::neg_open(subscription_id(sub_id), filter, hex_encode(msg)).as_json()
}

pub(crate) fn neg_msg_text(sub_id: &str, msg: &[u8]) -> String {
    ClientMessage::NegMsg {
        subscription_id: Cow::Owned(subscription_id(sub_id)),
        message: Cow::Owned(hex_encode(msg)),
    }
    .as_json()
}

pub(crate) fn neg_close_text(sub_id: &str) -> String {
    ClientMessage::NegClose {
        subscription_id: Cow::Owned(subscription_id(sub_id)),
    }
    .as_json()
}

fn subscription_id(sub_id: &str) -> SubscriptionId {
    SubscriptionId::new(sub_id.to_string())
}

#[cfg(test)]
mod tests {
    use nostr::{ClientMessage, JsonUtil as _};

    use super::*;

    #[test]
    fn neg_open_uses_upstream_nostr_message_shape() {
        let filter = Filter::new();
        let text = neg_open_text("s", filter, &[0x60, 0xaa]);
        let parsed = ClientMessage::from_json(&text).unwrap();
        assert!(matches!(parsed, ClientMessage::NegOpen { .. }));
        assert!(text.contains("\"60aa\""));
    }
}
