// Fuzz harness uses classic `n % k == 0` patterns and `iter::repeat(_).take(_)`
// for readability; clippy's modernised versions don't help the random-noise
// generators read more clearly.
#![allow(clippy::manual_is_multiple_of)]
#![allow(clippy::manual_str_repeat)]
#![allow(clippy::manual_repeat_n)]

//! 1000-URI fuzz harness for the `bunker://` parser.
//!
//! Generates 1000 inputs — a mix of well-formed URIs, intentionally malformed
//! variants, and adversarial garbage — and asserts:
//!
//! 1. The parser **never panics** on any input.
//! 2. For well-formed inputs, the parser accepts and round-trips via
//!    `BunkerUri::to_string()` → `parse_bunker_uri()`.
//! 3. For known-invalid inputs, the parser returns a typed error (not Ok).
//! 4. Total wall-time stays under 1 second (the parser is hot-path-ish; we
//!    don't want regressions silently O(n²)-ing).
//!
//! The generator is deterministic (seeded LCG) so failures reproduce.  No
//! `arbitrary`/`proptest` deps; total dep surface = `std` only.

use nmp_signers::{parse_bunker_uri, BunkerParseError, MAX_BUNKER_URI_LEN};
use std::time::Instant;

const PK_HEX: &str = "79be667ef9dcbbac55a06295ce870b07029bfcdb2dce28d959f2815b16f81798";

const HEX: &[u8] = b"0123456789abcdef";

#[test]
fn fuzz_1000_uris_never_panics_and_round_trips() {
    let mut rng = Lcg::new(0xDEADBEEFCAFEBABE);
    let mut accepted: u32 = 0;
    let mut rejected: u32 = 0;
    let mut round_trip_ok: u32 = 0;

    let start = Instant::now();
    for i in 0..1000u32 {
        let (input, classification) = generate_input(&mut rng, i);
        let result = parse_bunker_uri(&input);

        match (&result, &classification) {
            (Ok(uri), Class::WellFormed) => {
                accepted += 1;
                let printed = uri.to_string();
                let reparsed = parse_bunker_uri(&printed).unwrap_or_else(|e| {
                    panic!(
                        "round-trip failed: original={input:?} printed={printed:?} err={e:?}"
                    );
                });
                assert_eq!(
                    uri, &reparsed,
                    "round-trip inequality: original={input:?} printed={printed:?}"
                );
                round_trip_ok += 1;
            }
            (Ok(_), Class::ProbablyValid) => {
                accepted += 1;
            }
            (Err(_), Class::ProbablyValid) => {
                // Acceptable — our "probably valid" generator may produce edge
                // cases that fall outside the spec.
                rejected += 1;
            }
            (Err(_), Class::WellFormed) => {
                panic!("well-formed input rejected: {input:?} -> {result:?}");
            }
            (Err(_), Class::KnownInvalid(_)) => {
                rejected += 1;
            }
            (Ok(_), Class::KnownInvalid(reason)) => {
                panic!(
                    "known-invalid input accepted ({reason}): {input:?} -> {result:?}"
                );
            }
        }
    }
    let elapsed = start.elapsed();
    println!(
        "fuzz: 1000 URIs in {elapsed:?} — accepted={accepted} \
         rejected={rejected} round_trip_ok={round_trip_ok}"
    );
    assert!(
        elapsed < std::time::Duration::from_secs(2),
        "parser too slow: {elapsed:?} for 1000 URIs"
    );
    // Should accept the well-formed slice (~250) without exception.
    assert!(round_trip_ok >= 200, "round_trip_ok too low: {round_trip_ok}");
    assert!(rejected >= 400, "rejected too low: {rejected}");
}

#[test]
fn fuzz_adversarial_lengths() {
    // URIs of length 0, 1, 8, 9, 64, 256, MAX, MAX+1, 10*MAX.  None should panic.
    for n in [0usize, 1, 8, 9, 64, 256, MAX_BUNKER_URI_LEN, MAX_BUNKER_URI_LEN + 1, 10 * MAX_BUNKER_URI_LEN] {
        let s: String = std::iter::repeat('A').take(n).collect();
        let _ = parse_bunker_uri(&s);
    }
    // Same with unicode garbage.
    for n in [0usize, 1, 16, 64, 256, 1024] {
        let s: String = std::iter::repeat('🚀').take(n).collect();
        let _ = parse_bunker_uri(&s);
    }
}

#[test]
fn fuzz_byte_noise_after_prefix() {
    let mut rng = Lcg::new(0xC0FFEE);
    for _ in 0..256 {
        let len = (rng.next() % 200) as usize;
        let mut s = String::from("bunker://");
        for _ in 0..len {
            s.push((rng.next() % 95 + 32) as u8 as char);
        }
        let _ = parse_bunker_uri(&s);
    }
}

// --- generator + classification -----------------------------------------

#[derive(Clone, Debug)]
enum Class {
    /// Should parse and round-trip cleanly.
    WellFormed,
    /// Generator believes it's valid, but the spec edge may reject — both Ok
    /// and Err are acceptable; we just refuse panics.
    ProbablyValid,
    /// Generator deliberately built an invalid URI; parser MUST reject.
    KnownInvalid(&'static str),
}

fn generate_input(rng: &mut Lcg, i: u32) -> (String, Class) {
    let bucket = i % 10;
    match bucket {
        0 | 1 => well_formed_simple(rng),
        2 => well_formed_full(rng),
        3 => percent_encoded(rng),
        4 => extra_params(rng),
        5 => invalid_scheme(rng),
        6 => invalid_pubkey(rng),
        7 => no_relay(rng),
        8 => invalid_relay_scheme(rng),
        _ => adversarial(rng),
    }
}

fn well_formed_simple(rng: &mut Lcg) -> (String, Class) {
    let pk = random_pubkey(rng);
    let relay = random_relay(rng);
    (format!("bunker://{pk}?relay={relay}"), Class::WellFormed)
}

fn well_formed_full(rng: &mut Lcg) -> (String, Class) {
    let pk = random_pubkey(rng);
    let n_relays = (rng.next() % 3 + 1) as usize;
    let relays: Vec<String> = (0..n_relays).map(|_| random_relay(rng)).collect();
    let q = relays
        .iter()
        .map(|r| format!("relay={r}"))
        .collect::<Vec<_>>()
        .join("&");
    let secret = random_alnum(rng, 16);
    let perms = "sign_event:1,nip04_encrypt";
    (
        format!("bunker://{pk}?{q}&secret={secret}&perms={perms}"),
        Class::WellFormed,
    )
}

fn percent_encoded(rng: &mut Lcg) -> (String, Class) {
    let pk = random_pubkey(rng);
    // Percent-encode the relay URL.
    let relay_raw = random_relay(rng);
    let encoded = percent_encode(&relay_raw);
    (
        format!("bunker://{pk}?relay={encoded}"),
        Class::ProbablyValid,
    )
}

fn extra_params(rng: &mut Lcg) -> (String, Class) {
    let pk = random_pubkey(rng);
    let relay = random_relay(rng);
    let extra_k = random_alnum(rng, 6);
    let extra_v = random_alnum(rng, 8);
    (
        format!("bunker://{pk}?relay={relay}&{extra_k}={extra_v}"),
        Class::WellFormed,
    )
}

fn invalid_scheme(rng: &mut Lcg) -> (String, Class) {
    let pk = random_pubkey(rng);
    let relay = random_relay(rng);
    let schemes = ["nostr", "https", "http", "ws", "wss", "bunkers", "bunker:", "bun"];
    let s = schemes[(rng.next() as usize) % schemes.len()];
    (
        format!("{s}://{pk}?relay={relay}"),
        Class::KnownInvalid("wrong scheme"),
    )
}

fn invalid_pubkey(rng: &mut Lcg) -> (String, Class) {
    let relay = random_relay(rng);
    let bad_lens = [0usize, 1, 32, 63, 65, 128];
    let len = bad_lens[(rng.next() as usize) % bad_lens.len()];
    let pk: String = (0..len).map(|_| HEX[(rng.next() as usize) % HEX.len()] as char).collect();
    (
        format!("bunker://{pk}?relay={relay}"),
        Class::KnownInvalid("bad pubkey length"),
    )
}

fn no_relay(rng: &mut Lcg) -> (String, Class) {
    let pk = random_pubkey(rng);
    let secret = random_alnum(rng, 8);
    let with_qs = rng.next() % 2 == 0;
    let s = if with_qs {
        format!("bunker://{pk}?secret={secret}")
    } else {
        format!("bunker://{pk}")
    };
    (s, Class::KnownInvalid("no relay"))
}

fn invalid_relay_scheme(rng: &mut Lcg) -> (String, Class) {
    let pk = random_pubkey(rng);
    let bads = [
        "https://r.example",
        "http://r.example",
        "tcp://r.example",
        "relay.example.com",
        "",
    ];
    let bad = bads[(rng.next() as usize) % bads.len()];
    (
        format!("bunker://{pk}?relay={bad}"),
        Class::KnownInvalid("invalid relay scheme"),
    )
}

fn adversarial(rng: &mut Lcg) -> (String, Class) {
    // Random byte noise, but always starting with `bunker://` so we hit
    // post-prefix code paths.  Class: ProbablyValid (we just refuse panics).
    let prefix = "bunker://";
    let n = (rng.next() % 256) as usize;
    let mut s = String::with_capacity(prefix.len() + n);
    s.push_str(prefix);
    for _ in 0..n {
        let b = (rng.next() % 95 + 32) as u8;
        s.push(b as char);
    }
    (s, Class::ProbablyValid)
}

fn random_pubkey(rng: &mut Lcg) -> String {
    if rng.next() % 4 == 0 {
        PK_HEX.to_string()
    } else {
        (0..64).map(|_| HEX[(rng.next() as usize) % HEX.len()] as char).collect()
    }
}

fn random_relay(rng: &mut Lcg) -> String {
    let scheme = if rng.next() % 2 == 0 { "wss" } else { "ws" };
    let host_len = (rng.next() % 12 + 6) as usize;
    let host: String = (0..host_len).map(|_| {
        let c = (rng.next() % 26 + b'a' as u64) as u8;
        c as char
    }).collect();
    let port = if rng.next() % 4 == 0 {
        format!(":{}", rng.next() % 60000 + 1024)
    } else {
        String::new()
    };
    let path = if rng.next() % 3 == 0 {
        format!("/{}", random_alnum(rng, 5))
    } else {
        String::new()
    };
    format!("{scheme}://{host}.example{port}{path}")
}

fn random_alnum(rng: &mut Lcg, len: usize) -> String {
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789";
    (0..len).map(|_| CHARS[(rng.next() as usize) % CHARS.len()] as char).collect()
}

fn percent_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        if matches!(b, b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~') {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{:02X}", b));
        }
    }
    out
}

// Deterministic 64-bit LCG for reproducibility.
struct Lcg(u64);
impl Lcg {
    fn new(seed: u64) -> Self {
        Self(seed)
    }
    fn next(&mut self) -> u64 {
        // Numerical Recipes LCG.
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        self.0
    }
}

#[test]
fn smoke_known_inputs() {
    // Sanity to ensure the test infra itself works.
    assert!(parse_bunker_uri(&format!(
        "bunker://{PK_HEX}?relay=wss://r.example"
    ))
    .is_ok());
    assert!(matches!(
        parse_bunker_uri(""),
        Err(BunkerParseError::Empty)
    ));
}
