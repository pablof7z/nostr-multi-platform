---
title: Red CI Merges to Master — Pattern and Prevention
slug: red-ci-merges-to-master
summary: PRs merged with red CI cause downstream build breaks. PR #789 broke the iOS build by shipping a duplicate protocol declaration and unpatched call sites. Always verify the build succeeds before merging any PR.
tags:
  - ci
  - process
  - ios
  - build-break
volatility: warm
confidence: medium
created: 2026-05-29
updated: 2026-05-29
verified: 2026-05-29
compiled-from: conversation
sources:
  - session:38935d82-0cbf-4e85-98d3-a0f056fd450c
---

# Red CI Merges to Master — Pattern and Prevention

> PRs merged with red CI cause downstream build breaks. PR #789 broke the iOS build by shipping a duplicate protocol declaration and unpatched call sites. Always verify the build succeeds before merging any PR.

## Pattern

PRs merged to master with red CI (failing continuous integration checks) cause downstream breakage because subsequent work assumes the merged code compiles and passes tests. The iOS project was uncompilable after PR #789 merged with red CI — it introduced a duplicate NostrProfileHost protocol declaration and left 11 ChirpAvatar call sites unpatched after changing the init signature. [^38935-25]

## Example: PR #789

PR #789 merged to master despite CI failures. It added NostrProfileHost.swift as a new file but left the same protocol definition inline in ProfileWire.swift, causing an invalid redeclaration compile error. It also updated ChirpAvatar.init to require a pubkey: String parameter but left all 11 call sites unpatched, causing missing argument for parameter 'pubkey' in call errors at every avatar usage site. The iOS project did not compile from the moment PR #789 landed until PR #794 fixed both regressions. [^38935-26]


PR #794 fixed both regressions from PR #789 in a single PR with two commits: (1) remove duplicate NostrProfileHost / EnvironmentValues from ProfileWire.swift, (2) add pubkey: to all 11 ChirpAvatar call sites, each with the contextually correct pubkey (item.authorPubkey, conversation.peerPubkey, message.senderPubkeyHex, etc.). The fixes were clean surgical corrections — no scope creep. After #794 merged, the iOS project compiled again. [^38935-39]
## Detection

The build breaks were detected by attempting a build after the merge. The two errors — redeclaration of NostrProfileHost and missing pubkey: arguments — were both immediately visible in the Swift compiler output. A clean build of the iOS target would have caught both before merge. [^38935-27]

## Prevention

Per the never-merge-on-pending-cargo-test rule, no PR should merge while tests show pending or failing. For iOS specifically, a successful Xcode build of the Chirp target should be required before merging any PR that touches Swift source files. [^38935-28]

## See Also
- [[chirp-ios-avatar-profile-lifecycle|Chirp iOS Avatar and Profile Lifecycle — NostrProfileHost Gap]] — related guide
- [[never-merge-on-pending-cargo-test|Never Merge on Pending cargo test — Cross-Crate Suite Is Mandatory]] — related guide

