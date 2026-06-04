package org.nmp.android.model

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class RelayRolesTest {
    @Test
    fun canonicalRelayRoleInputMapsLabelsAndLegacyReadWrite() {
        val options = defaultRelayRoleOptions()

        assertEquals("both", defaultRelayRoleValue(options))
        assertEquals("read", canonicalRelayRoleInput("Read", options))
        assertEquals("write", canonicalRelayRoleInput("Write", options))
        assertEquals("both", canonicalRelayRoleInput("Both", options))
        assertEquals("both,indexer", canonicalRelayRoleInput("Both + Index", options))
        assertEquals("both", canonicalRelayRoleInput("ReadWrite", options))
        assertEquals("both", canonicalRelayRoleInput("read+write", options))
        assertEquals("both", canonicalRelayRoleInput("read,write", options))
        assertNull(canonicalRelayRoleInput("owner", options))
    }

    @Test
    fun relayRoleLabelDisplaysCanonicalValues() {
        val options = defaultRelayRoleOptions()

        assertEquals("Read", relayRoleLabel("read", options))
        assertEquals("Write", relayRoleLabel("write", options))
        assertEquals("Both", relayRoleLabel("both", options))
        assertEquals("Both + Index", relayRoleLabel("both,indexer", options))
        assertEquals("custom", relayRoleLabel("custom", options))
    }
}
