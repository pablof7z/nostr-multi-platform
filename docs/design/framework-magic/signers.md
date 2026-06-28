# Framework Magic §C11 — Signer Onboarding

> Parent: `docs/design/framework-magic.md`.
> Read first: `docs/product-spec/subsystems.md` §7.4 (sessions + signer catalog);
> `docs/design/kernel-substrate.md` (capabilities, actions, and composition);
> ADR-0015.

## C11. Signer onboarding: bunker:// + nsec creation as kernel actions

**Statement.** Two signer-onboarding flows are first-class kernel actions, complete from a single dispatched intent without any app-side orchestration:

1. **Bunker URL onboarding.** A pasted `bunker://...` URL parses into a `BunkerConnect` action; the action runs the NIP-46 rendezvous, establishes the remote-signer connection, persists the connection token via `KeyringCapability`, and emits an `Account` with `signer_kind = Nip46Bunker` into `SessionState.accounts`.
2. **Create new nsec.** A `CreateLocalIdentity { passphrase, label }` action generates a new keypair, encrypts the nsec via NIP-49 with the given passphrase, persists the encrypted nsec via `KeyringCapability`, and emits an `Account` with `signer_kind = LocalKey` into `SessionState.accounts`.

In both cases the new account becomes available to the active-session machinery (per C12) on a subsequent `SwitchActiveAccount` dispatch.

**Framework does:**

- The signer catalog at `subsystems.md` §7.4 names the supported account and signer kinds.
- `nmp-signers` owns signer implementations, account IDs, signer payloads, and bunker URI parsing; `nmp-signer-iface` holds operation types shared with `nmp-core`.
- `KeyringCapability` wraps macOS Keychain / Windows Credential Manager / Secret Service / Android Keystore. Capability calls report; they do not decide.
- The NIP-46 rendezvous flow is wrapped by Rust action/session code; progress and terminal state surface through action/projection state.
- The NIP-49 encryption is the `nostr` crate's `EncryptedSecretKey`; the framework wraps it as a step inside the `CreateLocalIdentity` action.

**App writes:** for **bunker**, dispatch the typed `BunkerConnect { url:
"bunker://..." }` action. For **create new nsec**, dispatch
`CreateLocalIdentity { passphrase, label }`. The action ledger row exposes
progress (parsing, rendezvous, awaiting user approval on the bunker app,
persisted, available); the app's UI renders the ledger row as a step indicator
if it wants, but the orchestration is the framework's. The app does **not** call
NIP-46 transport code, does **not** invoke NIP-49 encryption, does **not** touch
the Keychain directly, and does **not** wire the new identity into the session
state.

**Failure mode prevented:** the constellation of "DIY signer onboarding" bugs that every Nostr-on-mobile app re-discovers — leaked plaintext nsec in app state during the encryption window, lost bunker connection on app suspend, race between persistence and session activation, partial-failure leaving an `Account` in `SessionState` with no usable signer. Typed action/session state makes the "partial success" path explicit and recoverable.

**Test:** `c11_bunker_url_and_nsec_creation_complete_via_actions`. The test has two sub-paths against an in-memory `KeyringCapability` mock and a mock NIP-46 rendezvous endpoint:

1. **Bunker onboarding:**
   a. Dispatch `BunkerConnect { url: "bunker://abc?relay=wss%3A%2F%2Fmock&secret=xyz" }`.
   b. Mock rendezvous endpoint responds with a successful `connect` response.
   c. Assert the action ledger row transitions `Pending → Running(Parsing) → Running(Rendezvous) → Running(Persisting) → Completed { account_id }`.
   d. Assert `SessionState.accounts` contains one new `Account` with `signer_kind = Nip46Bunker`; the `KeyringCapability` mock has one stored entry keyed by the new account id.
   e. Assert no plaintext bunker secret crossed FFI (the test's reconciler audit log shows no `Account` snapshot field carrying the raw URL); only the typed `Account` + `signer_kind` enum.
2. **Create new nsec:**
   a. Dispatch `CreateLocalIdentity { passphrase: "test-passphrase", label: "alice" }`.
   b. Assert the action ledger row transitions `Pending → Running(Generating) → Running(Encrypting) → Running(Persisting) → Completed { account_id }`.
   c. Assert `SessionState.accounts` contains one new `Account` with `signer_kind = LocalKey`, `display.label = "alice"`; the `KeyringCapability` mock has one stored entry containing the NIP-49 ciphertext (the test inspects the mock's stored bytes — the prefix is `ncryptsec1`).
   d. Assert the plaintext nsec is **not** present in `SessionState`, in any view payload, in any diagnostic surface, or in the test's reconciler audit log. The plaintext exists only inside the actor's transient action state during encryption.
   e. Assert a follow-up `SwitchActiveAccount { account_id }` succeeds and that the actor can sign a test event using the newly-created identity (round-trip: dispatch a `SendNote` against the new account, observe a signed event in the action ledger before publish).

**Milestone owner:** **[PENDING M6]**. M6 is the signers + write-path milestone. M6 owner adds the framework-magic delta after the test goes green. Test checked in as `#[ignore = "pending M6 signers"]`.

## Why only these two onboarding paths

The full signer catalog at `subsystems.md` §7.4 lists five kinds:

- Local key (raw nsec, encrypted at rest) — **covered by C11 sub-path 2**.
- NIP-49 (password-encrypted) — **subsumed by C11 sub-path 2** (the NIP-49 encryption is the persistence step of the local-key creation, not a separate flow).
- NIP-46 bunker — **covered by C11 sub-path 1**.
- NIP-07 (web only) — browser support is documented in
  [`docs/wasm-surface.md`](../../wasm-surface.md#browser-signerprivate-flow-capability-model);
  it is not a native v1-ladder C11 contract bullet.
- External Android Amber via NIP-55 — wired via the `ExternalSignerCapability` (`kernel-substrate.md` §5); not a v1-ladder contract bullet because Android is M15.

C11 covers the two paths the user explicitly named for signer onboarding:
*"NIP-46 bunker:// URL parsing + connection flow"* and *"Create new nsec
flow. Generate, encrypt (NIP-49), and store via Keychain capability."* The
other signer kinds inherit the same account/session and capability-reporting
rules, but their onboarding flows have platform-specific surfaces that this
contract does not assert at this level.

A potential C11.b sibling bullet covering NIP-55 may be added in the M15
framework-magic delta. NIP-07 remains a browser-runtime capability surface, not
a native C11 onboarding path.

## The capability boundary

This bullet is a load-bearing demonstration of cardinal doctrine **D7** ("capabilities report; never decide policy" — `docs/product-spec/overview-and-dx.md` §1.5 D7). The KeyringCapability **reports** (here is the stored bytes; persistence succeeded/failed). It does **not decide** (whether to retry, whether to fall back to a different storage backend, whether to surface a UI prompt). The framework decides; the capability executes.

The test's assertion that no plaintext nsec crosses FFI is the structural
witness for **D6** ("errors never cross FFI as exceptions"; `overview-and-dx.md`
§1.5 D6) and for **D5** (bounded native state: the platform layer never sees
unencrypted key material; signing remains Rust-owned).

## Cross-references

- ADR-0015 — signer crate boundary and session ownership.
- `docs/design/kernel-substrate.md` — capability/action/projection substrate.
- `docs/product-spec/subsystems.md` §7.4 — `SessionState` + `Account` shapes.
- The `nostr-connect` and `nostr-keyring` crates (aim.md §3) — the protocol/OS primitives the framework composes.

## What this chapter does not cover

- **Account switching mechanics** — that's C12 in `sessions.md`.
- **Signing a publish** — the sign step inside `SendNoteAction` (C7). C11 covers onboarding; subsequent signing is the publish path.
- **Multi-device account sync** — out of v1 scope per `aim.md` §9.
- **Key-recovery and passphrase reset flows** — application-level UI on top of the framework primitives; not a contract bullet because these flows compose existing actions (delete identity, create new identity).
