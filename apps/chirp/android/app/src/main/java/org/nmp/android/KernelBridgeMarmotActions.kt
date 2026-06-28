package org.nmp.android

// M14-1c / #2169 — The Marmot write seam now routes through the typed byte
// doorway via `GeneratedActionBuilders.marmotXxx(...)` → `bridge.dispatchBytes`.
// The hand-spelled `MARMOT_ACTION_NAMESPACE` literal and
// `dispatchMarmotAction(actionJson)` JSON bridge are DELETED here; the
// boundary gate (`ci/check_native_action_boundary.py`) asserts no hand-spelled
// `"nmp.marmot"` literal survives in production Kotlin.

/** Dispatch a Marmot action bytes payload through the typed byte doorway. */
internal fun KernelBridge.dispatchMarmotBytes(bytes: ByteArray): DispatchResult =
    dispatchBytes(bytes)
