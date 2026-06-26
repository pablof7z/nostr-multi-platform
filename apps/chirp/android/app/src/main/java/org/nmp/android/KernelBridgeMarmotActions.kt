package org.nmp.android

private const val MARMOT_ACTION_NAMESPACE = "nmp.marmot"

/** Dispatch a Marmot action envelope through the UniFFI AppHandle byte doorway.
 *  staged: see #2145 (M14-1) — migrate to GeneratedActionBuilders bytes-only dispatch. */
internal fun KernelBridge.dispatchMarmotAction(actionJson: String): DispatchResult =
    dispatchActionJson(MARMOT_ACTION_NAMESPACE, actionJson)
