package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidLocalHotspotDataPathTest {
    @Test
    fun stripsLegacyWifiQuotes() {
        assertEquals("SP3-LAB", stripWifiQuotes("\"SP3-LAB\""))
        assertEquals("SP3-LAB", stripWifiQuotes("SP3-LAB"))
    }

    @Test
    fun rejectsPrivilegedOrInvalidPorts() {
        assertThrows(IllegalArgumentException::class.java) {
            validateLocalHotspotPort(80)
        }
        assertThrows(IllegalArgumentException::class.java) {
            validateLocalHotspotPort(65536)
        }
    }

    @Test
    fun acceptsUnprivilegedProjectPort() {
        validateLocalHotspotPort(45125)
    }
}