#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
FLOW="${ROOT}/.maestro/chirp/onboarding.yaml"
DEVICE_NAME="${CHIRP_MAESTRO_DEVICE:-iPhone 17}"
RELAY_PORT="${CHIRP_MAESTRO_RELAY_PORT:-10547}"
RELAY_URL="${CHIRP_MAESTRO_RELAY_URL:-ws://127.0.0.1:${RELAY_PORT}}"
DISPLAY_NAME="${CHIRP_MAESTRO_DISPLAY_NAME:-Maestro Chirp Smoke}"
DERIVED_DATA="${CHIRP_MAESTRO_DERIVED_DATA:-${ROOT}/apps/chirp/ios/DerivedData-maestro}"
APP_PATH="${DERIVED_DATA}/Build/Products/Debug-iphonesimulator/Chirp.app"
RELAY_LOG="${TMPDIR:-/tmp}/chirp-maestro-nak-${RELAY_PORT}.log"
RELAY_PID=""
PABLO_HEX="fa984bd7dbb282f07e16e7ae87b26a2a7b9b90b7246a44771f0cf5ae58018f52"
FIATJAF_HEX="3bf0c63fcb93463407af97a5e5ee64fa883d107ef9e558472c4eb9aaaefa459d"

require() {
  if ! command -v "$1" >/dev/null 2>&1; then
    echo "missing required command: $1" >&2
    exit 1
  fi
}

require cargo
require jq
require maestro
require nak
require xcodebuild
require xcrun

device_udid() {
  xcrun simctl list devices available |
    awk -v name="$DEVICE_NAME" '
      $0 ~ name && $0 ~ /\([0-9A-F-]+\)/ {
        match($0, /\([0-9A-F-]+\)/)
        print substr($0, RSTART + 1, RLENGTH - 2)
        exit
      }
    '
}

wait_for_relay() {
  local deadline=$((SECONDS + 10))
  until nak relay "$RELAY_URL" >/dev/null 2>&1; do
    if (( SECONDS >= deadline )); then
      echo "relay did not become reachable at ${RELAY_URL}" >&2
      return 1
    fi
    sleep 0.2
  done
}

event_by_profile_name() {
  nak req -k 0 -l 100 "$RELAY_URL" 2>/dev/null |
    jq -c --arg name "$DISPLAY_NAME" '
      select(.kind == 0)
      | select((.content | fromjson? | .name) == $name)
    ' |
    head -n 1
}

event_by_author_and_kind() {
  local pubkey="$1"
  local kind="$2"
  nak req -a "$pubkey" -k "$kind" -l 1 "$RELAY_URL" 2>/dev/null |
    jq -c --arg pubkey "$pubkey" --argjson kind "$kind" '
      select(.kind == $kind and .pubkey == $pubkey)
    ' |
    head -n 1
}

wait_for_event() {
  local description="$1"
  shift
  local deadline=$((SECONDS + 30))
  local event=""
  until [[ -n "$event" ]]; do
    event="$("$@" || true)"
    if [[ -n "$event" ]]; then
      printf '%s\n' "$event"
      return 0
    fi
    if (( SECONDS >= deadline )); then
      echo "timed out waiting for ${description}" >&2
      return 1
    fi
    sleep 1
  done
}

wait_until() {
  local description="$1"
  shift
  local deadline=$((SECONDS + 30))
  until "$@"; do
    if (( SECONDS >= deadline )); then
      echo "timed out waiting for ${description}" >&2
      return 1
    fi
    sleep 1
  done
}

follow_feed_author_request_seen() {
  local author="$1"
  local match
  match="$(
    awk '/got request / { sub(/^.*got request /, ""); print }' "$RELAY_LOG" |
      jq -c --arg author "$author" '
        select((.kinds // []) | index(1))
        | select((.kinds // []) | index(6))
        | select((.authors // []) | index($author))
      ' 2>/dev/null |
      head -n 1
  )"
  [[ -n "$match" ]]
}

follow_feed_request_seen() {
  follow_feed_author_request_seen "$PABLO_HEX" &&
    follow_feed_author_request_seen "$FIATJAF_HEX"
}

main() {
  local udid
  udid="$(device_udid)"
  if [[ -z "$udid" ]]; then
    echo "could not find an available simulator named ${DEVICE_NAME}" >&2
    exit 1
  fi

  cargo build -p nmp-core --target aarch64-apple-ios-sim
  cargo build -p nmp-signer-broker --target aarch64-apple-ios-sim
  cargo build -p nmp-app-chirp --target aarch64-apple-ios-sim

  xcodebuild \
    -project "${ROOT}/apps/chirp/ios/Chirp.xcodeproj" \
    -scheme Chirp \
    -destination "platform=iOS Simulator,id=${udid}" \
    -derivedDataPath "$DERIVED_DATA" \
    build

  xcrun simctl boot "$udid" >/dev/null 2>&1 || true
  xcrun simctl bootstatus "$udid" -b
  xcrun simctl install "$udid" "$APP_PATH"

  nak serve --hostname 127.0.0.1 --port "$RELAY_PORT" >"$RELAY_LOG" 2>&1 &
  RELAY_PID=$!
  trap '[[ -z "$RELAY_PID" ]] || kill "$RELAY_PID" >/dev/null 2>&1 || true' EXIT
  wait_for_relay

  local rendered_flow
  rendered_flow="$(mktemp "${TMPDIR:-/tmp}/chirp-onboarding.XXXXXX")"
  awk -v relay="$RELAY_URL" -v name="$DISPLAY_NAME" '
    { gsub("ws://127.0.0.1:10547", relay); gsub("Maestro Chirp Smoke", name); print }
  ' "$FLOW" >"$rendered_flow"

  maestro --device "$udid" test "$rendered_flow"

  local profile_event
  profile_event="$(wait_for_event "kind:0 profile for ${DISPLAY_NAME}" event_by_profile_name)"
  local pubkey
  pubkey="$(jq -r '.pubkey' <<<"$profile_event")"

  local relay_list_event
  relay_list_event="$(wait_for_event "kind:10002 relay list for ${pubkey}" event_by_author_and_kind "$pubkey" 10002)"

  jq -e --arg relay "$RELAY_URL" '
    any(.tags[]?; .[0] == "r" and .[1] == $relay)
  ' <<<"$relay_list_event" >/dev/null

  local contacts_event
  contacts_event="$(wait_for_event "kind:3 contacts for ${pubkey}" event_by_author_and_kind "$pubkey" 3)"
  jq -e --arg pablo "$PABLO_HEX" --arg fiatjaf "$FIATJAF_HEX" '
    any(.tags[]?; .[0] == "p" and .[1] == $pablo)
    and any(.tags[]?; .[0] == "p" and .[1] == $fiatjaf)
  ' <<<"$contacts_event" >/dev/null

  wait_until "follow-feed REQ for default follows" follow_feed_request_seen

  echo "CHIRP_MAESTRO_ONBOARDING_OK pubkey=${pubkey} relay=${RELAY_URL} display_name=${DISPLAY_NAME}"
}

main "$@"
