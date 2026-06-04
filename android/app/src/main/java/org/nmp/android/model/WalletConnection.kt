package org.nmp.android.model

fun isWalletConnectedStatus(status: String?): Boolean =
    status.equals("ready", ignoreCase = true) ||
        status.equals("connecting", ignoreCase = true) ||
        status.equals("connected", ignoreCase = true)

fun isWalletReadyStatus(status: String?): Boolean =
    status.equals("ready", ignoreCase = true) ||
        status.equals("connected", ignoreCase = true)
