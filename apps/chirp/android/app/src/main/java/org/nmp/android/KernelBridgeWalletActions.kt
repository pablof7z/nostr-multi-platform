package org.nmp.android

/**
 * NIP-47 / NWC write surface for [KernelBridge].
 *
 * The app-facing methods are typed wallet intents. The ADR-0064
 * namespace/body transport stays private to this bridge file so UI and model
 * code cannot route arbitrary writes by spelling action namespaces.
 */
internal fun KernelBridge.walletConnect(uri: String): DispatchResult =
    dispatchWalletWrite(
        "nmp.wallet.connect",
        """{"Connect":{"uri":${walletJsonString(uri)}}}""",
    )

internal fun KernelBridge.walletDisconnect(): DispatchResult =
    dispatchWalletWrite("nmp.wallet.disconnect", "\"Disconnect\"")

private fun KernelBridge.dispatchWalletWrite(namespace: String, bodyJson: String): DispatchResult {
    val handle = rawHandle()
    return if (handle != 0L) {
        DispatchResult.parse(nativeDispatchActionBytes(handle, namespace, bodyJson))
    } else {
        DispatchResult.Failure("dispatch returned a null handle")
    }
}

private fun walletJsonString(value: String): String {
    val sb = StringBuilder(value.length + 2)
    sb.append('"')
    for (c in value) {
        when (c) {
            '"' -> sb.append("\\\"")
            '\\' -> sb.append("\\\\")
            '\n' -> sb.append("\\n")
            '\r' -> sb.append("\\r")
            '\t' -> sb.append("\\t")
            else -> sb.append(c)
        }
    }
    sb.append('"')
    return sb.toString()
}
