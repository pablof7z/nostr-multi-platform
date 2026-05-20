# Chirp Maestro Smoke Tests

Run the onboarding smoke path from the repository root:

```sh
.maestro/chirp/run-onboarding-smoke.sh
```

The runner builds the iOS simulator app, installs it on `CHIRP_MAESTRO_DEVICE`
or `iPhone 17`, starts an empty local `nak serve` relay, drives onboarding with
Maestro, and then uses `nak req` to verify the newly registered user's kind `0`
profile, kind `10002` relay list, kind `3` default follows, and the relay log's
follow-feed REQ for Pablo + fiatjaf kind `1`/`6` events.

Useful overrides:

```sh
CHIRP_MAESTRO_DEVICE="iPhone 17 Pro" \
CHIRP_MAESTRO_RELAY_PORT=10548 \
CHIRP_MAESTRO_DISPLAY_NAME="Maestro Chirp Manual" \
.maestro/chirp/run-onboarding-smoke.sh
```
