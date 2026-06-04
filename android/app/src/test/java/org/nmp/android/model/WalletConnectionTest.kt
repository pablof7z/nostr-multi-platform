package org.nmp.android.model

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class WalletConnectionTest {
    @Test
    fun readyAndConnectingAreConnectedStates() {
        assertTrue(isWalletConnectedStatus("ready"))
        assertTrue(isWalletConnectedStatus("Ready"))
        assertTrue(isWalletConnectedStatus("connecting"))
        assertTrue(isWalletConnectedStatus("connected"))

        assertFalse(isWalletConnectedStatus("disconnected"))
        assertFalse(isWalletConnectedStatus("error"))
        assertFalse(isWalletConnectedStatus(null))
    }

    @Test
    fun readyMeansUsableWallet() {
        assertTrue(isWalletReadyStatus("ready"))
        assertTrue(isWalletReadyStatus("connected"))

        assertFalse(isWalletReadyStatus("connecting"))
        assertFalse(isWalletReadyStatus("disconnected"))
        assertFalse(isWalletReadyStatus(null))
    }
}
