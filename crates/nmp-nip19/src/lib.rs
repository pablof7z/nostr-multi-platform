//! NIP-19: bech32-encoded entities for Nostr.
//!
//! Thin adapter over [`nostr::nips::nip19`] (the rust-nostr canonical codec).
//! NMP keeps its own typed surface — [`Nip19Entity`], [`NprofileData`],
//! [`NeventData`], [`NaddrData`], the `encode_*` / `decode_*` /
//! [`parse`] / [`format`] free functions, and the [`Nip19Error`] enum — so
//! every existing caller is source-compatible, but the actual bech32 + TLV
//! encoding/decoding is delegated to `nostr` rather than re-implemented here.
//!
//! This is a Layer-4 protocol crate, carved out of the `nmp-core` kernel
//! substrate (issue #2515) per `docs/architecture/crate-boundaries.md` §3 —
//! the substrate must stay generic and must not own protocol-specific
//! parsers/nouns; a typed NIP-19 codec is a protocol module that belongs in
//! L4.
//!
//! Per AGENTS.md / aim.md ("reuse the `nostr` crate; never re-implement a
//! protocol codec from scratch") this crate is an NMP-shaped wrapper, not a
//! second codec. The previous hand-rolled bech32/TLV implementation was the
//! parallel codec #1493 set out to retire — having two NIP-19 codecs in one
//! workspace is a divergence/correctness hazard.
//!
//! # Example — bare key round-trip
//! ```
//! use nmp_nip19::{Nip19Entity, encode_npub, decode_npub};
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
//! use nmp_nip19::{NprofileData, encode_nprofile, decode_nprofile};
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

use nostr::nips::nip01::Coordinate;
use nostr::nips::nip19::{FromBech32, Nip19, Nip19Coordinate, Nip19Event, Nip19Profile, ToBech32};
use nostr::{EventId, Kind, PublicKey, RelayUrl, SecretKey};

// ─── HRPs ──────────────────────────────────────────────────────────────────

const HRP_NPUB: &str = "npub";
const HRP_NSEC: &str = "nsec";
const HRP_NOTE: &str = "note";
const HRP_NPROFILE: &str = "nprofile";
const HRP_NEVENT: &str = "nevent";
const HRP_NADDR: &str = "naddr";

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
    /// bech32 encoding/decoding failure (also covers an unparseable relay URL
    /// inside a TLV payload).
    Bech32(String),
    /// TLV structure is malformed.
    MalformedTlv(String),
    /// Unknown HRP — not a recognised NIP-19 prefix.
    UnknownHrp(String),
}

impl std::fmt::Display for Nip19Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidHex => write!(f, "invalid hex input"),
            Self::Bech32(msg) => write!(f, "bech32 error: {msg}"),
            Self::MalformedTlv(msg) => write!(f, "malformed TLV: {msg}"),
            Self::UnknownHrp(hrp) => write!(f, "unknown HRP: {hrp}"),
        }
    }
}

impl std::error::Error for Nip19Error {}

// ─── Hex helpers ───────────────────────────────────────────────────────────

/// Reject obviously-malformed hex *before* handing it to `nostr`, so a bad
/// pubkey / id surfaces as [`Nip19Error::InvalidHex`] (the variant callers
/// match on) rather than a generic key/parse error.
fn require_hex64(hex: &str) -> Result<(), Nip19Error> {
    if hex.len() == 64 && hex.bytes().all(|b| b.is_ascii_hexdigit()) {
        Ok(())
    } else {
        Err(Nip19Error::InvalidHex)
    }
}

// ─── Error mapping ───────────────────────────────────────────────────────────

/// Map a `nostr` NIP-19 error onto NMP's typed surface.
fn map_nostr_err(err: nostr::nips::nip19::Error) -> Nip19Error {
    use nostr::nips::nip19::Error as N;
    match err {
        N::WrongPrefix => Nip19Error::UnknownHrp(String::new()),
        N::FieldMissing(_) | N::TLV | N::TryFromSlice => Nip19Error::MalformedTlv(err.to_string()),
        N::Keys(_) | N::Event(_) => Nip19Error::InvalidHex,
        N::RelayUrl(_) => Nip19Error::Bech32(err.to_string()),
        other => Nip19Error::Bech32(other.to_string()),
    }
}

/// The HRP (human-readable prefix) of a bech32 string — the substring before
/// the final `1` separator. Used to give [`Nip19Error::UnknownHrp`] the actual
/// prefix and to reject cross-HRP confusion in the `decode_*` helpers (e.g. an
/// `nprofile…` fed to [`decode_npub`]).
fn hrp_of(bech: &str) -> Result<&str, Nip19Error> {
    bech.rfind('1')
        .map(|sep| &bech[..sep])
        .filter(|h| !h.is_empty())
        .ok_or_else(|| Nip19Error::Bech32("no separator '1'".into()))
}

/// Guard that `bech`'s HRP equals `expected`; otherwise surface the actual HRP
/// as [`Nip19Error::UnknownHrp`] (cross-HRP confusion is a silent-routing bug
/// class — see `decode_npub_rejects_cross_hrp_nprofile_string`).
fn require_hrp(bech: &str, expected: &str) -> Result<(), Nip19Error> {
    let hrp = hrp_of(bech)?;
    if hrp == expected {
        Ok(())
    } else {
        Err(Nip19Error::UnknownHrp(hrp.to_string()))
    }
}

// ─── Conversions: nostr types → NMP data ─────────────────────────────────────

fn relays_to_strings(relays: &[RelayUrl]) -> Vec<String> {
    relays.iter().map(ToString::to_string).collect()
}

fn relays_from_strings(relays: &[String]) -> Result<Vec<RelayUrl>, Nip19Error> {
    relays
        .iter()
        .map(|r| {
            // NIP-19 relay TLV length is a single byte, so a relay URL over
            // 255 bytes would silently corrupt the encoding. Reject up-front
            // with a typed error rather than emit a non-round-trippable string.
            if r.len() > MAX_TLV_VALUE_LEN {
                return Err(Nip19Error::MalformedTlv(format!(
                    "relay URL exceeds {MAX_TLV_VALUE_LEN}-byte TLV limit: {} bytes",
                    r.len()
                )));
            }
            RelayUrl::parse(r).map_err(|e| Nip19Error::Bech32(e.to_string()))
        })
        .collect()
}

/// Maximum byte length of a single NIP-19 TLV value (the length prefix is one
/// byte). Relay URLs and the `naddr` identifier must fit, or the encoded
/// entity would not round-trip.
const MAX_TLV_VALUE_LEN: usize = 255;

/// Convert an NMP `u32` kind to a `nostr::Kind`, rejecting values outside the
/// protocol's u16 domain (0..=65535). Without this guard a `u32` ≥ 65536 would
/// wrap on the `as u16` cast and silently encode a different kind.
fn kind_to_nostr(kind: u32) -> Result<Kind, Nip19Error> {
    u16::try_from(kind)
        .map(Kind::from_u16)
        .map_err(|_| Nip19Error::MalformedTlv(format!("event kind {kind} exceeds u16 range")))
}

fn nprofile_to_data(p: &Nip19Profile) -> NprofileData {
    NprofileData {
        pubkey: p.public_key.to_hex(),
        relays: relays_to_strings(&p.relays),
    }
}

fn nevent_to_data(e: &Nip19Event) -> NeventData {
    NeventData {
        event_id: e.event_id.to_hex(),
        relays: relays_to_strings(&e.relays),
        author: e.author.map(|a| a.to_hex()),
        kind: e.kind.map(|k| k.as_u16() as u32),
    }
}

fn ncoordinate_to_data(c: &Nip19Coordinate) -> NaddrData {
    NaddrData {
        identifier: c.coordinate.identifier.clone(),
        pubkey: c.coordinate.public_key.to_hex(),
        kind: c.coordinate.kind.as_u16() as u32,
        relays: relays_to_strings(&c.relays),
    }
}

// ─── Conversions: NMP data → nostr types ─────────────────────────────────────

fn pubkey_from_hex(hex: &str) -> Result<PublicKey, Nip19Error> {
    require_hex64(hex)?;
    PublicKey::from_hex(hex).map_err(|_| Nip19Error::InvalidHex)
}

fn event_id_from_hex(hex: &str) -> Result<EventId, Nip19Error> {
    require_hex64(hex)?;
    EventId::from_hex(hex).map_err(|_| Nip19Error::InvalidHex)
}

fn data_to_nprofile(data: &NprofileData) -> Result<Nip19Profile, Nip19Error> {
    Ok(Nip19Profile::new(
        pubkey_from_hex(&data.pubkey)?,
        relays_from_strings(&data.relays)?,
    ))
}

fn data_to_nevent(data: &NeventData) -> Result<Nip19Event, Nip19Error> {
    let mut ev = Nip19Event::new(event_id_from_hex(&data.event_id)?);
    ev.relays = relays_from_strings(&data.relays)?;
    ev.author = match &data.author {
        Some(a) => Some(pubkey_from_hex(a)?),
        None => None,
    };
    ev.kind = match data.kind {
        Some(k) => Some(kind_to_nostr(k)?),
        None => None,
    };
    Ok(ev)
}

fn data_to_ncoordinate(data: &NaddrData) -> Result<Nip19Coordinate, Nip19Error> {
    // The `d`-tag identifier is encoded as the `special` TLV (single-byte
    // length); reject over-255-byte identifiers up-front.
    if data.identifier.len() > MAX_TLV_VALUE_LEN {
        return Err(Nip19Error::MalformedTlv(format!(
            "naddr identifier exceeds {MAX_TLV_VALUE_LEN}-byte TLV limit: {} bytes",
            data.identifier.len()
        )));
    }
    let coord = Coordinate::new(kind_to_nostr(data.kind)?, pubkey_from_hex(&data.pubkey)?)
        .identifier(data.identifier.clone());
    Ok(Nip19Coordinate::new(
        coord,
        relays_from_strings(&data.relays)?,
    ))
}

// ─── Bare-key encode / decode ──────────────────────────────────────────────

/// Encode a public key hex string as an `npub` bech32 string.
#[must_use]
pub fn encode_npub(hex: &str) -> Result<String, Nip19Error> {
    // `PublicKey::to_bech32()` is infallible (`Err = Infallible`); the
    // `map_err` arm can never run, but D6 forbids `unreachable!` in nmp-core,
    // so degrade to a typed error rather than panic across the FFI seam.
    pubkey_from_hex(hex)?
        .to_bech32()
        .map_err(|_| Nip19Error::Bech32("npub encode".into()))
}

/// Decode an `npub` bech32 string to a hex public key.
#[must_use]
pub fn decode_npub(bech: &str) -> Result<String, Nip19Error> {
    require_hrp(bech, HRP_NPUB)?;
    PublicKey::from_bech32(bech)
        .map(|pk| pk.to_hex())
        .map_err(map_nostr_err)
}

/// Encode a private key hex string as an `nsec` bech32 string.
#[must_use]
pub fn encode_nsec(hex: &str) -> Result<String, Nip19Error> {
    require_hex64(hex)?;
    // `SecretKey::to_bech32()` is infallible; degrade rather than panic (D6).
    SecretKey::from_hex(hex)
        .map_err(|_| Nip19Error::InvalidHex)?
        .to_bech32()
        .map_err(|_| Nip19Error::Bech32("nsec encode".into()))
}

/// Decode an `nsec` bech32 string to a hex private key.
#[must_use]
pub fn decode_nsec(bech: &str) -> Result<String, Nip19Error> {
    require_hrp(bech, HRP_NSEC)?;
    SecretKey::from_bech32(bech)
        .map(|sk| sk.to_secret_hex())
        .map_err(map_nostr_err)
}

/// Encode an event id hex string as a `note` bech32 string.
#[must_use]
pub fn encode_note(hex: &str) -> Result<String, Nip19Error> {
    // `EventId::to_bech32()` is infallible; degrade rather than panic (D6).
    event_id_from_hex(hex)?
        .to_bech32()
        .map_err(|_| Nip19Error::Bech32("note encode".into()))
}

/// Decode a `note` bech32 string to a hex event id.
#[must_use]
pub fn decode_note(bech: &str) -> Result<String, Nip19Error> {
    require_hrp(bech, HRP_NOTE)?;
    EventId::from_bech32(bech)
        .map(|id| id.to_hex())
        .map_err(map_nostr_err)
}

// ─── nprofile ──────────────────────────────────────────────────────────────

/// Encode an `NprofileData` as an `nprofile` bech32m string.
#[must_use]
pub fn encode_nprofile(data: &NprofileData) -> Result<String, Nip19Error> {
    data_to_nprofile(data)?.to_bech32().map_err(map_nostr_err)
}

/// Decode an `nprofile` bech32m string into `NprofileData`.
#[must_use]
pub fn decode_nprofile(bech: &str) -> Result<NprofileData, Nip19Error> {
    require_hrp(bech, HRP_NPROFILE)?;
    Nip19Profile::from_bech32(bech)
        .map(|p| nprofile_to_data(&p))
        .map_err(map_nostr_err)
}

// ─── nevent ────────────────────────────────────────────────────────────────

/// Encode an `NeventData` as an `nevent` bech32m string.
#[must_use]
pub fn encode_nevent(data: &NeventData) -> Result<String, Nip19Error> {
    data_to_nevent(data)?.to_bech32().map_err(map_nostr_err)
}

/// Decode an `nevent` bech32m string into `NeventData`.
#[must_use]
pub fn decode_nevent(bech: &str) -> Result<NeventData, Nip19Error> {
    require_hrp(bech, HRP_NEVENT)?;
    Nip19Event::from_bech32(bech)
        .map(|e| nevent_to_data(&e))
        .map_err(map_nostr_err)
}

// ─── naddr ─────────────────────────────────────────────────────────────────

/// Encode an `NaddrData` as an `naddr` bech32m string.
#[must_use]
pub fn encode_naddr(data: &NaddrData) -> Result<String, Nip19Error> {
    data_to_ncoordinate(data)?
        .to_bech32()
        .map_err(map_nostr_err)
}

/// Decode an `naddr` bech32m string into `NaddrData`.
#[must_use]
pub fn decode_naddr(bech: &str) -> Result<NaddrData, Nip19Error> {
    require_hrp(bech, HRP_NADDR)?;
    Nip19Coordinate::from_bech32(bech)
        .map(|c| ncoordinate_to_data(&c))
        .map_err(map_nostr_err)
}

// ─── Top-level polymorphic parse / format ──────────────────────────────────

/// Parse any NIP-19 bech32 string into a typed `Nip19Entity`.
///
/// # Example
/// ```
/// use nmp_nip19::{parse, Nip19Entity};
///
/// let bech = "npub180cvv07tjdrrgpa0j7j7tmnyl2yr6yr7l8j4s3evf6u64th6gkwsyjh6w6";
/// let entity = parse(bech).unwrap();
/// assert!(matches!(entity, Nip19Entity::Npub(_)));
/// ```
#[must_use]
pub fn parse(bech: &str) -> Result<Nip19Entity, Nip19Error> {
    // Validate the separator first so a non-bech32 string surfaces as
    // `Bech32` (the variant callers match on for "not a NIP-19 string"),
    // mirroring the prior hand-rolled dispatcher.
    let hrp = hrp_of(bech)?;
    // Reject an unrecognised HRP up-front — before bech32 checksum
    // validation — so an unknown prefix always surfaces as `UnknownHrp(hrp)`
    // regardless of whether the body happens to be valid bech32 (matches the
    // prior hand-rolled dispatcher's prefix-first contract).
    if !matches!(
        hrp,
        HRP_NPUB | HRP_NSEC | HRP_NOTE | HRP_NPROFILE | HRP_NEVENT | HRP_NADDR
    ) {
        return Err(Nip19Error::UnknownHrp(hrp.to_string()));
    }
    match Nip19::from_bech32(bech) {
        Ok(Nip19::Pubkey(pk)) => Ok(Nip19Entity::Npub(pk.to_hex())),
        Ok(Nip19::Secret(sk)) => Ok(Nip19Entity::Nsec(sk.to_secret_hex())),
        Ok(Nip19::EventId(id)) => Ok(Nip19Entity::Note(id.to_hex())),
        Ok(Nip19::Profile(p)) => Ok(Nip19Entity::Nprofile(nprofile_to_data(&p))),
        Ok(Nip19::Event(e)) => Ok(Nip19Entity::Nevent(nevent_to_data(&e))),
        Ok(Nip19::Coordinate(c)) => Ok(Nip19Entity::Naddr(ncoordinate_to_data(&c))),
        #[allow(unreachable_patterns)]
        Ok(_) => Err(Nip19Error::UnknownHrp(hrp.to_string())),
        // `nostr` returns `WrongPrefix` for a recognised-bech32 / unknown-NIP19
        // HRP; surface the actual prefix the caller passed.
        Err(nostr::nips::nip19::Error::WrongPrefix) => Err(Nip19Error::UnknownHrp(hrp.to_string())),
        Err(e) => Err(map_nostr_err(e)),
    }
}

/// Format any `Nip19Entity` back to a bech32 string.
#[must_use]
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
mod tests;
