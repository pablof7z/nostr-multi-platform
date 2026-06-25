# ADR-0066 — Delegated NIP-44 decrypt sessions for bunker DM backfill

- **Status:** Accepted for staged implementation (2026-06-25).
- **Date:** 2026-06-25
- **Issue:** #1259.
- **Related:**
  - **ADR-0050** — landed the scalar signer port
    (`sign | nip44_encrypt | nip44_decrypt`) plus the bounded remote-signer DM
    fallback. This ADR decides the deferred §D7 capability.
  - **ADR-0031** — NMP owns the NIP-46 broker transport, so the verb set can grow
    without importing an upstream session manager.
  - **ADR-0048** — NIP-55/Amber remains a scalar external signer path unless a
    separate platform capability proves a batch primitive.
- **Doctrines touched:** D0 (open signer capability, no app noun in core), D4
  (one decrypt path with optional bulk acceleration), D6 (per-item errors are
  data), D7 (signer reports capability; kernel decides), D13 (no reusable secret
  export).

---

## Context

ADR-0050 made bunker DM receive structurally possible: the NIP-17 inbox no longer
holds raw keys, and each kind:1059 envelope unwraps through two scalar
`Nip44DecryptForAccount` port calls. That path is correct but expensive for
remote signers. Backfilling N envelopes needs two sequential NIP-44 decrypts per
envelope, so a scalar-only bunker backfill is O(2N) remote RPCs and may ask the
user to approve every decrypt.

ADR-0050 therefore landed a bounded fallback only: remote-signer accounts admit
a small number of in-flight decrypt chains, newest first, and surface
`decrypt_state` plus `undecrypted_count` when the policy cannot decrypt the full
backfill. That fallback is mandatory and remains the interop baseline.

The deferred question is whether NMP should add a bulk capability. NIP-46 names
standard methods such as `nip44_decrypt`, but it does not standardize a batch
decrypt session. NIP-55/Amber is an Android external signer contract, not a
NIP-46 extension point. Public support for this exact extension in nsec.app or
Amber is not verified, so the extension must be optional and negotiated.

## Decision

### D1 — Choose scoped session + batch decrypt, reject key export

NMP will add an optional NIP-46 extension made of a short-lived decrypt grant and
one or more batch decrypt calls:

1. `nmp_nip44_decrypt_session_begin`
2. `nmp_nip44_decrypt_batch`
3. `nmp_nip44_decrypt_session_end`

The extension **never exports NIP-44 conversation keys**. The signer receives
peer pubkeys and ciphertexts, decrypts inside the signer boundary, and returns
only plaintext or a per-item error. Reusable key export is rejected because it
would move long-lived secret material into the kernel/inbox path and create a
second secret-handling regime that ADR-0050 deliberately removed.

### D2 — Wire shape

NIP-46 method params stay JSON-RPC-compatible arrays with one object payload.

`nmp_nip44_decrypt_session_begin` params:

```json
[{
  "scope": "nmp.nip44.backfill",
  "requester_pubkey": "<account hex>",
  "max_items": 512,
  "expires_at": "<unix seconds>"
}]
```

Result:

```json
{
  "session_id": "<opaque signer token>",
  "max_batch_items": 128,
  "expires_at": "<unix seconds>"
}
```

`nmp_nip44_decrypt_batch` params:

```json
[{
  "session_id": "<opaque signer token>",
  "items": [
    {
      "id": "outer:event-id",
      "peer_pubkey": "<sender or seal pubkey hex>",
      "ciphertext": "<nip44 ciphertext>"
    }
  ]
}]
```

Result:

```json
{
  "items": [
    { "id": "outer:event-id", "plaintext": "<decrypted json>" },
    { "id": "outer:bad-id", "error": "decrypt_failed" }
  ]
}
```

`nmp_nip44_decrypt_session_end` params:

```json
[{ "session_id": "<opaque signer token>" }]
```

Result is `true` on best-effort cleanup. Session end failure is not fatal; the
signer owns expiry and must reject expired session ids.

### D3 — Interface shape

`nmp-signer-iface` will grow signer-owned request/result structs and an optional
batch/session method on `RemoteSignerHandle`. Non-capable signers return
`unsupported` by default. The types are cryptographic capability types only:
`scope` is an opaque string, not a NIP-17 type, so `nmp-core` remains free of
DM-specific vocabulary.

The NIP-46 implementation advertises support only after the session begin call
succeeds or a restored payload records a previously negotiated extension version.
NIP-55/Amber and unknown third-party bunkers keep using scalar
`nip44_decrypt`.

### D4 — Actor and inbox behavior

The actor gains a batch decrypt port sibling to the scalar sign commands. It
parks and times out like the ADR-0050 scalar port; completions still enter via
the waking actor mailbox. There is one decision point: ask the signer for the
batch capability, use it when available, and fall back to the scalar bounded
queue when it is not.

The NIP-17 inbox cannot treat the existing `undecrypted_count` counter as a
backfill source. The capable path must retain or replay raw kind:1059 candidate
events from the canonical event-log/read-model source, then drive them through
the batch session. A batch backfill parses outer plaintext first, builds the
second batch from verified kind:13 seals, and inserts kind:14 rumors under the
same active-account generation guard used by the scalar chain.

`decrypt_state` remains the user-facing policy surface:

- unsupported or failed negotiation: scalar fallback, possibly `limited`;
- batch-capable signer with complete candidate replay settled: `ok`;
- signer unavailable: `unavailable`.

### D5 — Interop matrix

| Signer/backend | Batch-session support | Required behavior |
|---|---:|---|
| NMP-controlled NIP-46 test signer/broker | Planned | Implement the extension and use it as the conformance oracle. |
| nsec.app bunker | Unverified | Negotiate; fall back to scalar bounded decrypt if unsupported. |
| Amber / NIP-55 | Not a NIP-46 extension target | Keep scalar external-signer behavior unless a separate platform batch capability exists. |
| Local keys | Not needed | Existing inline scalar port is fast enough; no second local decrypt mechanism. |

### D6 — Security and logging

Batch requests and results are secret-bearing. Logs may record counts, session
state transitions, and item ids, but never ciphertext, plaintext, conversation
keys, bearer tokens, or raw RPC bodies. Session ids are opaque signer secrets and
must not enter snapshots. Per-item failures are data; malformed responses fail
the batch and fall back or surface `limited` without panicking across FFI.

### D7 — Implementation sequence

This ADR does not close #1259. The remaining staged work is:

1. Add signer-interface structs, unsupported defaults, NIP-46 request/response
   mapping, extension-version persistence, and malformed-response tests.
2. Add the actor batch decrypt port, continuation/result type, deadline handling,
   and scalar fallback tests.
3. Add NIP-17 candidate replay/retention and session-aware backfill while keeping
   the existing scalar bounded queue untouched.
4. Add an NMP-controlled mock capable bunker conformance test and record the
   third-party interop results in the issue before closing it.

## Consequences

- A capable bunker can approve a scoped backfill session and decrypt batches
  without O(2N) interactive prompts.
- The mandatory scalar fallback keeps nsec.app, Amber, and other current signers
  working without extension support.
- The architecture stays simpler than key export: one signer-owned secret
  boundary, one actor port family, one inbox insertion path.
- #1259 remains open until the staged implementation proves full backfill can
  report `decrypt_state: ok` for a batch-capable bunker.
