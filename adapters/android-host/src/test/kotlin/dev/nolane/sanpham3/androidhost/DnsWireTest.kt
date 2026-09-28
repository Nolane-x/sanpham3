package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class DnsWireTest {
    @Test
    fun rootQueryHasMinimalExpectedShape() {
        val query = DnsWire.rootAQuery()

        assertEquals(17, query.size)
        assertEquals(0x53, query[0].toInt() and 0xff)
        assertEquals(0x50, query[1].toInt() and 0xff)
        assertEquals(1, query[5].toInt() and 0xff)
    }

    @Test
    fun validatesResponseAndReturnsRcode() {
        val response = DnsWire.rootAQuery()
        response[2] = (response[2].toInt() or 0x80).toByte()
        response[3] = 0x03

        assertEquals(3, DnsWire.validateResponse(response, response.size))
    }

    @Test
    fun rejectsWrongTransactionId() {
        val response = DnsWire.rootAQuery()
        response[0] = 0x00
        response[2] = (response[2].toInt() or 0x80).toByte()

        assertThrows(IllegalArgumentException::class.java) {
            DnsWire.validateResponse(response, response.size)
        }
    }

    @Test
    fun rejectsPacketThatIsNotAResponse() {
        val response = DnsWire.rootAQuery()

        assertThrows(IllegalArgumentException::class.java) {
            DnsWire.validateResponse(response, response.size)
        }
    }
}
