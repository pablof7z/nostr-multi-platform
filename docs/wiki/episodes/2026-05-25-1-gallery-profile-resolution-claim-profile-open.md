---
type: episode-card
date: 2026-05-25
session: 53838558-81bd-433d-a46d-d117ecebb361
transcript: /Users/pablofernandez/.claude/projects/-Users-pablofernandez-Work-nostr-multi-platform/53838558-81bd-433d-a46d-d117ecebb361.jsonl
salience: architecture
status: superseded
subjects:
  - gallery-profile-resolution
  - claim-profile-seam
  - snapshot-envelope-wire-contract
supersedes:
  - 2026-05-25-3-sharedsnapshot-envelope-parsing-root-cause-relay
related_claims: []
source_lines:
  - 6689-6698
  - 6908-6913
  - 6979-6981
captured_at: 2026-06-18T05:29:54Z
---

# Episode: Gallery profile resolution: claim_profile → open_author pivot + relay bootstrap + envelope decoder fix

## Prior State

Gallery used nmp_app_claim_profile to resolve profiles, which populates the kernel's internal profiles cache but does NOT project claim-only pubkeys onto the snapshot wire. Additionally, the gallery had no bootstrap relays at startup (empty app_relays set with no logged-in user → no kind:10002 → nowhere to send kind:0 fetches). The snapshot decoder did a decodeIfPresent-only direct parse that silently accepted the outer envelope {"t":"snapshot","v":…} as an empty GallerySnapshot, never reaching the envelope fallback path.

## Trigger

All 11 gallery screenshots showed 'Loading profile from relays…' spinner — real profiles never resolved. Validation agent confirmed nmp-ffi alone uses EmptyOutboxRouter (0 REQs in 30s) and that claim_profile has no snapshot-wire projection for non-authenticated consumers.

## Decision

Three-part fix in PR #567: (1) Seed 3 bootstrap relays (purplepag.es, relay.damus.io, nos.lol) at GalleryModel.start() before profile-interest calls; (2) Switch from nmp_app_claim_profile → nmp_app_open_author, which lands resolved ProfileCard under projections.author_view.profile; (3) Rewrite snapshot decoder to branch on the top-level "t" key in the envelope. Validation example moved from nmp-ffi to nmp-app-template because register_defaults is needed for the routing substrate.

## Consequences

- Real profiles resolve in ~750ms without a logged-in user; all 11 gallery screenshots now show pablof7z's real avatar, display name, and NIP-05 badge
- claim_profile remains semantically honest for 'render an arbitrary profile without signing in' but needs a future projections.claimed_profiles map to work end-to-end through the snapshot callback (noted as follow-up in PR)
- Any FFI consumer that needs relay routing must use nmp-app-template::register_defaults — bare nmp-ffi ships EmptyOutboxRouter which silently drops all REQs
- The kernel always wraps snapshots in {"t":"snapshot","v":…}; any future consumer decoder must check the envelope key first

## Open Tail

- Add projections.claimed_profiles map so claim_profile can surface resolved profiles through the snapshot wire without requiring open_author
- ABI mismatch: nmp_app_gallery_register was declared as void(void*) in the old header but is actually void(void) — drive-by fix in PR #567, may need broader header audit

## Evidence

- transcript lines 6689-6698
- transcript lines 6908-6913
- transcript lines 6979-6981

