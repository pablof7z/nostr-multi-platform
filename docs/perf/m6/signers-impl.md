# M6 — Signers + Accounts impl phase 1

> Task #43 — `nmp-signers` crate landing.  Reports `cargo test`,
> `cargo clippy`, fuzz timings.  See `docs/decisions/0015-m6-signer-design.md`
> for the design rationale.

## Scope landed

- New crate `nmp-signers` (sibling to `nmp-core`, per D0).
- `Signer` trait + `SignerOp<T>` thunk type (no Tokio).
- `LocalKeySigner` — in-memory + NIP-49 ncryptsec round-trip.
- `Nip46Signer` + `Nip46SignerHandle` — bunker:// + pluggable
  `Nip46Transport` so the kernel can drive the live RPC, and tests can stub.
- `Nip07Signer` — wasm-target stub (returns `Unsupported` everywhere off-wasm;
  payload + trait shape stable for the wasm follow-up).
- `bunker://` URL parser — strict hex pubkey validation, percent-decoding,
  round-trip via `Display`.
- `AccountManager` — multi-account state with synchronous active-switch +
  mandatory id-precompute post-condition (applesauce SignerMismatchError).
- `Kind3RewireObserver` — captures active-account flips into a kernel-drained
  buffer for kind:3 + kind:10002 re-subscription.
- 37 tests total: 24 unit + 4 fuzz (incl. 1000-URI suite) + 9 integration.
- ADR-0015 documenting trait shape + synthesis reconciliation.

## Test results

```
$ cargo test -p nmp-signers
test result: ok. 24 passed; 0 failed; 0 ignored        (lib unit)
test result: ok.  4 passed; 0 failed; 0 ignored        (bunker_uri_fuzz)
test result: ok.  9 passed; 0 failed; 0 ignored        (integration)
```

Total wall-time: ~0.7 s on M-class Apple silicon.

## Clippy

```
$ cargo clippy -p nmp-signers --all-targets -- -D warnings
    Finished `dev` profile [unoptimized + debuginfo] target(s) in 1.39s
```

Clean.  Zero warnings, zero `-D warnings` errors.

## Fuzz harness

`tests/bunker_uri_fuzz.rs` — 1000 generated URIs per run, three test
functions:

- `fuzz_1000_uris_never_panics_and_round_trips` — main suite.  Generates
  well-formed + intentionally-malformed + adversarial inputs; asserts
  parser never panics, accepts well-formed, rejects known-invalid, and
  round-trips Display.
- `fuzz_adversarial_lengths` — 0-byte, 4 KiB, 40 KiB, unicode 1024-rune
  inputs; assert no panic.
- `fuzz_byte_noise_after_prefix` — 256 random byte-noise URIs after the
  `bunker://` prefix.

Timings (release-mode subset):

| Suite | Inputs | Wall-time | Per-URI |
|---|---:|---:|---:|
| `fuzz_1000_uris…` | 1000 | ~5 ms | 5 µs |
| `fuzz_adversarial_lengths` | 14 | sub-ms | — |
| `fuzz_byte_noise_after_prefix` | 256 | ~1 ms | 4 µs |

Per-URI cost is dominated by `url::Url::parse` on the relay URL; the bunker
URI shell itself parses in <1 µs.

## Doctrine compliance

- **D0** — `nmp-signers` is a new sibling crate; `nmp-core` stays free of
  signer materials.
- **D4** — `AccountManager.switch_active` flips the active id and notifies
  observers in a single synchronous critical section.  The kernel actor
  remains the single writer; the manager is held by the actor.
- **D6** — `Signer` trait operations return `SignerOp<T>` /
  `Result<_, SignerError>` internally only; no error type crosses FFI.
  The FFI surface (separate commit) will convert all errors to
  `toast: Option<String>` per ADR-0007.
- **D7** — `Nip46Signer` does not own the relay pool.  The transport is
  injected (`Arc<dyn Nip46Transport>`); the kernel applies routing policy.
- **D8** — `pubkey()` is synchronous and cached; the hot path never blocks
  on a remote round-trip to know who is active.  `SignerOp` resolves in
  Pending mode via the actor's existing `try_recv()` loop without per-tick
  allocations.

## Forward references

- ADR-0015 (this commit) — full design rationale, synthesis reconciliation,
  follow-ups.
- PD-004 — `IdentityId` → ULID before M8.
- `docs/plan/m6-signers-write.md` — milestone scope.
- `docs/plan/m5-nip42.md` — depends on `Signer` trait + `signer_active()`
  to route AUTH challenges.
- `docs/plan/m7-interaction-loop.md` — depends on this for `SendNote`.

## What's NOT in this commit (deferred per scope)

- `KeychainCapability` real iOS implementation.
- Live NIP-46 RPC subscription (kernel relay-pool integration).
- FFI action variants (`AddLocalAccount`, `AddBunkerAccount`, `SwitchActive`).
- `IdentityModule` registration into the existing `nmp-core::substrate`
  module registry.
- iOS login UX (paste nsec / paste bunker / generate new).
- M7 `SendNote` action wiring.

Each is a separate commit/task; this lands the trait + impls + manager
foundation that unblocks all of them.
