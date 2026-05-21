# Opus Direction Review #44 — 2026-05-21

## What actually shipped since #43

Nothing structural. The only commits after #43's review doc (`8996cdde`) are
four D0 doc-noun chores (`708f1f04`, `7f4ec43e`, `1e870af0`, `17e81b20`). The
review brief claims "a background agent is now building the real kind:10050
ingest." **It is not.** No `kernel/ingest/dm_relay_list.rs` exists,
`recipient_dm_relays` (`outbox.rs:172`) still returns `None`, and there is no
`10050` ingest arm. Trust the tree, not the framing: the kind:10050 gap is
fully open, identical to #43.

## Finding 1 — kind:10050 mis-routing still live (Critical)

**Observed:** `dm.rs:154-162` routes both gift-wrap envelopes to the *sender's*
Content relays via `bootstrap_urls_for_role(Content)`. NIP-17 §2 requires each
envelope to go to the *receiver's* kind:10050 list. The send "succeeds" with no
toast; if the recipient reads a different relay set, the DM is silently lost.
This is #42 Risk #1 and #43 Finding 1 — three cycles unaddressed.
**Do:** Land the kind:10050 ingest path. #43 templated it exactly (98-line
clone of `relay_list.rs`); do not re-design. Reject any PR titled "kind:10050"
that leaves `recipient_dm_relays` returning `None`.

## Finding 2 — ADR-0026 NIP-44 seam: cut it (High)

**Observed:** `nip44_encrypt`/`nip44_decrypt` have zero non-test callers in any
seal/unseal path (the `broker.rs` and `handle.rs` hits are trait
forwarders/impls, not consumers). The 2-cycle deadline is elapsed. **The seam
is also structurally insufficient:** `nmp_nip59::gift_wrap` takes `&Keys`
because the seal mints an *ephemeral* keypair and runs NIP-44 with that
ephemeral seckey, not the account key. A bunker exposes only one nip44 RPC
keyed to the *active* account — it cannot perform the ephemeral-pair ECDH. So
the missing piece is not "wire the seam"; it is refactoring `gift_wrap` to
accept an injected encrypter abstraction.
**Do:** Delete `nip44_encrypt`/`nip44_decrypt`, the `ArcRemoteSigner`
forwarders, and the `Nip46Signer` impl now. Re-introduce only alongside a
concrete `gift_wrap` signature change. Do not grant a third cycle.

## Finding 3 — Third Rust-owned key slot (High)

**Observed:** PR #136 added `NmpApp::nip17_local_keys` —
`Arc<Mutex<Option<nostr::Keys>>>` fed to `DmInboxProjection` (`inbox.rs:31`).
That is now a *third* key slot: IdentityRuntime canonical + `marmot_local_nsec`
+ `nip17_local_keys`, with 27 mirror sites across nmp-core. #43 said the
two-slot drift "has not worsened" — it did. Each new protocol crate currently
adds its own slot.
**Do:** Before any fourth consumer, design one typed key-access seam
(`active_local_keys()` already exists on `IdentityRuntime` — make projections
read *it*, not a co-maintained mirror).

## Finding 4 — Bunker DM gap still silent both directions (High)

**Observed:** `DmInboxSnapshot` has only `conversations` (`inbox.rs:110`); no
`unsupported` field. Remote-signer accounts get `{conversations: []}` forever —
no placeholder, no toast. Send side toasts (`dm.rs:82`); receive side is
silent. CI is green because every inbox test uses `Keys::generate()`.
**Do:** Bundle the one-line placeholder into the kind:10050 PR — emit
`{"conversations": [], "unsupported": "bunker accounts cannot yet decrypt
DMs"}` when signed in with no local keys. Purely additive; `DmListView`
already renders `conversations`.

## Finding 5 — Structural health (Medium / Low)

- **Chirp iOS LOC: 9,944**, not the 10,743 #43 reported. The project did *not*
  grow 8%; #43's count was wrong. The "growth on a broken foundation"
  discipline narrative does not hold this cycle — drop it. (Low)
- **DomainModule migration machinery** — still test-only callers, zero
  production driver. Cut candidate, not urgent. (Low)
- **D0 structural state: clean.** Zero app-crate imports in protocol crates.
  The doc sweep is genuinely complete. (Low)

## Q-answers

- **Q1:** Cut ADR-0026 now. Minimal missing piece is *not* the seam — it is a
  `gift_wrap` signature refactor to inject an encrypter. The seam as built can
  never satisfy NIP-59's ephemeral-key seal.
- **Q2:** Yes — additive `unsupported` field on `DmInboxSnapshot`. No D0/D3
  break: it is a placeholder in the type contract (D1), set from existing
  signed-in/no-local-key state the projection already observes.
- **Q3:** `dm_relay_lists` lives in `Kernel` (D3-correct). Keyed by hex
  pubkey, strict-`>` supersession like kind:10002, empty list removes the
  entry. No eviction needed — replaceables self-supersede.
- **Q4:** kind:10050 ingest. Reply threading and NIP-57 are moot while the
  primary DM path loses messages.
- **Q5:** The live anti-pattern is the third key slot (Finding 3), not doc
  comments. Otherwise the live paths are D0-clean.

## Bottom line

Three cycles, one Critical bug unmoved: NIP-17 DMs route to the wrong relays
and silently lose messages. ADR-0026's seam cannot fix the bunker case as
designed — cut it. The next PR is kind:10050 ingest + the one-line bunker
placeholder. Nothing else.
