---
title: Kernel Self-Identity REQ and Account Pubkey Fallback
slug: kernel-self-identity-req-and-account-pubkey
summary: The kernel's self-identity REQs (profile-target, target-relays) and the requested_profiles set use the active account's pubkey when available, falling back to T
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-18
updated: 2026-05-28
verified: 2026-05-18
compiled-from: conversation
sources:
  - session:09da8d90-44d5-4038-834b-5393adb0d2b9
  - session:575288b2-1197-44d2-ba9b-d72e8d74f9a6
  - session:fc128f85-af57-41cd-8c5b-a71d15450e17
  - session:5d180e52-7c43-4a99-bfc4-769eb40dc03f
  - session:7b4ae585-801c-441f-811d-5308e1002f08
  - session:64f3e239-c4c1-4c32-82de-458516b28418
  - session:8bd548b9-af6d-4108-bc64-16ebf8dfa4f7
---

# Kernel Self-Identity REQ and Account Pubkey Fallback

## Kernel Self-Identity REQs and Account Pubkey

The kernel's self-identity REQs (profile-target, target-relays) and the requested_profiles set use the active account's pubkey when available, falling back to TEST_PUBKEY when no account is active. The app must call open_primary_author at boot to instruct the kernel to fetch kind:0 for the primary pubkey from relays. The profile_card() function returns data for the active account when one is signed in, and falls back to TEST_PUBKEY/TEST_NPUB when no account is active. The logical_interests() diagnostic row tracks the active account's pubkey rather than always using TEST_PUBKEY. The KernelUpdate.test_npub FFI field remains static and cannot be made dynamic without an FFI type change. The startup request flow fetches only the self profile, self relay list, and self kind:3 contacts, returning empty if no account is signed in. When the active account's kind:3 (contacts) is ingested, the ingest_contacts function extracts follow p-tags but batch-claims their kind:0 profiles so followed users' profiles are prefetched on cold start rather than remaining as unresolved hex. Timeline opening (maybe_open_timeline) builds the author set solely from the active account's contacts, and gates opening (should_open_timeline) on the active account's kind:3 having arrived or a 3-second deadline. retarget_timeline emits a self-contacts REQ on sign-in or account switch so the timeline can open even when sign-in happens after startup. Sign-in, account creation, and account switching reconcile the M2 follow-feed and emit bootstrap REQs for the new active account so the follow feed works even when login happens after cold-start. Account-switch safety requires a drop_owner + set_sub pattern with a bootstrap_interest_ids tracker to avoid silently keeping the old pubkey in interest author filters. kernel.profiles is rehydrated from LMDB on cold start so that previously fetched profiles are immediately available without re-fetching from the wire. Cold-start profile fetching fans out across multiple indexers (purplepag.es, relay.nostr.band, kindpag.es) rather than only hitting purplepag.es.

<!-- citations: [^09da8-4] [^57528-10] [^fc128-3] [^5d180-2] [^7b4ae-5] [^64f3e-4] [^8bd54-2] -->
## See Also

