//! NUT-07 proof state check request/response types.

use serde::{Deserialize, Serialize};

/// State check request.
#[derive(Debug, Serialize)]
pub struct StateCheckRequest {
    #[serde(rename = "Ys")]
    pub ys: Vec<String>, // Y = hash_to_curve(secret), compressed hex
}

/// NUT-07 proof spend state. Typed for the same fail-closed reason as
/// [`super::mint_quote::MintQuoteState`] — an unrecognized state string is a
/// decode error, not a silently-ignored fourth state.
#[derive(Clone, Copy, Debug, Deserialize, Eq, PartialEq, Serialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ProofSpendState {
    Unspent,
    Spent,
    Pending,
}

#[derive(Debug, Deserialize)]
pub struct ProofState {
    #[serde(rename = "Y")]
    pub y: String,
    pub state: ProofSpendState,
    #[serde(default)]
    pub witness: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct StateCheckResponse {
    pub states: Vec<ProofState>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn proof_state_rejects_unknown_state() {
        let json = r#"{"Y":"02aa","state":"WEIRD"}"#;
        assert!(serde_json::from_str::<ProofState>(json).is_err());
    }
}
