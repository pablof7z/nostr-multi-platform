package org.nmp.android

private const val MARMOT_ACTION_NAMESPACE = "nmp.marmot"

/** Dispatch a Marmot action envelope through the existing typed byte doorway. */
internal fun KernelBridge.dispatchMarmotAction(actionJson: String): DispatchResult {
    val handle = rawHandle()
    return if (handle != 0L) {
        DispatchResult.parse(nativeDispatchActionBytes(handle, MARMOT_ACTION_NAMESPACE, actionJson))
    } else {
        DispatchResult.Failure("dispatch returned a null handle")
    }
}
