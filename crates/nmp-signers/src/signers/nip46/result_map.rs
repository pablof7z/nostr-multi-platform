//! Typed response mapping for NIP-46 extension methods.

use std::sync::mpsc;

use serde::de::DeserializeOwned;

use nmp_signer_iface::{SignerError, SignerOp};

/// Map a raw NIP-46 string response into a typed signer result, normalising
/// signer-side errors before the caller observes them.
pub fn map_response_with_error<T, F, E>(
    raw_op: SignerOp<String>,
    parse: F,
    map_error: E,
) -> SignerOp<T>
where
    T: Send + 'static,
    F: FnOnce(String) -> Result<T, SignerError> + Send + 'static,
    E: FnOnce(SignerError) -> SignerError + Send + 'static,
{
    match raw_op {
        SignerOp::Ready(Ok(s)) => SignerOp::Ready(parse(s)),
        SignerOp::Ready(Err(e)) => SignerOp::Ready(Err(map_error(e))),
        SignerOp::Pending(rx) => {
            let (tx, out_rx) = mpsc::channel();
            std::thread::spawn(move || {
                let result = match rx.recv() {
                    Ok(Ok(s)) => parse(s),
                    Ok(Err(e)) => Err(map_error(e)),
                    Err(_) => Err(SignerError::Backend(
                        "nip46 response channel disconnected".to_string(),
                    )),
                };
                let _ = tx.send(result);
            });
            SignerOp::Pending(out_rx)
        }
    }
}

/// Parse a JSON object/array/scalar returned in a NIP-46 `result` field.
pub fn parse_json_result<T: DeserializeOwned>(
    result_json: &str,
    label: &str,
) -> Result<T, SignerError> {
    serde_json::from_str(result_json)
        .map_err(|e| SignerError::Backend(format!("malformed {label} response payload: {e}")))
}
