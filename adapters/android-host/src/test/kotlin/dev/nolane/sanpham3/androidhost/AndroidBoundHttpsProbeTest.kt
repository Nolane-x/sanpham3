package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidBoundHttpsProbeTest {
    @Test
    fun targetValidationRejectsInvalidConfiguration() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidHttpsProbeTarget.fromLiteralAddress(
                literalAddress = "203.0.113.10",
                port = 0,
                serverName = "example.com",
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidHttpsProbeTarget.fromLiteralAddress(
                literalAddress = "203.0.113.10",
                serverName = "",
            )
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidHttpsProbeTarget.fromLiteralAddress(
                literalAddress = "203.0.113.10",
                serverName = "example.com",
                path = "not-absolute",
            )
        }
    }

    @Test
    fun rejectsHostnamesAsProbeAddresses() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidHttpsProbeTarget.fromLiteralAddress(
                literalAddress = "example.com",
                serverName = "example.com",
            )
        }
    }

    @Test
    fun parsesValidHttpStatusLine() {
        val probe = AndroidBoundHttpsProbe()
        val bytes =
            "HTTP/1.1 204 No Content\r\nX-Test: yes\r\n\r\n"
                .encodeToByteArray()

        assertEquals(204, probe.parseHttpStatus(bytes))
    }

    @Test
    fun rejectsMalformedStatusLine() {
        val probe = AndroidBoundHttpsProbe()

        assertThrows(IllegalArgumentException::class.java) {
            probe.parseHttpStatus("garbage\r\n".encodeToByteArray())
        }
    }
}
