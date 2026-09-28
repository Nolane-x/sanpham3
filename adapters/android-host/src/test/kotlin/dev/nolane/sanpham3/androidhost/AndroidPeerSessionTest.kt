package dev.nolane.sanpham3.androidhost

import java.io.ByteArrayInputStream
import java.io.EOFException
import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidPeerSessionTest {
    @Test
    fun decodesBigEndianHandleFields() {
        val bytes = byteArrayOf(
            0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x01, 0x23,
            0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x04, 0x56,
        )

        assertEquals(0x123L, AndroidPeerSession.decodeU64(bytes, 0))
        assertEquals(0x456L, AndroidPeerSession.decodeU64(bytes, 8))
    }

    @Test
    fun exactReadConsumesRequestedBytes() {
        val input = ByteArrayInputStream(byteArrayOf(1, 2, 3, 4))
        assertArrayEquals(byteArrayOf(1, 2, 3), input.readExactly(3))
        assertEquals(4, input.read())
    }

    @Test
    fun exactReadRejectsEarlyEof() {
        val input = ByteArrayInputStream(byteArrayOf(1, 2))

        assertThrows(EOFException::class.java) {
            input.readExactly(3)
        }
    }

    @Test
    fun handleParserRejectsSignedU64Range() {
        val bytes = ByteArray(8) { 0xff.toByte() }

        assertThrows(IllegalArgumentException::class.java) {
            AndroidPeerSession.decodeU64(bytes, 0)
        }
    }
}
