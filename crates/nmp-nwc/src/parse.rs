//! NIP-47 `nostr+walletconnect://` URI parser.
//!
//! ## Why not `nostr::nips::nip47::NostrWalletConnectURI`?
//!
//! The `nostr` crate ships a NWC URI type behind its `nip47` feature, but it is
//! a strict spec parser built on `url::Url::parse` and is **not** a drop-in
//! replacement for the real-world input this client must accept. `NwcUri`
//! retains, each pinned by a test below:
//!
//! - **Case-insensitive scheme** (`Nostr+walletconnect://`) — some deeplink
//!   handlers auto-capitalize the leading char; upstream rejects it.
//! - **Whitespace tolerance** — surrounding *and* inner (pre-`&`) whitespace
//!   from hand-copied wallet-UI strings is trimmed; upstream `url::Url` rejects.
//! - **`Zeroizing` on the client secret** — this is wallet-spending key
//!   material; upstream exposes a bare `SecretKey`.
//! - **Graceful non-ws relay handling** — a bad relay alongside a valid one is
//!   dropped, not fatal; upstream `RelayUrl::parse` silently skips but offers no
//!   typed `ParseError` surface for the all-bad case.
//!
//! Switching to the upstream type would regress every one of these. The crate
//! still enables `nostr`'s `nip04`/`nip44` features for the crypto path; it
//! intentionally does **not** enable `nip47`.

use zeroize::Zeroizing;

/// A parsed NWC connection URI.
///
/// Format: `nostr+walletconnect://<wallet_pubkey_hex>?relay=<url>(&relay=<url>)*&secret=<client_secret_hex>[&lud16=<email>]`
///
/// - `wallet_pubkey_hex`: the 64-char hex pubkey of the wallet service
/// - `client_secret_hex`: the 64-char hex of the client's secret key
/// - `relay_urls`: ordered list of relay URLs for NWC messages. NIP-47 allows
///   multiple `relay=` query parameters; values are trimmed (handling
///   real-world copy-paste whitespace) and deduplicated.
/// - `lud16`: optional Lightning address
#[derive(Debug, Clone)]
pub struct NwcUri {
    pub wallet_pubkey_hex: String,
    /// Client secret key (64-char hex). Wrapped in `Zeroizing` so the heap
    /// allocation is wiped on drop — this is wallet-spending key material.
    /// Accessors deref transparently to `String`: use `.as_str()` for `&str`.
    pub client_secret_hex: Zeroizing<String>,
    pub relay_urls: Vec<String>,
    pub lud16: Option<String>,
}

impl NwcUri {
    /// First relay in the URI's list. Parser guarantees `relay_urls` is non-empty.
    #[must_use] 
    pub fn primary_relay_url(&self) -> &str {
        &self.relay_urls[0]
    }
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
    /// Parse a `nostr+walletconnect://` URI string. The scheme match is
    /// case-insensitive — wallet UIs / mobile deeplink handlers sometimes emit
    /// `Nostr+walletconnect://` (auto-capitalize first letter).
    ///
    /// # Errors
    ///
    /// Returns `ParseError` if the URI scheme, wallet pubkey, or required `relay` parameter are invalid.
    pub fn parse(uri: &str) -> Result<Self, ParseError> {
        const SCHEME: &str = "nostr+walletconnect://";
        let uri = uri.trim();
        let rest = if uri.len() >= SCHEME.len() && uri[..SCHEME.len()].eq_ignore_ascii_case(SCHEME)
        {
            &uri[SCHEME.len()..]
        } else {
            return Err(ParseError::BadScheme);
        };

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

        let mut relay_urls: Vec<String> = Vec::new();
        let mut client_secret_hex: Option<String> = None;
        let mut lud16: Option<String> = None;

        for part in query.split('&') {
            if part.is_empty() {
                continue;
            }
            if let Some((k, v)) = part.split_once('=') {
                let v = url_decode(v);
                match k {
                    "relay" => {
                        let trimmed = v.trim().to_string();
                        if !trimmed.is_empty() && !relay_urls.contains(&trimmed) {
                            relay_urls.push(trimmed);
                        }
                    }
                    "secret" => client_secret_hex = Some(v),
                    "lud16" => lud16 = Some(v),
                    _ => {}
                }
            }
        }

        if relay_urls.is_empty() {
            return Err(ParseError::MissingRelay);
        }
        // Reject if no relay survives the ws:// scheme gate.
        if !relay_urls
            .iter()
            .any(|u| u.starts_with("wss://") || u.starts_with("ws://"))
        {
            return Err(ParseError::InvalidRelayUrl);
        }
        // Drop any non-ws schemes that snuck in alongside valid ones.
        relay_urls.retain(|u| u.starts_with("wss://") || u.starts_with("ws://"));

        let client_secret_hex = client_secret_hex.ok_or(ParseError::MissingSecret)?;
        if !is_hex64(&client_secret_hex) {
            return Err(ParseError::InvalidSecret);
        }

        Ok(Self {
            wallet_pubkey_hex,
            client_secret_hex: Zeroizing::new(client_secret_hex),
            relay_urls,
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
        assert_eq!(parsed.client_secret_hex.as_str(), secret);
        assert_eq!(parsed.relay_urls, vec!["wss://relay.example.com"]);
        assert_eq!(parsed.primary_relay_url(), "wss://relay.example.com");
        assert!(parsed.lud16.is_none());
    }

    /// Real-world URI shape: multiple `relay=` params + a stray trailing space
    /// before `&secret=` (the kind of formatting variance produced by hand-
    /// copying from a wallet UI). All relays survive, trimmed, in declared
    /// order; duplicates dedup.
    #[test]
    fn parse_multi_relay_trims_and_dedupes() {
        let wallet_pk = "e".repeat(64);
        let secret = "f".repeat(64);
        let uri = format!(
            "nostr+walletconnect://{}?relay=wss://relay.damus.io&relay=wss://relay.8333.space/&relay=wss://nos.lol&relay=wss://relay.primal.net&relay=wss://relay.primal.net &secret={}",
            wallet_pk, secret
        );
        let parsed = NwcUri::parse(&uri).unwrap();
        assert_eq!(
            parsed.relay_urls,
            vec![
                "wss://relay.damus.io",
                "wss://relay.8333.space/",
                "wss://nos.lol",
                "wss://relay.primal.net",
            ]
        );
        assert_eq!(parsed.primary_relay_url(), "wss://relay.damus.io");
        assert_eq!(parsed.client_secret_hex.as_str(), secret);
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

    /// Mobile keyboards / deeplink handlers sometimes auto-capitalize the
    /// scheme's leading char. The parser must still accept it — otherwise
    /// the iOS Connect-button enable check fails closed and the user is
    /// stuck without an obvious recovery.
    #[test]
    fn scheme_is_case_insensitive() {
        let wallet_pk = "a".repeat(64);
        let secret = "b".repeat(64);
        let uri = format!(
            "Nostr+walletconnect://{}?relay=wss://r.io&secret={}",
            wallet_pk, secret
        );
        let parsed = NwcUri::parse(&uri).unwrap();
        assert_eq!(parsed.wallet_pubkey_hex, wallet_pk);
    }

    #[test]
    fn missing_relay_returns_error() {
        let wallet_pk = "a".repeat(64);
        let secret = "b".repeat(64);
        let uri = format!("nostr+walletconnect://{}?secret={}", wallet_pk, secret);
        assert_eq!(NwcUri::parse(&uri).unwrap_err(), ParseError::MissingRelay);
    }

    /// Empty path segment before `?` — `MissingWalletPubkey`, not a panic.
    #[test]
    fn empty_wallet_pubkey_returns_error() {
        let secret = "b".repeat(64);
        let uri = format!("nostr+walletconnect://?relay=wss://r.io&secret={}", secret);
        assert_eq!(
            NwcUri::parse(&uri).unwrap_err(),
            ParseError::MissingWalletPubkey
        );
    }

    /// Wrong-length or non-hex wallet pubkey → `InvalidWalletPubkey`.
    #[test]
    fn invalid_wallet_pubkey_returns_error() {
        let secret = "b".repeat(64);
        // Too short.
        let short = format!(
            "nostr+walletconnect://{}?relay=wss://r.io&secret={}",
            "a".repeat(63),
            secret
        );
        assert_eq!(
            NwcUri::parse(&short).unwrap_err(),
            ParseError::InvalidWalletPubkey
        );
        // Non-hex character.
        let nonhex = format!(
            "nostr+walletconnect://{}?relay=wss://r.io&secret={}",
            "g".repeat(64),
            secret
        );
        assert_eq!(
            NwcUri::parse(&nonhex).unwrap_err(),
            ParseError::InvalidWalletPubkey
        );
    }

    /// `relay=` present but no entry survives the ws/wss scheme gate.
    #[test]
    fn non_ws_relay_returns_invalid_relay_url() {
        let wallet_pk = "a".repeat(64);
        let secret = "b".repeat(64);
        let uri = format!(
            "nostr+walletconnect://{}?relay=http://insecure.example.com&secret={}",
            wallet_pk, secret
        );
        assert_eq!(
            NwcUri::parse(&uri).unwrap_err(),
            ParseError::InvalidRelayUrl
        );
    }

    /// A non-ws relay alongside a valid wss relay is silently dropped — the
    /// connection still parses with only the valid relay retained.
    #[test]
    fn non_ws_relay_dropped_when_valid_relay_present() {
        let wallet_pk = "a".repeat(64);
        let secret = "b".repeat(64);
        let uri = format!(
            "nostr+walletconnect://{}?relay=http://bad.example&relay=wss://good.example&secret={}",
            wallet_pk, secret
        );
        let parsed = NwcUri::parse(&uri).unwrap();
        assert_eq!(parsed.relay_urls, vec!["wss://good.example"]);
    }

    /// `relay=` present but `secret=` absent → `MissingSecret`.
    #[test]
    fn missing_secret_returns_error() {
        let wallet_pk = "a".repeat(64);
        let uri = format!(
            "nostr+walletconnect://{}?relay=wss://r.io",
            wallet_pk
        );
        assert_eq!(NwcUri::parse(&uri).unwrap_err(), ParseError::MissingSecret);
    }

    /// Wrong-length or non-hex secret → `InvalidSecret`.
    #[test]
    fn invalid_secret_returns_error() {
        let wallet_pk = "a".repeat(64);
        let short = format!(
            "nostr+walletconnect://{}?relay=wss://r.io&secret={}",
            wallet_pk,
            "b".repeat(10)
        );
        assert_eq!(
            NwcUri::parse(&short).unwrap_err(),
            ParseError::InvalidSecret
        );
        let nonhex = format!(
            "nostr+walletconnect://{}?relay=wss://r.io&secret={}",
            wallet_pk,
            "z".repeat(64)
        );
        assert_eq!(
            NwcUri::parse(&nonhex).unwrap_err(),
            ParseError::InvalidSecret
        );
    }

    /// Leading/trailing whitespace around the whole URI is tolerated.
    #[test]
    fn surrounding_whitespace_is_trimmed() {
        let wallet_pk = "a".repeat(64);
        let secret = "b".repeat(64);
        let uri = format!(
            "  nostr+walletconnect://{}?relay=wss://r.io&secret={}\n",
            wallet_pk, secret
        );
        let parsed = NwcUri::parse(&uri).unwrap();
        assert_eq!(parsed.wallet_pubkey_hex, wallet_pk);
    }
}
