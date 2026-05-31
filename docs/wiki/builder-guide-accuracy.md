---
title: Builder Guide Section Accuracy Verification
slug: builder-guide-accuracy
summary: The doctrine, actor, reactivity, FFI, iOS, and testing sections (§00, §01, §03, §04, §06, §16–18, §21–22, §25–27) are accurate and grounded in real code.
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-30
updated: 2026-05-30
verified: 2026-05-30
compiled-from: conversation
sources:
  - session:c3f757f1-6292-4e52-b520-5bb52e7de2bf
  - session:b6578d9e-697f-41ae-ab75-5e5643ceff13
---

# Builder Guide Section Accuracy Verification

## Section Accuracy

The builder guide at /Users/pablofernandez/Work/nostr-multi-platform/docs/builder-guide/ is the only source of truth for the framework API. Source code under crates/, apps/, or ios/ in the NMP repository must not be read. The doctrine, actor, reactivity, FFI, iOS, and testing sections (§00, §01, §03, §04, §06, §16–18, §21–22, §25–27) are accurate and grounded in real code.

<!-- citations: [^c3f75-6] [^b6578-1] -->

## Review Goals and Report

The primary goal is to compile a working binary, and secondarily to report on the builder guide quality. A final report must be written to /tmp/nostr-broadcast/BUILDER_GUIDE_REVIEW.md. The report must explain what worked well, what was confusing or missing, what had to be guessed or inferred, and whether the builder guide was sufficient to build the app. [^b6578-2]

## Reading Order

The builder guide's Path A reading order is 00 → 01 → 02 → 05a → 05b → 11 → 12 → 19a → 19b → 26. [^b6578-3]

## Coverage Gaps

The builder guide sections 00 through 26 do not cover how to create an NmpApp from a Rust binary, how to generate a keypair via nmp_app_create_new_account, how to start relay connections via nmp_app_start, or how to dispatch actions without an iOS/Android shell. [^b6578-4]
