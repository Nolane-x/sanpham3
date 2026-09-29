package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test
import java.net.InetAddress

class AndroidPeerEgressTest {
    @Test
    fun requestWireMatchesRustPeerEgressContract() {
        val request = AndroidPeerEgress.ResolveRequest(
            requestId = 0x01020304,
            relaysRemaining = 8,
            hostname = "example.com",
        )

        val encoded = AndroidPeerEgress.encodeRequest(request)
        val expected = byteArrayOf(
            0x01, 0x02, 0x03, 0x04,
            0x08,
            0x0b,
            'e'.code.toByte(),
            'x'.code.toByte(),
            'a'.code.toByte(),
            'm'.code.toByte(),
            'p'.code.toByte(),
            'l'.code.toByte(),
            'e'.code.toByte(),
            '.'.code.toByte(),
            'c'.code.toByte(),
            'o'.code.toByte(),
            'm'.code.toByte(),
        )

        assertArrayEquals(expected, encoded)
        assertEquals(request, AndroidPeerEgress.decodeRequest(encoded))
    }

    @Test
    fun responseWireMatchesRustPeerEgressContract() {
        val response = AndroidPeerEgress.ResolveResponse(
            requestId = 0x01020304,
            status = AndroidPeerEgress.ResolveStatus.OK,
            addresses = listOf(InetAddress.getByName("8.8.8.8")),
        )

        val encoded = AndroidPeerEgress.encodeResponse(response)
        assertArrayEquals(
            byteArrayOf(
                0x01, 0x02, 0x03, 0x04,
                0x00,
                0x01,
                0x04,
                0x08, 0x08, 0x08, 0x08,
            ),
            encoded,
        )

        val decoded = AndroidPeerEgress.decodeResponse(encoded)
        assertEquals(response.requestId, decoded.requestId)
        assertEquals(response.status, decoded.status)
        assertEquals("8.8.8.8", decoded.addresses.single().hostAddress)
    }

    @Test
    fun hostnamePolicyMatchesConstrainedPublicOperation() {
        for (hostname in listOf(
            "localhost",
            "router.local",
            "printer.lan",
            "10.0.0.1",
            "::1",
            "singlelabel",
            "-bad.example",
            "bad-.example",
        )) {
            assertFalse(
                "$hostname should be rejected",
                AndroidPeerEgress.isValidPublicHostname(hostname),
            )
        }

        assertTrue(AndroidPeerEgress.isValidPublicHostname("example.com"))
        assertTrue(AndroidPeerEgress.isValidPublicHostname("www.example.com."))
    }

    @Test
    fun publicAddressFilterRejectsPrivateAndDocumentationRanges() {
        for (address in listOf(
            "127.0.0.1",
            "10.0.0.1",
            "169.254.1.1",
            "100.64.0.1",
            "192.0.2.1",
            "198.51.100.7",
            "203.0.113.9",
            "2001:db8::1",
            "fc00::1",
            "fe80::1",
        )) {
            assertFalse(
                "$address should be rejected",
                AndroidPeerEgress.isPublicDestination(
                    InetAddress.getByName(address),
                ),
            )
        }

        assertTrue(
            AndroidPeerEgress.isPublicDestination(
                InetAddress.getByName("8.8.8.8"),
            ),
        )
        assertTrue(
            AndroidPeerEgress.isPublicDestination(
                InetAddress.getByName("2606:4700:4700::1111"),
            ),
        )
    }

    @Test
    fun resolverFiltersPrivateResultsAndKeepsPublicOnes() {
        val response = AndroidPeerEgress.resolveRequest(
            AndroidPeerEgress.ResolveRequest(
                requestId = 42,
                relaysRemaining = 8,
                hostname = "example.com",
            ),
            AndroidPeerEgress.Resolver {
                listOf(
                    InetAddress.getByName("10.0.0.7"),
                    InetAddress.getByName("8.8.8.8"),
                )
            },
        )

        assertEquals(AndroidPeerEgress.ResolveStatus.OK, response.status)
        assertEquals(
            listOf("8.8.8.8"),
            response.addresses.mapNotNull(InetAddress::getHostAddress),
        )
    }
}
