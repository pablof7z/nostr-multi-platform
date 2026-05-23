//! NIP-19: bech32-encoded entities for Nostr.
//!
//! Implements parse + format for the six entity types:
//! `npub`, `nsec`, `note` (bare 32-byte keys/ids) and the TLV forms
//! `nprofile`, `nevent`, `naddr`.
//!
//! # Example — bare key round-trip
//! ```
//! use nmp_core::nip19::{Nip19Entity, encode_npub, decode_npub};
//!
//! let hex = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
//! let bech = encode_npub(hex).unwrap();
//! assert!(bech.starts_with("npub1"));
//! let recovered = decode_npub(&bech).unwrap();
//! assert_eq!(recovered, hex);
//! ```
//!
//! # Example — nprofile round-trip
//! ```
//! use nmp_core::nip19::{NprofileData, encode_nprofile, decode_nprofile};
//!
//! let data = NprofileData {
//!     pubkey: "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d".into(),
//!     relays: vec!["wss://r.x".into()],
//! };
//! let bech = encode_nprofile(&data).unwrap();
//! assert!(bech.starts_with("nprofile1"));
//! let decoded = decode_nprofile(&bech).unwrap();
//! assert_eq!(decoded.pubkey, data.pubkey);
//! ```

use bech32::{Bech32, Bech32m, Hrp};

// ─── HRPs ──────────────────────────────────────────────────────────────────

const HRP_NPUB: &str = "npub";
const HRP_NSEC: &str = "nsec";
const HRP_NOTE: &str = "note";
const HRP_NPROFILE: &str = "nprofile";
const HRP_NEVENT: &str = "nevent";
const HRP_NADDR: &str = "naddr";

// ─── TLV type bytes ────────────────────────────────────────────────────────

/// TLV type byte: `special` (pubkey / event-id / d-tag).
pub const TLV_SPECIAL: u8 = 0;
/// TLV type byte: relay URL.
pub const TLV_RELAY: u8 = 1;
/// TLV type byte: author pubkey.
pub const TLV_AUTHOR: u8 = 2;
/// TLV type byte: event kind (4-byte big-endian u32).
pub const TLV_KIND: u8 = 3;

// ─── Public data types ─────────────────────────────────────────────────────

/// Structured data for an `nprofile` entity (public key + optional relays).
#[derive(Debug, Clone, PartialEq)]
pub struct NprofileData {
    /// 32-byte pubkey as a lowercase hex string.
    pub pubkey: String,
    /// Zero or more relay URLs.
    pub relays: Vec<String>,
}

/// Structured data for an `nevent` entity.
#[derive(Debug, Clone, PartialEq)]
pub struct NeventData {
    /// 32-byte event id as a lowercase hex string.
    pub event_id: String,
    /// Zero or more relay URLs.
    pub relays: Vec<String>,
    /// Optional author pubkey (hex).
    pub author: Option<String>,
    /// Optional event kind.
    pub kind: Option<u32>,
}

/// Structured data for an `naddr` entity (addressable / parameterised-replaceable events).
#[derive(Debug, Clone, PartialEq)]
pub struct NaddrData {
    /// The `d` tag identifier.
    pub identifier: String,
    /// Author pubkey (hex). Required for naddr.
    pub pubkey: String,
    /// Event kind. Required for naddr.
    pub kind: u32,
    /// Zero or more relay URLs.
    pub relays: Vec<String>,
}

/// All six NIP-19 entity variants.
#[derive(Debug, Clone, PartialEq)]
pub enum Nip19Entity {
    /// `npub` — public key.
    Npub(String),
    /// `nsec` — private key.
    Nsec(String),
    /// `note` — event id.
    Note(String),
    /// `nprofile` — public key + relays.
    Nprofile(NprofileData),
    /// `nevent` — event id + relays + optional author/kind.
    Nevent(NeventData),
    /// `naddr` — addressable event coordinate.
    Naddr(NaddrData),
}

/// Errors produced by NIP-19 encode/decode.
#[derive(Debug, PartialEq)]
pub enum Nip19Error {
    /// Input is not valid hex or wrong length.
    InvalidHex,
    /// bech32 encoding/decoding failure.
    Bech32(String),
    /// TLV structure is malformed.
    MalformedTlv(String),
    /// Unknown HRP — not a recognised NIP-19 prefix.
    UnknownHrp(String),
    /// A required TLV field is absent.
    MissingField(&'static str),
}

impl std::fmt::Display for Nip19Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHex => write!(f, "invalid hex input"),
            Self::Bech32(msg) => write!(f, "bech32 error: {msg}"),
            Self::MalformedTlv(msg) => write!(f, "malformed TLV: {msg}"),
            Self::UnknownHrp(hrp) => write!(f, "unknown HRP: {hrp}"),
            Self::MissingField(field) => write!(f, "missing required TLV field: {field}"),
        }
    }
}

// ─── Hex helpers ───────────────────────────────────────────────────────────

fn hex_to_bytes(hex: &str) -> Result<[u8; 32], Nip19Error> {
    if hex.len() != 64 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(Nip19Error::InvalidHex);
    }
    let mut out = [0u8; 32];
    for (i, chunk) in hex.as_bytes().chunks(2).enumerate() {
        let hi = nibble(chunk[0])?;
        let lo = nibble(chunk[1])?;
        out[i] = (hi << 4) | lo;
    }
    Ok(out)
}

fn nibble(b: u8) -> Result<u8, Nip19Error> {
    match b {
        b'0'..=b'9' => Ok(b - b'0'),
        b'a'..=b'f' => Ok(b - b'a' + 10),
        b'A'..=b'F' => Ok(b - b'A' + 10),
        _ => Err(Nip19Error::InvalidHex),
    }
}

#[must_use] 
pub fn bytes_to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|b| format!("{b:02x}")).collect()
}

// ─── Bare-key encode / decode ──────────────────────────────────────────────

fn encode_bare(hrp_str: &str, hex: &str) -> Result<String, Nip19Error> {
    let bytes = hex_to_bytes(hex)?;
    let hrp = Hrp::parse(hrp_str).map_err(|e| Nip19Error::Bech32(e.to_string()))?;
    bech32::encode::<Bech32>(hrp, &bytes).map_err(|e| Nip19Error::Bech32(e.to_string()))
}

fn decode_bare(bech: &str, expected_hrp: &str) -> Result<String, Nip19Error> {
    let (hrp, bytes) = bech32::decode(bech).map_err(|e| Nip19Error::Bech32(e.to_string()))?;
    if hrp.as_str() != expected_hrp {
        return Err(Nip19Error::UnknownHrp(hrp.to_string()));
    }
    if bytes.len() != 32 {
        return Err(Nip19Error::MalformedTlv(format!(
            "expected 32 bytes, got {}",
            bytes.len()
        )));
    }
    Ok(bytes_to_hex(&bytes))
}

/// Encode a public key hex string as an `npub` bech32 string.
pub fn encode_npub(hex: &str) -> Result<String, Nip19Error> {
    encode_bare(HRP_NPUB, hex)
}

/// Decode an `npub` bech32 string to a hex public key.
pub fn decode_npub(bech: &str) -> Result<String, Nip19Error> {
    decode_bare(bech, HRP_NPUB)
}

/// Encode a private key hex string as an `nsec` bech32 string.
pub fn encode_nsec(hex: &str) -> Result<String, Nip19Error> {
    encode_bare(HRP_NSEC, hex)
}

/// Decode an `nsec` bech32 string to a hex private key.
pub fn decode_nsec(bech: &str) -> Result<String, Nip19Error> {
    decode_bare(bech, HRP_NSEC)
}

/// Encode an event id hex string as a `note` bech32 string.
pub fn encode_note(hex: &str) -> Result<String, Nip19Error> {
    encode_bare(HRP_NOTE, hex)
}

/// Decode a `note` bech32 string to a hex event id.
pub fn decode_note(bech: &str) -> Result<String, Nip19Error> {
    decode_bare(bech, HRP_NOTE)
}

// ─── TLV helpers ───────────────────────────────────────────────────────────

/// Append one TLV entry to `buf`. Panics if `value.len() > 255`.
pub fn tlv_append(buf: &mut Vec<u8>, typ: u8, value: &[u8]) {
    assert!(value.len() <= 255, "TLV value too long");
    buf.push(typ);
    buf.push(value.len() as u8);
    buf.extend_from_slice(value);
}

/// Iterate over TLV triplets `(type, value)`.
fn tlv_iter(data: &[u8]) -> TlvIter<'_> {
    TlvIter { data, pos: 0 }
}

struct TlvIter<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Iterator for TlvIter<'a> {
    type Item = Result<(u8, &'a [u8]), Nip19Error>;

    fn next(&mut self) -> Option<Self::Item> {
        if self.pos >= self.data.len() {
            return None;
        }
        if self.pos + 2 > self.data.len() {
            return Some(Err(Nip19Error::MalformedTlv("truncated type/length".into())));
        }
        let typ = self.data[self.pos];
        let len = self.data[self.pos + 1] as usize;
        self.pos += 2;
        if self.pos + len > self.data.len() {
            return Some(Err(Nip19Error::MalformedTlv(format!(
                "TLV value truncated: need {len} bytes"
            ))));
        }
        let value = &self.data[self.pos..self.pos + len];
        self.pos += len;
        Some(Ok((typ, value)))
    }
}

fn encode_tlv(hrp_str: &str, tlv: &[u8]) -> Result<String, Nip19Error> {
    let hrp = Hrp::parse(hrp_str).map_err(|e| Nip19Error::Bech32(e.to_string()))?;
    bech32::encode::<Bech32m>(hrp, tlv).map_err(|e| Nip19Error::Bech32(e.to_string()))
}

fn decode_tlv(bech: &str, expected_hrp: &str) -> Result<Vec<u8>, Nip19Error> {
    let (hrp, bytes) = bech32::decode(bech).map_err(|e| Nip19Error::Bech32(e.to_string()))?;
    if hrp.as_str() != expected_hrp {
        return Err(Nip19Error::UnknownHrp(hrp.to_string()));
    }
    Ok(bytes)
}

// ─── nprofile ──────────────────────────────────────────────────────────────

/// Encode an `NprofileData` as an `nprofile` bech32m string.
pub fn encode_nprofile(data: &NprofileData) -> Result<String, Nip19Error> {
    let key_bytes = hex_to_bytes(&data.pubkey)?;
    let mut tlv = Vec::new();
    tlv_append(&mut tlv, TLV_SPECIAL, &key_bytes);
    for relay in &data.relays {
        tlv_append(&mut tlv, TLV_RELAY, relay.as_bytes());
    }
    encode_tlv(HRP_NPROFILE, &tlv)
}

/// Decode an `nprofile` bech32m string into `NprofileData`.
pub fn decode_nprofile(bech: &str) -> Result<NprofileData, Nip19Error> {
    let bytes = decode_tlv(bech, HRP_NPROFILE)?;
    let mut pubkey: Option<String> = None;
    let mut relays = Vec::new();
    for item in tlv_iter(&bytes) {
        let (typ, val) = item?;
        match typ {
            TLV_SPECIAL => {
                if val.len() != 32 {
                    return Err(Nip19Error::MalformedTlv("pubkey must be 32 bytes".into()));
                }
                pubkey = Some(bytes_to_hex(val));
            }
            TLV_RELAY => relays.push(String::from_utf8_lossy(val).into_owned()),
            _ => {} // unknown TLV types are ignored per spec
        }
    }
    Ok(NprofileData {
        pubkey: pubkey.ok_or(Nip19Error::MissingField("special/pubkey"))?,
        relays,
    })
}

// ─── nevent ────────────────────────────────────────────────────────────────

/// Encode an `NeventData` as an `nevent` bech32m string.
pub fn encode_nevent(data: &NeventData) -> Result<String, Nip19Error> {
    let id_bytes = hex_to_bytes(&data.event_id)?;
    let mut tlv = Vec::new();
    tlv_append(&mut tlv, TLV_SPECIAL, &id_bytes);
    for relay in &data.relays {
        tlv_append(&mut tlv, TLV_RELAY, relay.as_bytes());
    }
    if let Some(ref author) = data.author {
        tlv_append(&mut tlv, TLV_AUTHOR, &hex_to_bytes(author)?);
    }
    if let Some(kind) = data.kind {
        tlv_append(&mut tlv, TLV_KIND, &kind.to_be_bytes());
    }
    encode_tlv(HRP_NEVENT, &tlv)
}

/// Decode an `nevent` bech32m string into `NeventData`.
pub fn decode_nevent(bech: &str) -> Result<NeventData, Nip19Error> {
    let bytes = decode_tlv(bech, HRP_NEVENT)?;
    let mut event_id: Option<String> = None;
    let mut relays = Vec::new();
    let mut author: Option<String> = None;
    let mut kind: Option<u32> = None;
    for item in tlv_iter(&bytes) {
        let (typ, val) = item?;
        match typ {
            TLV_SPECIAL => {
                if val.len() != 32 {
                    return Err(Nip19Error::MalformedTlv("event id must be 32 bytes".into()));
                }
                event_id = Some(bytes_to_hex(val));
            }
            TLV_RELAY => relays.push(String::from_utf8_lossy(val).into_owned()),
            TLV_AUTHOR => {
                if val.len() != 32 {
                    return Err(Nip19Error::MalformedTlv("author must be 32 bytes".into()));
                }
                author = Some(bytes_to_hex(val));
            }
            TLV_KIND => {
                if val.len() != 4 {
                    return Err(Nip19Error::MalformedTlv("kind must be 4 bytes".into()));
                }
                kind = Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]]));
            }
            _ => {}
        }
    }
    Ok(NeventData {
        event_id: event_id.ok_or(Nip19Error::MissingField("special/event_id"))?,
        relays,
        author,
        kind,
    })
}

// ─── naddr ─────────────────────────────────────────────────────────────────

/// Encode an `NaddrData` as an `naddr` bech32m string.
pub fn encode_naddr(data: &NaddrData) -> Result<String, Nip19Error> {
    let author_bytes = hex_to_bytes(&data.pubkey)?;
    let mut tlv = Vec::new();
    tlv_append(&mut tlv, TLV_SPECIAL, data.identifier.as_bytes());
    for relay in &data.relays {
        tlv_append(&mut tlv, TLV_RELAY, relay.as_bytes());
    }
    tlv_append(&mut tlv, TLV_AUTHOR, &author_bytes);
    tlv_append(&mut tlv, TLV_KIND, &data.kind.to_be_bytes());
    encode_tlv(HRP_NADDR, &tlv)
}

/// Decode an `naddr` bech32m string into `NaddrData`.
pub fn decode_naddr(bech: &str) -> Result<NaddrData, Nip19Error> {
    let bytes = decode_tlv(bech, HRP_NADDR)?;
    let mut identifier: Option<String> = None;
    let mut relays = Vec::new();
    let mut pubkey: Option<String> = None;
    let mut kind: Option<u32> = None;
    for item in tlv_iter(&bytes) {
        let (typ, val) = item?;
        match typ {
            TLV_SPECIAL => identifier = Some(String::from_utf8_lossy(val).into_owned()),
            TLV_RELAY => relays.push(String::from_utf8_lossy(val).into_owned()),
            TLV_AUTHOR => {
                if val.len() != 32 {
                    return Err(Nip19Error::MalformedTlv("author must be 32 bytes".into()));
                }
                pubkey = Some(bytes_to_hex(val));
            }
            TLV_KIND => {
                if val.len() != 4 {
                    return Err(Nip19Error::MalformedTlv("kind must be 4 bytes".into()));
                }
                kind = Some(u32::from_be_bytes([val[0], val[1], val[2], val[3]]));
            }
            _ => {}
        }
    }
    Ok(NaddrData {
        identifier: identifier.ok_or(Nip19Error::MissingField("special/identifier"))?,
        pubkey: pubkey.ok_or(Nip19Error::MissingField("author"))?,
        kind: kind.ok_or(Nip19Error::MissingField("kind"))?,
        relays,
    })
}

// ─── Top-level polymorphic parse / format ──────────────────────────────────

/// Parse any NIP-19 bech32 string into a typed `Nip19Entity`.
///
/// # Example
/// ```
/// use nmp_core::nip19::{parse, Nip19Entity};
///
/// let bech = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
/// let entity = parse(bech).unwrap();
/// assert!(matches!(entity, Nip19Entity::Npub(_)));
/// ```
pub fn parse(bech: &str) -> Result<Nip19Entity, Nip19Error> {
    let sep = bech
        .rfind('1')
        .ok_or_else(|| Nip19Error::Bech32("no separator '1'".into()))?;
    match &bech[..sep] {
        HRP_NPUB => Ok(Nip19Entity::Npub(decode_npub(bech)?)),
        HRP_NSEC => Ok(Nip19Entity::Nsec(decode_nsec(bech)?)),
        HRP_NOTE => Ok(Nip19Entity::Note(decode_note(bech)?)),
        HRP_NPROFILE => Ok(Nip19Entity::Nprofile(decode_nprofile(bech)?)),
        HRP_NEVENT => Ok(Nip19Entity::Nevent(decode_nevent(bech)?)),
        HRP_NADDR => Ok(Nip19Entity::Naddr(decode_naddr(bech)?)),
        other => Err(Nip19Error::UnknownHrp(other.to_string())),
    }
}

/// Format any `Nip19Entity` back to a bech32 string.
pub fn format(entity: &Nip19Entity) -> Result<String, Nip19Error> {
    match entity {
        Nip19Entity::Npub(hex) => encode_npub(hex),
        Nip19Entity::Nsec(hex) => encode_nsec(hex),
        Nip19Entity::Note(hex) => encode_note(hex),
        Nip19Entity::Nprofile(data) => encode_nprofile(data),
        Nip19Entity::Nevent(data) => encode_nevent(data),
        Nip19Entity::Naddr(data) => encode_naddr(data),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Deterministic 32-byte hex fixture (matches the module doctests).
    const PK: &str = "3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d";
    /// A second distinct deterministic 32-byte hex fixture (event id / author).
    const ID: &str = "0000000000000000000000000000000000000000000000000000000000000001";

    // ─── parse() polymorphic dispatcher ────────────────────────────────────

    #[test]
    fn parse_dispatches_npub_to_npub_variant() {
        let bech = encode_npub(PK).unwrap();
        assert_eq!(parse(&bech).unwrap(), Nip19Entity::Npub(PK.into()));
    }

    #[test]
    fn parse_dispatches_note_to_note_variant() {
        let bech = encode_note(ID).unwrap();
        assert_eq!(parse(&bech).unwrap(), Nip19Entity::Note(ID.into()));
    }

    #[test]
    fn parse_dispatches_nprofile_to_nprofile_variant() {
        let data = NprofileData {
            pubkey: PK.into(),
            relays: vec!["wss://relay.example".into()],
        };
        let bech = encode_nprofile(&data).unwrap();
        assert_eq!(parse(&bech).unwrap(), Nip19Entity::Nprofile(data));
    }

    // ─── nevent round-trip with author + kind (exercises 4-byte TLV_KIND) ──

    #[test]
    fn nevent_round_trip_preserves_author_and_kind() {
        let data = NeventData {
            event_id: ID.into(),
            relays: vec!["wss://relay.example".into()],
            author: Some(PK.into()),
            kind: Some(1),
        };
        let bech = encode_nevent(&data).unwrap();
        assert!(bech.starts_with("nevent1"));
        let decoded = decode_nevent(&bech).unwrap();
        assert_eq!(decoded, data);
    }

    // ─── error paths — silent-failure classes ──────────────────────────────

    #[test]
    fn parse_non_bech32_input_errors_without_panic() {
        // No '1' separator at all — must be a graceful Err, never a panic.
        let err = parse("notbech32atall").unwrap_err();
        assert!(matches!(err, Nip19Error::Bech32(_)));
    }

    #[test]
    fn parse_unknown_hrp_errors_without_panic() {
        // Syntactically bech32-shaped but an unrecognised HRP.
        let err = parse("xyz1qqqqqqqq").unwrap_err();
        assert!(matches!(err, Nip19Error::UnknownHrp(hrp) if hrp == "xyz"));
    }

    #[test]
    fn decode_npub_rejects_cross_hrp_nprofile_string() {
        // Cross-HRP confusion is a real silent-routing bug class: an
        // nprofile string fed to decode_npub must not silently succeed.
        let nprofile = encode_nprofile(&NprofileData {
            pubkey: PK.into(),
            relays: vec![],
        })
        .unwrap();
        let err = decode_npub(&nprofile).unwrap_err();
        assert!(matches!(err, Nip19Error::UnknownHrp(hrp) if hrp == "nprofile"));
    }

    #[test]
    fn encode_npub_rejects_non_hex_input() {
        let err = encode_npub("not-hex-and-wrong-length").unwrap_err();
        assert_eq!(err, Nip19Error::InvalidHex);
    }

    #[test]
    fn decode_nprofile_missing_special_tlv_errors() {
        // A valid nprofile-HRP bech32m payload that omits TLV_SPECIAL must
        // surface MissingField rather than yielding an empty-pubkey struct.
        let mut tlv = Vec::new();
        tlv_append(&mut tlv, TLV_RELAY, b"wss://relay.example");
        let bech = encode_tlv(HRP_NPROFILE, &tlv).unwrap();
        let err = decode_nprofile(&bech).unwrap_err();
        assert_eq!(err, Nip19Error::MissingField("special/pubkey"));
    }
}
