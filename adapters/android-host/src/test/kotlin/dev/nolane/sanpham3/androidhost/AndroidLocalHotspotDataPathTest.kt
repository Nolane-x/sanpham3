package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidLocalHotspotDataPathTest {
    @Test
    fun stripsLegacyWifiQuotes() {
        assertEquals("SP3-LAB", stripWifiQuotes("\"SP3-LAB\""))
        assertEquals("SP3-LAB", stripWifiQuotes("SP3-LAB"))
    }

    @Test
    fun hotspotBootstrapRoundtripsAndFitsTinyCarrierBudget() {
        val endpoint = AndroidLocalHotspotEndpoint(
            ssid = "SP3-LAB",
            passphrase = "12345678",
            security = AndroidLocalHotspotSecurity.WPA2_PSK,
            port = 45125,
        )

        val encoded = AndroidLocalHotspotBootstrap.encode(endpoint)
        val decoded = AndroidLocalHotspotBootstrap.decode(encoded)

        assertEquals(endpoint, decoded)
        assertTrue(
            encoded.size <=
                AndroidLocalHotspotBootstrap.MAX_ENCODED_BYTES,
        )
    }

    @Test
    fun openHotspotBootstrapCarriesNoSecret() {
        val endpoint = AndroidLocalHotspotEndpoint(
            ssid = "SP3-OPEN",
            passphrase = null,
            security = AndroidLocalHotspotSecurity.OPEN,
            port = 45125,
        )

        val encoded = AndroidLocalHotspotBootstrap.encode(endpoint)
        assertEquals(
            endpoint,
            AndroidLocalHotspotBootstrap.decode(encoded),
        )
    }

    @Test
    fun securedBootstrapRejectsShortPassphrase() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidLocalHotspotBootstrap.encode(
                AndroidLocalHotspotEndpoint(
                    ssid = "SP3",
                    passphrase = "short",
                    security = AndroidLocalHotspotSecurity.WPA2_PSK,
                    port = 45125,
                ),
            )
        }
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