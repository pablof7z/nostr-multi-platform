#!/usr/bin/env bash
#
# check-ffi-surface-freeze.sh — CI gate for the C-ABI surface freeze.
#
# The seam migration doctrine (ADR-0027 direction) requires that ALL new app
# verbs are exposed through `dispatch_action("nmp.X.Y", json)` — NOT as new
# per-verb `#[no_mangle] pub extern "C" fn nmp_app_*` exports. KernelBridge.swift
# is already 1,800 LOC of hand-written Swift mirrors; every new per-verb export
# doubles the cost (Rust variant + Swift mirror) and becomes a permanent ABI
# promise to the App Store binary. The seam migration rate is ~5 verbs/quarter;
# adding new per-verb exports widens the gap and makes v1 drift further away.
#
# WHAT THIS SCRIPT CHECKS — diff-based, PR-only:
#   Given BASE..HEAD, extract lines in the diff that are:
#     +pub extern "C" fn nmp_app_   (new per-verb Rust export)
#   minus lines that are:
#     -pub extern "C" fn nmp_app_   (deleted per-verb Rust export)
#   If NET additions > 0: fail and list the new symbols.
#
# EXEMPTIONS:
#   Renames and relocations that delete one symbol and add another (same net)
#   are allowed — the diff will show -1 and +1, net 0. Genuine new surface
#   requires an ADR approval; reference it in the commit message with
#   "ADR-XXXX: <title>" and add an `# adr-override: ADR-XXXX` comment in
#   this script once the ADR is merged.
#
# INVOCATION:
#   check-ffi-surface-freeze.sh <BASE_SHA> <HEAD_REF>
#
#   BASE_SHA: the merge-base or PR base commit SHA
#   HEAD_REF: the PR head ref (e.g. "pr-head" after `git fetch origin pull/N/head:pr-head`)
#
# The workflow passes both from the pull_request_target event context.

set -euo pipefail

BASE_SHA="${1:-}"
HEAD_REF="${2:-}"

if [[ -z "${BASE_SHA}" || -z "${HEAD_REF}" ]]; then
    echo "Usage: $0 <BASE_SHA> <HEAD_REF>" >&2
    exit 1
fi

# Collect added and removed per-verb exports from the diff.
# Pattern: lines starting with + or - (not ++) followed by
# `pub extern "C" fn nmp_app_`.
ADDED="$(git diff "${BASE_SHA}...${HEAD_REF}" -- \
    'crates/nmp-core/src/ffi/' \
    'crates/nmp-signer-broker/src/' \
    'apps/chirp/nmp-app-chirp/src/' \
    | grep -E '^\+pub extern "C" fn nmp_app_' \
    | sed 's/^+//' \
    | grep -oE 'fn nmp_app_[a-zA-Z0-9_]+' \
    | sed 's/^fn //' \
    | sort -u || true)"

REMOVED="$(git diff "${BASE_SHA}...${HEAD_REF}" -- \
    'crates/nmp-core/src/ffi/' \
    'crates/nmp-signer-broker/src/' \
    'apps/chirp/nmp-app-chirp/src/' \
    | grep -E '^\-pub extern "C" fn nmp_app_' \
    | sed 's/^-//' \
    | grep -oE 'fn nmp_app_[a-zA-Z0-9_]+' \
    | sed 's/^fn //' \
    | sort -u || true)"

# Net new: added but not removed.
NET_NEW="$(comm -23 \
    <(printf '%s\n' "${ADDED}") \
    <(printf '%s\n' "${REMOVED}") \
    | grep -v '^$' || true)"

if [[ -n "${NET_NEW}" ]]; then
    echo "" >&2
    echo "C-ABI SURFACE FREEZE VIOLATION" >&2
    echo "================================" >&2
    echo "" >&2
    echo "This PR adds new per-verb nmp_app_* C exports:" >&2
    while IFS= read -r sym; do
        [[ -z "${sym}" ]] && continue
        echo "  + ${sym}" >&2
    done <<< "${NET_NEW}"
    echo "" >&2
    echo "The C-ABI surface is frozen. All new app verbs MUST go through:" >&2
    echo "  dispatch_action(\"nmp.X.Y\", json_payload)" >&2
    echo "" >&2
    echo "Rationale: KernelBridge.swift is already ~1,800 LOC of hand-written" >&2
    echo "Swift mirrors. Each new per-verb export adds a Rust variant + a Swift" >&2
    echo "mirror + a C declaration — tripling the maintenance surface — and" >&2
    echo "becomes a permanent ABI promise once it ships to the App Store." >&2
    echo "" >&2
    echo "To add a new app verb:" >&2
    echo "  1. Register an ActionModule in apps/chirp/nmp-app-chirp/src/ffi.rs" >&2
    echo "  2. Implement ActionModule::execute in the appropriate nmp-nip* crate" >&2
    echo "  3. Call dispatch_action(\"nmp.X.Y\", ...) from Swift" >&2
    echo "" >&2
    echo "If you believe a new nmp_app_* export is genuinely required (e.g. a" >&2
    echo "lifecycle hook with no dispatch analogue), write an ADR and reference" >&2
    echo "it in your commit message as 'ADR-XXXX: <title>'." >&2
    echo "" >&2
    exit 1
fi

ADDED_COUNT="$(printf '%s\n' "${ADDED}" | grep -c . || true)"
REMOVED_COUNT="$(printf '%s\n' "${REMOVED}" | grep -c . || true)"

if [[ "${ADDED_COUNT}" -gt 0 ]]; then
    echo "ffi-surface-freeze: OK — ${ADDED_COUNT} symbol(s) renamed/relocated (net 0)."
else
    echo "ffi-surface-freeze: OK — no new nmp_app_* per-verb exports."
fi
exit 0
