---
title: NMP Relay Admission Policy & PrivateNetworkPolicy
slug: nmp-relay-admission-policy
summary: "Relay admission policy is distinct from per-account blocked relays (kind:10006-driven) because it is system-wide, URL-structural, and a security invariant rathe"
tags:
  - capture
volatility: warm
confidence: medium
created: 2026-05-27
updated: 2026-05-27
verified: 2026-05-27
compiled-from: conversation
sources:
  - session:3694d91c-5936-4527-bdd7-837a45b3610a
---

# NMP Relay Admission Policy & PrivateNetworkPolicy

## Purpose and Scope

Relay admission policy is distinct from per-account blocked relays (kind:10006-driven) because it is system-wide, URL-structural, and a security invariant rather than user-declared. [^3694d-1]


## Trait Definition

The `RelayAdmissionPolicy` trait defines a single method `fn is_admissible(&self, url: &str) -> bool` and lives in `nmp-router`, not `nmp-core`. [^3694d-2]

## Default Policy

The default admission policy implementation, `PrivateNetworkPolicy`, blocks loopback, RFC-1918, link-local, unspecified, and unparseable URLs. [^3694d-3]

## Lane Applicability

Relay admission checks are applied only to untrusted, network-sourced lanes 1 (NIP-65 mailbox), 2 (Hint), and 3 (Provenance). Operator-controlled lanes 4 (UserConfigured), 6 (Indexer), and 7 (AppRelay) are not subject to relay admission checks. [^3694d-4]

## Ownership and Configuration

The admission policy is owned by the `GenericOutboxRouter` struct via an `admission: Arc<dyn RelayAdmissionPolicy>` field, not by `RoutingContext`. `GenericOutboxRouter` provides a `with_admission_policy()` builder method to allow swapping or composing the default admission policy. [^3694d-5]
## See Also

