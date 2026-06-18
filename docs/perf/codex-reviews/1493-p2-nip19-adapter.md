# Codex review — fix/1493-p2-nip19-adapter (#1493)

Diff: rewrite `nmp-core/src/nip19.rs` from a hand-rolled bech32/TLV NIP-19
codec into a thin adapter over `nostr::nips::nip19` (nostr 0.44.3). NMP public
types + fn signatures preserved; hand-rolled internals + `Nip19Error::MissingField`
+ `bytes_to_hex` removed.

## Verdict: 2 blocking findings (BOTH ADDRESSED), no other blockers.

- BLOCKING (addressed): `u32` kinds above `u16::MAX` silently truncated on the
  `as u16` cast (`Some(65536)` → kind `0`). The NMP surface accepts `u32` but
  Nostr kinds are u16. Added `kind_to_nostr()` which rejects out-of-range kinds
  with a typed `Nip19Error::MalformedTlv`. Tests:
  `encode_nevent_rejects_kind_above_u16`, `encode_naddr_rejects_kind_above_u16`.
- BLOCKING (addressed): relay URLs / `naddr` identifier over 255 bytes would
  overflow the single-byte TLV length and produce non-round-trippable output.
  Added `MAX_TLV_VALUE_LEN` guards in `relays_from_strings` + `data_to_ncoordinate`,
  returning typed `MalformedTlv`. Tests: `encode_nprofile_rejects_oversized_relay`,
  `encode_naddr_rejects_oversized_identifier`.

Codex confirmed no blocker for: cross-HRP rejection (nprofile→decode_npub),
prefix-first `parse()` `UnknownHrp`, panic/`unreachable!` paths (D6-clean), or
`require_hex64`.

## Behaviour deltas (rust-nostr canonical; documented in tests)
- Event `kind` round-trips as u16 (0..=65535) — the real Nostr kind domain. The
  surface keeps `Option<u32>` for ergonomics; out-of-range now errors (was a
  4-byte-u32 TLV in the old hand-rolled codec).
- Relay URLs round-trip through `nostr::RelayUrl` normalisation.

## Verification
- `cargo test -p nmp-core --lib nip19` — 15/15.
- `cargo test -p nmp-core --test nip19_nip21` — 31/31; `--test nip21` — 17/17;
  `--test nip19_nip21_props` — 4/4; `--doc nip19` — 3/3.
- `cargo test -p nmp-ffi nip19` — 6/6. nmp-content / nmp-nip01 / nmp-blossom /
  nmp-nip47 build clean.
- `cargo test -p nmp-testing --test doctrine_lint_smoke` — 74/74 (D6 fix: the
  infallible bare-key encodes degrade to a typed `Err`, no `unreachable!`).
- File-size gate clean (the NIP-21 integration tests were split into a new
  `tests/nip21.rs` to keep `nip19_nip21.rs` under the hard-cap baseline).
