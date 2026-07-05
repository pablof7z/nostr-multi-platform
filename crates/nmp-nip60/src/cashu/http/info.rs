//! `/v1/info` (NUT-06) — request builder + response parsing for a mint's raw
//! metadata (name, icon, description, ...). Mirrors `keyset.rs`'s shape
//! exactly: `build_*` constructs the wire request, `parse_*` decodes the raw
//! response into the typed [`MintInfoResponse`]. Pure — no I/O — so both the
//! native `ureq` transport (`super::super::client::MintClient`) and a future
//! browser transport can share this same request/response shape (see this
//! module's parent's doc comment, "the capability lane").
//!
//! # Not the fee source
//!
//! NUT-06's `/v1/info` response carries display metadata only — no fee data.
//! Per-unit `input_fee_ppk` stays sourced from `/v1/keysets` (NUT-02), via
//! [`super::keyset`]/`MintClient::get_keysets_with_fees` — this module never
//! duplicates that.

use super::{
    parse_json_response, MintHttpMethod, MintHttpOperation, MintHttpRequest, MintRawResponse,
};
use crate::cashu::types::MintInfoResponse;
use crate::error::Nip60Error;

#[must_use]
pub fn build_get_info_request() -> MintHttpRequest {
    MintHttpRequest {
        operation: MintHttpOperation::GetInfo,
        method: MintHttpMethod::Get,
        path: "/v1/info".to_string(),
        body: Vec::new(),
    }
}

pub fn parse_info_response(raw: &MintRawResponse) -> Result<MintInfoResponse, Nip60Error> {
    parse_json_response(raw, "mint info")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cashu::http::mint_http_support::ok;

    #[test]
    fn build_get_info_request_shape() {
        let req = build_get_info_request();
        assert_eq!(req.operation, MintHttpOperation::GetInfo);
        assert_eq!(req.method, MintHttpMethod::Get);
        assert_eq!(req.path, "/v1/info");
        assert!(req.body.is_empty());
    }

    #[test]
    fn parse_info_response_decodes_icon_url() {
        let raw = ok(br#"{
            "name": "Test Mint",
            "pubkey": "02deadbeef",
            "version": "Nutshell/0.15.0",
            "description": "a test mint",
            "icon_url": "https://mint.example/icon.png"
        }"#);
        let info = parse_info_response(&raw).expect("mint info decodes");
        assert_eq!(info.name.as_deref(), Some("Test Mint"));
        assert_eq!(info.pubkey, "02deadbeef");
        assert_eq!(
            info.icon_url.as_deref(),
            Some("https://mint.example/icon.png")
        );
    }

    #[test]
    fn parse_info_response_defaults_missing_icon_url() {
        // Real-world mints routinely omit `icon_url` entirely — must decode
        // to `None`, never fail closed on an absent optional field.
        let raw = ok(br#"{"pubkey":"02deadbeef"}"#);
        let info = parse_info_response(&raw).expect("mint info decodes without icon_url");
        assert_eq!(info.icon_url, None);
        assert_eq!(info.name, None);
    }

    #[test]
    fn parse_info_response_rejects_non_2xx() {
        let raw = MintRawResponse {
            status_code: 500,
            body: b"{\"code\":1,\"detail\":\"boom\"}".to_vec(),
        };
        let err = parse_info_response(&raw).unwrap_err();
        assert!(matches!(err, Nip60Error::MintProtocol(_)));
    }
}
