---
type: episode-card
date: 2026-05-19
session: fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/fe79b2c4-3f04-4fc9-8dde-08f19a3190b4.jsonl
salience: product
status: active
subjects:
  - marmot-registration
  - active-local-nsec
  - create-account-flow
supersedes: []
related_claims: []
source_lines:
  - 12197-12235
  - 12779-12788
captured_at: 2026-06-18T04:30:47Z
---

# Episode: Marmot auto-registration after createAccount — nsec stays in Rust

## Prior State

After createAccount, the generated nsec (Keys::generate()) stayed inside the Rust actor thread and was never accessible to Swift, so MarmotService couldn't be initialized — the UI showed 'sign in with a key' for the encrypted groups feature on a brand-new account.

## Trigger

User observed the bug and directed: 'why does swift need to get the nsec? the mls stuff should also happen in the rust side, right?' — the architectural call was to keep the secret entirely in Rust and auto-register from there.

## Decision

Introduced active_local_nsec: Arc<Mutex<Option<String>>> slot in NmpApp. The actor writes the nsec synchronously before emitting identity-change snapshots (update_nsec_slot called before maybe_emit_after_dispatch in all identity arms). New nmp_app_chirp_marmot_register_active FFI reads the slot and delegates to register_with_keys(). Swift calls registerActive() with no secret; the key never crosses the C-ABI boundary.

## Consequences

- createAccount now auto-registers Marmot — no 'sign in with a key' dead end
- Dual registration paths: signInNsec → cachedSecretKey → registerIfNeeded(secretKey:), createAccount → registerActive() → Rust reads slot
- Race-free: slot is populated before the emit that triggers Swift's apply(), so by the time registerActive() runs the slot is guaranteed populated
- nmp-core has no app-specific naming — the generic slot is NmpApp::active_local_nsec(); the chirp-specific FFI function stays in nmp-app-chirp (D0 boundary preserved)

## Open Tail

- A fully automatic approach (actor notifies a registered hook when active_nsec changes, no Swift call at all) was discussed but deferred — current design still requires Swift to call registerActive() from apply()

## Evidence

- transcript lines 12197-12235
- transcript lines 12779-12788

