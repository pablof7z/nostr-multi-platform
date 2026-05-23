//! Pure-function `bunker://` URL parser.  No allocations beyond what `url`
//! and `Vec`/`String` need; total parse cost is microseconds even on adversarial
//! 4 KiB input.

use std::fmt;

use zeroize::Zeroizing;

/// Maximum accepted `bunker://` URI length in bytes.  Anything longer is a
/// red flag (a real URI is ~150–300 bytes); reject fast rather than allocate.
pub const MAX_BUNKER_URI_LEN: usize = 4 * 1024;

/// Parsed `bunker://` URI.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BunkerUri {
    /// Remote signer pubkey, lowercase hex (64 chars).
    pub remote_pubkey_hex: String,
    /// One or more rendezvous relays (`ws://` or `wss://`).
    pub relays: Vec<String>,
    /// Optional connection secret (`?secret=...`).
    ///
    /// Wrapped in [`Zeroizing`] — a NIP-46 connection secret is sensitive
    /// credential material and is wiped from the heap when the `BunkerUri`
    /// is dropped.
    pub secret: Option<Zeroizing<String>>,
    /// Optional permissions string (`?perms=sign_event:1,nip04_encrypt`).
    pub permissions: Option<String>,
    /// Unknown query params preserved for round-trip.
    pub extra: Vec<(String, String)>,
}

/// Errors returned by [`parse_bunker_uri`].
#[derive(Clone, Debug, Eq, PartialEq)]
pub enum BunkerParseError {
    /// Empty input.
    Empty,
    /// Input exceeded [`MAX_BUNKER_URI_LEN`].
    TooLong(usize),
    /// Scheme must be `bunker`.
    WrongScheme(String),
    /// Pubkey was missing, wrong length, or contained non-hex chars.
    InvalidPubkey(String),
    /// At least one `?relay=...` is required.
    NoRelay,
    /// A `relay=` value failed URL parsing or wasn't `ws://` / `wss://`.
    InvalidRelay(String),
    /// Catch-all for malformed input.
    Malformed(String),
}

impl fmt::Display for BunkerParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Empty => f.write_str("empty bunker uri"),
            Self::TooLong(n) => write!(f, "bunker uri too long ({n} bytes)"),
            Self::WrongScheme(s) => write!(f, "wrong scheme: {s}"),
            Self::InvalidPubkey(s) => write!(f, "invalid pubkey: {s}"),
            Self::NoRelay => f.write_str("at least one relay required"),
            Self::InvalidRelay(s) => write!(f, "invalid relay url: {s}"),
            Self::Malformed(s) => write!(f, "malformed uri: {s}"),
        }
    }
}

impl std::error::Error for BunkerParseError {}

impl fmt::Display for BunkerUri {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "bunker://{}", self.remote_pubkey_hex)?;
        let mut first = true;
        let sep = |f: &mut fmt::Formatter<'_>, first: &mut bool| -> fmt::Result {
            if *first {
                f.write_str("?")?;
                *first = false;
            } else {
                f.write_str("&")?;
            }
            Ok(())
        };
        for r in &self.relays {
            sep(f, &mut first)?;
            write!(f, "relay={}", url_encode(r))?;
        }
        if let Some(s) = &self.secret {
            sep(f, &mut first)?;
            write!(f, "secret={}", url_encode(s.as_str()))?;
        }
        if let Some(p) = &self.permissions {
            sep(f, &mut first)?;
            write!(f, "perms={}", url_encode(p))?;
        }
        for (k, v) in &self.extra {
            sep(f, &mut first)?;
            write!(f, "{}={}", url_encode(k), url_encode(v))?;
        }
        Ok(())
    }
}

/// Parse a `bunker://` URI.  Pure function, no allocations beyond the result.
pub fn parse_bunker_uri(input: &str) -> Result<BunkerUri, BunkerParseError> {
    if input.is_empty() {
        return Err(BunkerParseError::Empty);
    }
    if input.len() > MAX_BUNKER_URI_LEN {
        return Err(BunkerParseError::TooLong(input.len()));
    }

    // Don't use `url::Url::parse` for the scheme step — `url` is picky about
    // `bunker://` (non-special scheme) and we want a deterministic prefix check
    // that can't be tricked by `Bunker://` or whitespace.
    let trimmed = input.trim_start();
    let lc_prefix = trimmed
        .get(..9)
        .ok_or_else(|| BunkerParseError::WrongScheme(input.to_string()))?
        .to_ascii_lowercase();
    if lc_prefix != "bunker://" {
        return Err(BunkerParseError::WrongScheme(input.to_string()));
    }
    let rest = &trimmed[9..];

    // Split host (pubkey) from query.
    let (pubkey_raw, query_raw) = match rest.find('?') {
        Some(idx) => (&rest[..idx], &rest[idx + 1..]),
        None => (rest, ""),
    };

    let pubkey_clean = strip_trailing_slash(pubkey_raw);
    let pubkey = normalise_pubkey(pubkey_clean)?;

    let mut relays: Vec<String> = Vec::new();
    let mut secret: Option<Zeroizing<String>> = None;
    let mut permissions: Option<String> = None;
    let mut extra: Vec<(String, String)> = Vec::new();

    if !query_raw.is_empty() {
        for pair in query_raw.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (k_raw, v_raw) = match pair.find('=') {
                Some(idx) => (&pair[..idx], &pair[idx + 1..]),
                None => (pair, ""),
            };
            let k = url_decode(k_raw);
            let v = url_decode(v_raw);
            match k.as_str() {
                "relay" => {
                    validate_relay_url(&v)?;
                    if !relays.iter().any(|r| r == &v) {
                        relays.push(v);
                    }
                }
                "secret" => {
                    if !v.is_empty() {
                        secret = Some(Zeroizing::new(v));
                    }
                }
                "perms" | "permissions" => {
                    if !v.is_empty() {
                        permissions = Some(v);
                    }
                }
                _ => extra.push((k, v)),
            }
        }
    }

    if relays.is_empty() {
        return Err(BunkerParseError::NoRelay);
    }

    Ok(BunkerUri {
        remote_pubkey_hex: pubkey,
        relays,
        secret,
        permissions,
        extra,
    })
}

fn strip_trailing_slash(s: &str) -> &str {
    s.strip_suffix('/').unwrap_or(s)
}

fn normalise_pubkey(s: &str) -> Result<String, BunkerParseError> {
    if s.len() != 64 {
        return Err(BunkerParseError::InvalidPubkey(format!(
            "expected 64 hex chars, got {}",
            s.len()
        )));
    }
    if !s.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(BunkerParseError::InvalidPubkey(
            "non-hex character in pubkey".to_string(),
        ));
    }
    Ok(s.to_ascii_lowercase())
}

fn validate_relay_url(s: &str) -> Result<(), BunkerParseError> {
    if s.is_empty() {
        return Err(BunkerParseError::InvalidRelay("empty relay".to_string()));
    }
    let lower = s.to_ascii_lowercase();
    if !(lower.starts_with("ws://") || lower.starts_with("wss://")) {
        return Err(BunkerParseError::InvalidRelay(format!(
            "scheme not ws/wss: {s}"
        )));
    }
    // Best-effort URL parse — `url::Url` accepts ws/wss as special schemes.
    url::Url::parse(s).map_err(|e| BunkerParseError::InvalidRelay(format!("{s}: {e}")))?;
    Ok(())
}

/// Minimal percent-decode.  Used because the `bunker://` host is not a
/// `url::Url::host()` (it's a raw hex string).
fn url_decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b == b'%' && i + 2 < bytes.len() {
            if let (Some(hi), Some(lo)) = (
                hex_digit(bytes[i + 1]),
                hex_digit(bytes[i + 2]),
            ) {
                out.push((hi << 4) | lo);
                i += 3;
                continue;
            }
        }
        if b == b'+' {
            out.push(b' ');
            i += 1;
            continue;
        }
        out.push(b);
        i += 1;
    }
    String::from_utf8(out).unwrap_or_else(|e| String::from_utf8_lossy(&e.into_bytes()).into_owned())
}

fn hex_digit(b: u8) -> Option<u8> {
    match b {
        b'0'..=b'9' => Some(b - b'0'),
        b'a'..=b'f' => Some(b - b'a' + 10),
        b'A'..=b'F' => Some(b - b'A' + 10),
        _ => None,
    }
}

/// Percent-encode for safe characters used in our `to_string`.
fn url_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if matches!(b,
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' |
            b'-' | b'.' | b'_' | b'~' |
            b':' | b'/' | b'?' | b'#' | b'@' | b'!' | b'$' | b',' | b';' | b'='
        ) {
            out.push(b as char);
        } else {
            out.push('%');
            out.push(hex_nybble(b >> 4));
            out.push(hex_nybble(b & 0x0F));
        }
    }
    out
}

fn hex_nybble(n: u8) -> char {
    match n {
        0..=9 => (b'0' + n) as char,
        10..=15 => (b'a' + n - 10) as char,
        _ => '0',
    }
}
