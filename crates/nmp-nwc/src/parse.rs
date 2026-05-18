//! NIP-47 `nostr+walletconnect://` URI parser.

/// A parsed NWC connection URI.
///
/// Format: `nostr+walletconnect://<wallet_pubkey_hex>?relay=<url>&secret=<client_secret_hex>[&lud16=<email>]`
///
/// - `wallet_pubkey_hex`: the 64-char hex pubkey of the wallet service
/// - `client_secret_hex`: the 64-char hex of the client's secret key
/// - `relay_url`: the relay URL for NWC messages
/// - `lud16`: optional Lightning address
#[derive(Debug, Clone)]
pub struct NwcUri {
    pub wallet_pubkey_hex: String,
    pub client_secret_hex: String,
    pub relay_url: String,
    pub lud16: Option<String>,
}

/// Parse errors for NWC URIs.
#[derive(Debug, Clone, PartialEq)]
pub enum ParseError {
    BadScheme,
    MissingWalletPubkey,
    InvalidWalletPubkey,
    MissingRelay,
    InvalidRelayUrl,
    MissingSecret,
    InvalidSecret,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::BadScheme => write!(f, "URI must start with nostr+walletconnect://"),
            Self::MissingWalletPubkey => write!(f, "missing wallet pubkey"),
            Self::InvalidWalletPubkey => write!(f, "wallet pubkey must be a 64-char hex string"),
            Self::MissingRelay => write!(f, "missing relay= query parameter"),
            Self::InvalidRelayUrl => write!(f, "relay must be a wss:// or ws:// URL"),
            Self::MissingSecret => write!(f, "missing secret= query parameter"),
            Self::InvalidSecret => write!(f, "secret must be a 64-char hex string"),
        }
    }
}

impl NwcUri {
    /// Parse a `nostr+walletconnect://` URI string.
    pub fn parse(uri: &str) -> Result<Self, ParseError> {
        let uri = uri.trim();
        let rest = uri
            .strip_prefix("nostr+walletconnect://")
            .ok_or(ParseError::BadScheme)?;

        // Split path from query: <wallet_pubkey>?<query>
        let (path, query) = match rest.split_once('?') {
            Some((p, q)) => (p, q),
            None => (rest, ""),
        };

        let wallet_pubkey_hex = path.to_string();
        if wallet_pubkey_hex.is_empty() {
            return Err(ParseError::MissingWalletPubkey);
        }
        if !is_hex64(&wallet_pubkey_hex) {
            return Err(ParseError::InvalidWalletPubkey);
        }

        let mut relay_url: Option<String> = None;
        let mut client_secret_hex: Option<String> = None;
        let mut lud16: Option<String> = None;

        for part in query.split('&') {
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once('=') {
                let v = url_decode(v);
                match k {
                    "relay" => relay_url = Some(v),
                    "secret" => client_secret_hex = Some(v),
                    "lud16" => lud16 = Some(v),
                    _ => {}
                }
            }
        }

        let relay_url = relay_url.ok_or(ParseError::MissingRelay)?;
        if !relay_url.starts_with("wss://") && !relay_url.starts_with("ws://") {
            return Err(ParseError::InvalidRelayUrl);
        }

        let client_secret_hex = client_secret_hex.ok_or(ParseError::MissingSecret)?;
        if !is_hex64(&client_secret_hex) {
            return Err(ParseError::InvalidSecret);
        }

        Ok(Self {
            wallet_pubkey_hex,
            client_secret_hex,
            relay_url,
            lud16,
        })
    }
}

fn is_hex64(s: &str) -> bool {
    s.len() == 64 && s.chars().all(|c| c.is_ascii_hexdigit())
}

/// Minimal percent-decode for relay URLs (handles %3A, %2F, etc.).
fn url_decode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let bytes = s.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                hex_nibble(bytes[i + 1]),
                hex_nibble(bytes[i + 2]),
            ) {
                out.push(char::from(hi << 4 | lo));
                i += 3;
                continue;
            }
        }
        out.push(bytes[i] as char);
        i += 1;
    }
    out
}

fn hex_nibble(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_well_formed_uri() {
        let wallet_pk = "a".repeat(64);
        let secret = "b".repeat(64);
        let uri = format!(
            "nostr+walletconnect://{}?relay=wss%3A%2F%2Frelay.example.com&secret={}",
            wallet_pk, secret
        );
        let parsed = NwcUri::parse(&uri).unwrap();
        assert_eq!(parsed.wallet_pubkey_hex, wallet_pk);
        assert_eq!(parsed.client_secret_hex, secret);
        assert_eq!(parsed.relay_url, "wss://relay.example.com");
        assert!(parsed.lud16.is_none());
    }

    #[test]
    fn parse_with_lud16() {
        let wallet_pk = "c".repeat(64);
        let secret = "d".repeat(64);
        let uri = format!(
            "nostr+walletconnect://{}?relay=wss://r.io&secret={}&lud16=user@wallet.com",
            wallet_pk, secret
        );
        let parsed = NwcUri::parse(&uri).unwrap();
        assert_eq!(parsed.lud16, Some("user@wallet.com".to_string()));
    }

    #[test]
    fn bad_scheme_returns_error() {
        assert_eq!(
            NwcUri::parse("nostr://abc").unwrap_err(),
            ParseError::BadScheme
        );
    }

    #[test]
    fn missing_relay_returns_error() {
        let wallet_pk = "a".repeat(64);
        let secret = "b".repeat(64);
        let uri = format!("nostr+walletconnect://{}?secret={}", wallet_pk, secret);
        assert_eq!(NwcUri::parse(&uri).unwrap_err(), ParseError::MissingRelay);
    }
}
