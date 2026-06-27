package org.nmp.android

import java.util.UUID

/**
 * NIP-47 / NWC write surface for [KernelBridge] (M14-1 / #2145).
 *
 * All three methods build typed FlatBuffers bytes via [GeneratedActionBuilders]
 * and dispatch through the byte doorway ([KernelBridge.dispatchBytes]).
 * App code never spells action namespaces — those live only in generated code.
 */
internal fun KernelBridge.walletConnect(uri: String): DispatchResult {
    val id = UUID.randomUUID().toString()
    return dispatchBytes(GeneratedActionBuilders.walletConnect(id, uri))
}

internal fun KernelBridge.walletDisconnect(): DispatchResult {
    val id = UUID.randomUUID().toString()
    return dispatchBytes(GeneratedActionBuilders.walletDisconnect(id))
}

internal fun KernelBridge.walletPayInvoice(bolt11: String, amountMsats: Long?): DispatchResult {
    val id = UUID.randomUUID().toString()
    return dispatchBytes(GeneratedActionBuilders.walletPayInvoice(id, bolt11, amountMsats))
}
