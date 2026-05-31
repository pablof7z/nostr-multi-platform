---
title: Relay Admission Policy
slug: relay-admission-policy
summary: A RelayAdmissionPolicy trait defines an is_admissible(url) -> bool method to determine whether a relay URL is permissible to connect to
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

# Relay Admission Policy

## Relay Admission Policy

A RelayAdmissionPolicy trait defines an is_admissible(url) -> bool method to determine whether a relay URL is permissible to connect to. The trait and its default PrivateNetworkPolicy implementation live in nmp-router, not nmp-core, keeping the substrate contract unmodified. [^3694d-1]


## Private Network Policy

The PrivateNetworkPolicy blocks loopback (127.x, ::1, localhost), RFC-1918 (10.x, 172.16-31.x, 192.168.x), link-local (169.254.x, fe80::), unspecified, and unparseable URLs. [^3694d-2]

## Policy Ownership and Configuration

The admission policy is owned by the GenericOutboxRouter struct via an admission: Arc<dyn RelayAdmissionPolicy> field rather than being carried on RoutingContext. The GenericOutboxRouter provides a with_admission_policy() builder method to allow swapping or composing the default PrivateNetworkPolicy (e.g., layering an operator deny-list). [^3694d-3]

## Lane Applicability

Relay admission checks apply only to untrusted, network-sourced lanes 1 (NIP-65 mailbox), 2 (Hint), and 3 (Provenance), and do not apply to operator-controlled lanes 4, 6, and 7. [^3694d-4]

## Architectural Boundaries

The BlockedRelayLookup (per-account, user-declared, kind:10006-driven) is a separate seam from the RelayAdmissionPolicy (system-wide, URL-structural, security-invariant) and the two must not be merged. [^3694d-5]
## See Also

