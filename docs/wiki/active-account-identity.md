---
title: Active Account Identity & Fallback
slug: active-account-identity
summary: The kernel resolves identity from the active account rather than a hardcoded TEST_PUBKEY, falling back to TEST_PUBKEY only when no account is active
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-29
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:d27a4f61-511b-4086-845d-335493f9b464
  - session:fe79b2c4-3f04-4fc9-8dde-08f19a3190b4
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:47203d35-d7c9-4c12-bc47-a40773d7acc2
  - session:93c599f0-3aea-440a-9c42-1de6cd8771fe
  - session:cd2b6122-2b7c-43fc-941b-c51e79ffc691
  - session:855be2a2-4866-4d8d-ad4f-145309da56bc
  - session:4edd41f1-8318-4a4b-98d8-de01ae35f81b
---

# Active Account Identity & Fallback

## Active Account Identity

The kernel resolves identity from the active account rather than a hardcoded TEST_PUBKEY, falling back to TEST_PUBKEY only when no account is active. This applies across three surfaces: startup_requests() resolves the self-profile and self-relay-list REQs (kind:0 and kind:10002) from the active_account pubkey; profile_card() branches on active_account, using that pubkey without a static npub when an account is active and falling back to TEST_PUBKEY with TEST_NPUB otherwise; and the logical_interests() diagnostic row in status.rs uses active_account.as_deref().unwrap_or(TEST_PUBKEY) to display the Profile entry for the signed-in user. The register.rs component falls back to an empty Pubkey when no account is active, resulting in silent anonymous mode rather than a TEST_PUBKEY fallback. When the active account's kind:3 contacts are ingested, the kernel must batch-claim kind:0 profiles for every pubkey in the follow set. New accounts on all platforms autofollow two hardcoded pubkeys (the user and fiatjaf) via DEFAULT_FOLLOWS in identity.rs. DM inbox and group chat observer registration is idempotent, preventing observer leaks on repeated sign-in. NmpApp holds an `active_local_nsec: Arc<Mutex<Option<String>>>` shared slot that the actor writes synchronously before emitting identity-change snapshots. The `create-account` REPL command accepts optional relay URLs as trailing arguments (e.g., `create-account alice wss://relay.primal.net`) to set app relays inline. The `a` key opens an account switcher overlay; pressing Enter on a selected account calls runtime.switch_account() to actually switch. The local secret-key path serves as the working identity mechanism for the interim. The remote signer NIP-46 / kernel keys provider (Seam C) is explicitly deferred as post-Marmout work. The sign_in_bunker function is a stub at identity.rs:388-408 that never progresses past the connecting state. The test_npub FFI field in KernelUpdate remains a static string and cannot be made dynamic without a FFI type change. V-90 Site 3 (cold-start signs off-actor) was a misdiagnosis: create_account always activates a fresh local key before its cold-start signs, so active_remote() is provably None and the blocking .wait() branch is never taken. Debug asserts at the three create_account sign sites enforce the local-key invariant that the whole V-90 Site-3 correction rests on. A remote signer cannot stall the actor during account creation, confirming V-54 as a non-bug that was removed from the backlog.

<!-- citations: [^09da8-1] [^57528-2] [^d27a4-1] [^fe79b-1] [^7b4ae-1] [^47203-1] [^93c59-1] [^cd2b6-1] [^855be-1] [^4edd4-1] -->
## See Also

