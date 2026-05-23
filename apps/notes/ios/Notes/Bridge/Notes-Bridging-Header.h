#ifndef NOTES_BRIDGING_HEADER_H
#define NOTES_BRIDGING_HEADER_H

// Notes second-app stateful spike — Swift -> Rust bridge.
//
// The Notes shell links exactly ONE Rust archive (`libnmp_app_notes.a`)
// produced by the `nmp-app-notes` crate. That archive aggregates
// `nmp-core` (the kernel substrate) and `nmp-signer-broker` (the NIP-46
// bunker bridge) — both already exposing every C symbol declared in
// `NmpCore.h` via `#[no_mangle] extern "C"`.
//
// NOTE — the only Notes-specific symbol is `nmp_app_notes_init`, an
// app-registration marker; it carries no protocol logic. Declared below.

#import "NmpCore.h"

// Notes-specific app-registration marker. Called once after `nmp_app_new`
// and before `nmp_app_start`. Carries no protocol logic.
void nmp_app_notes_init(void *app);

#endif
