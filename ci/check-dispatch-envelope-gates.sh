#!/usr/bin/env bash
#
# ADR-0064 / S2 (#1750) — DispatchEnvelope fail-closed CI gates.
#
# Static, fail-closed invariants over the write-command byte transport. The
# *runtime* rejection behaviors (schema_version tripwire, oversize bound,
# file-identifier, missing routing fields) are proven by the Rust round-trip
# trip-tests in
#   crates/nmp-core/src/transport/dispatch_envelope_tests.rs
# (each asserts the NEGATIVE — the bad case is rejected). This script guards the
# SHAPE those tests rely on, so the schema can never silently drift out from
# under them:
#
#   G1  file_identifier        — the envelope keeps its own `NMPD` magic (so the
#                                byte doorway fails closed on a wrong root).
#   G2  schema_version field   — the tripwire field exists in the schema.
#   G3  opaque [ubyte] payload — `payload` stays an opaque byte vector, NOT a
#                                union (the open ActionModule seam, ADR-0064 §2).
#   G4  max byte-size bound    — a `MAX_DISPATCH_ENVELOPE_BYTES` constant exists
#                                and gates decode (the oversize fail-closed gate).
#   G5  namespace uniqueness   — no two registered `ActionModule`s declare the
#                                same `const NAMESPACE` (the routing key the
#                                envelope's `action_namespace` resolves against
#                                must be unambiguous).
#
# Each gate has a `--self-test` proof that it TRIPS on the bad case (asserts the
# negative), satisfying the issue's "proven to trip" requirement for a CI gate.
#
# Usage:
#   ci/check-dispatch-envelope-gates.sh
#   ci/check-dispatch-envelope-gates.sh --self-test

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

SCHEMA="${REPO_ROOT}/crates/nmp-core/schema/dispatch_envelope.fbs"
DECODE_RS="${REPO_ROOT}/crates/nmp-core/src/transport/dispatch_envelope.rs"

fail() {
    echo "dispatch-envelope-gates: FAIL — $1" >&2
    exit 1
}

# G1 — the envelope declares its own file identifier "NMPD".
gate_file_identifier() {
    local schema="$1"
    grep -Eq 'file_identifier[[:space:]]+"NMPD"' "${schema}" \
        || fail "G1 file_identifier: \"NMPD\" not declared in ${schema#${REPO_ROOT}/}"
}

# G2 — the schema_version tripwire field exists.
gate_schema_version() {
    local schema="$1"
    grep -Eq '^[[:space:]]*schema_version[[:space:]]*:[[:space:]]*uint' "${schema}" \
        || fail "G2 schema_version: tripwire field missing in ${schema#${REPO_ROOT}/}"
}

# G3 — payload is an opaque [ubyte] vector, NOT a union. A `union` keyword on the
# payload (or a `payload:` typed as anything other than `[ubyte]`) closes the
# open ActionModule seam and is rejected.
gate_opaque_payload() {
    local schema="$1"
    grep -Eq '^[[:space:]]*payload[[:space:]]*:[[:space:]]*\[ubyte\]' "${schema}" \
        || fail "G3 opaque payload: payload is not an opaque [ubyte] vector in ${schema#${REPO_ROOT}/}"
    if grep -Eq '^[[:space:]]*union[[:space:]]' "${schema}"; then
        fail "G3 opaque payload: a union is declared — the payload must stay opaque (ADR-0064 §2)"
    fi
}

# G4 — the oversize bound constant exists and is wired into the decode path.
gate_max_bytes() {
    local decode="$1"
    grep -Eq 'MAX_DISPATCH_ENVELOPE_BYTES' "${decode}" \
        || fail "G4 max byte-size bound: MAX_DISPATCH_ENVELOPE_BYTES missing in ${decode#${REPO_ROOT}/}"
}

# G5 — namespace uniqueness across all registered ActionModule declarations.
# Collect every `const NAMESPACE: ... = "...";` literal under crates/ and assert
# no duplicate. View namespaces (`pub const NAMESPACE`) are read-side projection
# keys, not write routing keys, so this scopes to the non-`pub` ActionModule
# `const NAMESPACE` (the dispatch authority's keys).
gate_namespace_uniqueness() {
    local root="$1"
    local dupes
    # Exclude test sources: fixture modules deliberately reuse a namespace to
    # exercise the registry's own dedup (ADR-0049). The gate guards PRODUCTION
    # ActionModule registrations, not the registry's negative tests.
    dupes="$(
        grep -rlE '^[[:space:]]*const NAMESPACE:[[:space:]]*&.*str[[:space:]]*=[[:space:]]*"[^"]+"' \
            "${root}/crates" --include='*.rs' 2>/dev/null \
            | grep -vE '(^|/)tests?\.rs$|_tests\.rs$|/tests/' \
            | xargs grep -hoE '^[[:space:]]*const NAMESPACE:[[:space:]]*&.*str[[:space:]]*=[[:space:]]*"[^"]+"' 2>/dev/null \
            | grep -oE '"[^"]+"' \
            | sort \
            | uniq -d
    )"
    if [[ -n "${dupes}" ]]; then
        fail "G5 namespace uniqueness: duplicate ActionModule namespace(s): ${dupes//$'\n'/ }"
    fi
}

run_gates() {
    gate_file_identifier "${SCHEMA}"
    gate_schema_version "${SCHEMA}"
    gate_opaque_payload "${SCHEMA}"
    gate_max_bytes "${DECODE_RS}"
    gate_namespace_uniqueness "${REPO_ROOT}"
}

# --self-test: prove each gate TRIPS on a deliberately-broken fixture. Asserts
# the negative (the gate exits non-zero) for every invariant.
self_test() {
    local tmp
    tmp="$(mktemp -d)"
    trap 'rm -rf "${tmp}"' RETURN

    expect_trip() {
        local label="$1"; shift
        if ( "$@" ) 2>/dev/null; then
            echo "dispatch-envelope-gates: SELF-TEST FAIL — ${label} did NOT trip on the bad case" >&2
            exit 1
        fi
        echo "dispatch-envelope-gates: self-test OK — ${label} trips on the bad case"
    }

    # G1 trips: schema with no NMPD identifier.
    printf 'table E { x:int; }\nroot_type E;\n' >"${tmp}/no_id.fbs"
    expect_trip "G1 file_identifier" gate_file_identifier "${tmp}/no_id.fbs"

    # G2 trips: schema with no schema_version field.
    printf 'table E { correlation_id:string; }\nfile_identifier "NMPD";\n' >"${tmp}/no_ver.fbs"
    expect_trip "G2 schema_version" gate_schema_version "${tmp}/no_ver.fbs"

    # G3 trips: a union-typed (non-opaque) payload.
    printf 'union P { A }\ntable E { payload:P; }\nfile_identifier "NMPD";\n' >"${tmp}/union.fbs"
    expect_trip "G3 opaque payload" gate_opaque_payload "${tmp}/union.fbs"

    # G4 trips: a decode source missing the size bound.
    printf 'pub fn decode() {}\n' >"${tmp}/no_bound.rs"
    expect_trip "G4 max byte-size bound" gate_max_bytes "${tmp}/no_bound.rs"

    # G5 trips: two ActionModules claiming the same namespace.
    mkdir -p "${tmp}/crates/a/src" "${tmp}/crates/b/src"
    printf '    const NAMESPACE: &str = "nmp.dup";\n' >"${tmp}/crates/a/src/lib.rs"
    printf '    const NAMESPACE: &str = "nmp.dup";\n' >"${tmp}/crates/b/src/lib.rs"
    expect_trip "G5 namespace uniqueness" gate_namespace_uniqueness "${tmp}"

    echo "dispatch-envelope-gates: self-test OK (every gate trips on its bad case)"
}

case "${1:-}" in
--self-test)
    self_test
    ;;
"")
    run_gates
    echo "dispatch-envelope-gates: OK (G1 file-id, G2 schema_version, G3 opaque payload, G4 size bound, G5 namespace uniqueness)"
    ;;
*)
    echo "dispatch-envelope-gates: unknown mode '$1' (''|--self-test)" >&2
    exit 2
    ;;
esac
