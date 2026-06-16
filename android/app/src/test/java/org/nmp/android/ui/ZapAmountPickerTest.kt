package org.nmp.android.ui

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class ZapAmountPickerTest {
    @Test
    fun convertsSatsToMillisats() {
        assertEquals(21_000L, zapMsatsFromSats(21L))
        assertEquals(21_000_000L, zapMsatsFromSats(21_000L))
    }

    @Test
    fun parsesDigitOnlyCustomAmounts() {
        assertEquals(100_000L, parseCustomZapMsats("100 sats"))
        assertEquals(1_000_000L, parseCustomZapMsats("1,000"))
    }

    @Test
    fun rejectsEmptyZeroAndOverflow() {
        assertNull(parseCustomZapMsats(""))
        assertNull(parseCustomZapMsats("0"))
        assertNull(zapMsatsFromSats(Long.MAX_VALUE))
    }
}
