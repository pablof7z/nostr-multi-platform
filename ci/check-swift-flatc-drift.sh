#!/usr/bin/env bash
#
# Swift flatc codegen-drift gate (Issue #1, codegen drift correctness).
#
# The 35 checked-in `flatc --swift` Swift binding files under
#   ios/Chirp/Chirp/Bridge/Generated/*.generated.swift
# (every `.generated.swift` file EXCEPT KernelTypes.generated.swift and
# TypedProjectionDecoders.generated.swift, which are emitted by the
# nmp-codegen Swift `Decodable`/typed-decoder generators — covered by the
# `swift-codegen-drift` job — not by `flatc --swift`) are the flatc output
# for the `.fbs` schemas scattered across the workspace crates.
#
# This script regenerates each binding with the PINNED flatc (must match the
# `flatbuffers` Rust runtime pin / Swift pin in Cargo.toml — 25.12.19, the
# SAME pin as the Rust+Swift gate, NOT Android's 25.2.10 nor Web's 25.9.23)
# and fails on any byte difference — so the schemas and the checked-in Swift
# bindings can never silently drift apart.
#
# Naming convention: `flatc --swift` emits `<fbs_basename>_generated.swift`
# (snake_case, derived from the .fbs FILE name). The checked-in files are the
# same bytes renamed to `<RootType>.generated.swift` (PascalCase, dotted). This
# script maps each schema to its checked-in PascalCase file explicitly.
#
# Usage:
#   ci/check-swift-flatc-drift.sh
#   ci/check-swift-flatc-drift.sh --write
# Requires: flatc 25.12.19 on PATH.

set -euo pipefail

MODE="${1:---check}"
case "${MODE}" in
--check|--write) ;;
*)
    echo "swift-flatc-drift: unknown mode '${MODE}' (--check|--write)" >&2
    exit 2
    ;;
esac

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

# #1723 — flatc version pins are single-sourced from ci/flatc-pins.sh.
# shellcheck source=ci/flatc-pins.sh
source "${SCRIPT_DIR}/flatc-pins.sh"
EXPECTED_FLATC_VERSION="${FLATC_PIN_RUST_SWIFT}"
GENERATED_DIR="${REPO_ROOT}/ios/Chirp/Chirp/Bridge/Generated"
mkdir -p "${GENERATED_DIR}"

if ! command -v flatc >/dev/null 2>&1; then
    echo "swift-flatc-drift: flatc not found on PATH (need ${EXPECTED_FLATC_VERSION})" >&2
    exit 1
fi

ACTUAL_FLATC_VERSION="$(flatc --version | grep -oE '[0-9]+\.[0-9]+\.[0-9]+')"
if [[ "${ACTUAL_FLATC_VERSION}" != "${EXPECTED_FLATC_VERSION}" ]]; then
    echo "swift-flatc-drift: flatc ${ACTUAL_FLATC_VERSION} found, but the Swift" >&2
    echo "FlatBuffers bindings are pinned to flatc ${EXPECTED_FLATC_VERSION}" >&2
    echo "(matching the 'flatbuffers = \"${EXPECTED_FLATC_VERSION}\"' runtime pin in Cargo.toml;" >&2
    echo "this is the Rust+Swift pin, distinct from Android ${FLATC_PIN_KOTLIN} / Web ${FLATC_PIN_TS})." >&2
    exit 1
fi

# Schema-to-Swift mapping: each entry is "<schema_path>|<flatc_basename>|<CheckedInFile>".
#   <schema_path>    relative-to-repo-root path to the .fbs source
#   <flatc_basename> the file flatc --swift emits (<fbs_basename>_generated.swift)
#   <CheckedInFile>  the PascalCase checked-in file in ios/.../Generated
MAPPINGS=(
    "crates/nmp-core/schema/accounts.fbs|accounts_generated.swift|Accounts.generated.swift"
    "crates/nmp-core/schema/action_lifecycle.fbs|action_lifecycle_generated.swift|ActionLifecycle.generated.swift"
    "crates/nmp-core/schema/action_results.fbs|action_results_generated.swift|ActionResults.generated.swift"
    "crates/nmp-core/schema/action_stages.fbs|action_stages_generated.swift|ActionStages.generated.swift"
    "crates/nmp-core/schema/active_account.fbs|active_account_generated.swift|ActiveAccount.generated.swift"
    "crates/nmp-core/schema/bunker_handshake.fbs|bunker_handshake_generated.swift|BunkerHandshake.generated.swift"
    "crates/nmp-core/schema/claimed_events.fbs|claimed_events_generated.swift|ClaimedEvents.generated.swift"
    "crates/nmp-core/schema/configured_relays.fbs|configured_relays_generated.swift|ConfiguredRelays.generated.swift"
    "crates/nmp-core/schema/nip46_onboarding.fbs|nip46_onboarding_generated.swift|Nip46Onboarding.generated.swift"
    "crates/nmp-core/schema/nmp_update.fbs|nmp_update_generated.swift|NmpUpdate.generated.swift"
    "crates/nmp-core/schema/outbox_summary.fbs|outbox_summary_generated.swift|OutboxSummary.generated.swift"
    "crates/nmp-core/schema/profile_card.fbs|profile_card_generated.swift|ProfileCard.generated.swift"
    "crates/nmp-core/schema/profile.fbs|profile_generated.swift|Profile.generated.swift"
    "crates/nmp-core/schema/publish_outbox.fbs|publish_outbox_generated.swift|PublishOutbox.generated.swift"
    "crates/nmp-core/schema/publish_queue.fbs|publish_queue_generated.swift|PublishQueue.generated.swift"
    "crates/nmp-core/schema/relay_diagnostics.fbs|relay_diagnostics_generated.swift|RelayDiagnostics.generated.swift"
    "crates/nmp-core/schema/relay_role_options.fbs|relay_role_options_generated.swift|RelayRoleOptions.generated.swift"
    "crates/nmp-core/schema/ref_rowdelta.fbs|ref_rowdelta_generated.swift|RefRowDelta.generated.swift"
    "crates/nmp-core/schema/settings_hub.fbs|settings_hub_generated.swift|SettingsHub.generated.swift"
    "crates/nmp-core/schema/signer_state.fbs|signer_state_generated.swift|SignerState.generated.swift"
    "crates/nmp-content/schema/content_tree.fbs|content_tree_generated.swift|ContentTree.generated.swift"
    "crates/nmp-content/schema/embed_sidecar.fbs|embed_sidecar_generated.swift|ClaimedEventEmbeds.generated.swift"
    "crates/nmp-feed/schema/feed_home.fbs|feed_home_generated.swift|FeedWindow.generated.swift"
    "crates/nmp-marmot/schema/marmot_messages.fbs|marmot_messages_generated.swift|MarmotMessages.generated.swift"
    "crates/nmp-marmot/schema/marmot_snapshot.fbs|marmot_snapshot_generated.swift|MarmotSnapshot.generated.swift"
    "crates/nmp-nip01/schema/op_feed.fbs|op_feed_generated.swift|OpFeedSnapshot.generated.swift"
    "crates/nmp-nip01/schema/timeline_snapshot.fbs|timeline_snapshot_generated.swift|TimelineSnapshot.generated.swift"
    "crates/nmp-nip02/schema/follow_list.fbs|follow_list_generated.swift|FollowList.generated.swift"
    "crates/nmp-nip17/schema/dm_inbox.fbs|dm_inbox_generated.swift|DmInbox.generated.swift"
    "crates/nmp-nip17/schema/dm_relay_list.fbs|dm_relay_list_generated.swift|DmRelayList.generated.swift"
    "crates/nmp-nip29/schema/discovered_groups.fbs|discovered_groups_generated.swift|DiscoveredGroups.generated.swift"
    "crates/nmp-nip29/schema/group_chat.fbs|group_chat_generated.swift|GroupChat.generated.swift"
    "crates/nmp-nip29/schema/group_defaults.fbs|group_defaults_generated.swift|GroupDefaults.generated.swift"
    "crates/nmp-nip47/schema/wallet_status.fbs|wallet_status_generated.swift|WalletStatus.generated.swift"
    "crates/nmp-nip50/schema/search_results.fbs|search_results_generated.swift|SearchResults.generated.swift"
    "crates/nmp-nip57/schema/zaps.fbs|zaps_generated.swift|Zaps.generated.swift"
)

TMP_DIR="$(mktemp -d)"
trap 'rm -rf "${TMP_DIR}"' EXIT

drift_count=0
checked=0

for entry in "${MAPPINGS[@]}"; do
    IFS='|' read -r schema_rel flatc_name checked_in_name <<<"${entry}"
    schema_path="${REPO_ROOT}/${schema_rel}"
    checked_in="${GENERATED_DIR}/${checked_in_name}"

    if [[ ! -f "${schema_path}" ]]; then
        echo "swift-flatc-drift: schema missing: ${schema_rel}" >&2
        drift_count=$((drift_count + 1))
        continue
    fi

    # One subdir per schema so identically-named outputs (and an `include`'s
    # extra emitted module, e.g. op_feed → timeline_snapshot_generated.swift)
    # never collide. flatc resolves `include` siblings relative to the schema
    # dir automatically, so running over the schema path is sufficient.
    out_subdir="${TMP_DIR}/$(basename "${schema_rel}" .fbs)"
    mkdir -p "${out_subdir}"
    flatc --swift -o "${out_subdir}" "${schema_path}"

    fresh="${out_subdir}/${flatc_name}"
    if [[ ! -f "${fresh}" ]]; then
        echo "swift-flatc-drift: flatc did not emit expected file ${flatc_name} for ${schema_rel}" >&2
        echo "  (found: $(ls -1 "${out_subdir}" | tr '\n' ' '))" >&2
        drift_count=$((drift_count + 1))
        continue
    fi

    if [[ "${MODE}" == "--write" ]]; then
        cp "${fresh}" "${checked_in}"
        checked=$((checked + 1))
        continue
    fi

    if [[ ! -f "${checked_in}" ]]; then
        echo "swift-flatc-drift: checked-in Swift binding missing: ${checked_in_name}" >&2
        drift_count=$((drift_count + 1))
        continue
    fi

    if ! diff -u "${checked_in}" "${fresh}"; then
        echo "" >&2
        echo "swift-flatc-drift: ${checked_in_name} drifted from a fresh" >&2
        echo "'flatc --swift' run over ${schema_rel}." >&2
        echo "Regenerate with:" >&2
        echo "  bash ci/regenerate-flatbuffers.sh" >&2
        echo "" >&2
        drift_count=$((drift_count + 1))
        continue
    fi

    checked=$((checked + 1))
done

if [[ "${drift_count}" -ne 0 ]]; then
    echo "swift-flatc-drift: FAIL — ${drift_count} binding(s) drifted (${checked} in sync)" >&2
    exit 1
fi

if [[ "${MODE}" == "--write" ]]; then
    echo "swift-flatc-drift: wrote ${checked} Swift bindings (flatc ${EXPECTED_FLATC_VERSION})"
    exit 0
fi

echo "swift-flatc-drift: OK (flatc ${EXPECTED_FLATC_VERSION}, ${checked} Swift bindings in sync)"
