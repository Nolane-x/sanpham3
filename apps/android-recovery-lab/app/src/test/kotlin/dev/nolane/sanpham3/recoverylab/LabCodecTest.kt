package dev.nolane.sanpham3.recoverylab

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Assert.assertThrows
import org.junit.Test

class LabCodecTest {
    @Test
    fun parsesExactPeerKey() {
        val key = LabCodec.parsePeerKey("00ff" + "42".repeat(30))

        assertEquals(32, key.size)
        assertEquals(0x00, key[0].toInt() and 0xff)
        assertEquals(0xff, key[1].toInt() and 0xff)
        assertEquals(0x42, key[31].toInt() and 0xff)
    }

    @Test
    fun rejectsMalformedPeerKey() {
        assertThrows(IllegalArgumentException::class.java) {
            LabCodec.parsePeerKey("42")
        }
        assertThrows(IllegalArgumentException::class.java) {
            LabCodec.parsePeerKey("zz".repeat(32))
        }
    }

    @Test
    fun decodesBleL2capAdvertisement() {
        val payload = byteArrayOf(
            'S'.code.toByte(),
            'P'.code.toByte(),
            '3'.code.toByte(),
            'L'.code.toByte(),
            0,
            0x12,
            0x34,
        )

        assertEquals(0x1234, LabCodec.decodeL2capPsm(payload))
        assertEquals("5350334c001234", LabCodec.hex(payload))
    }

    @Test
    fun parsesGenericHexAndHashesDeterministically() {
        val bytes = LabCodec.parseHex("53 50 33 48 00 ff")

        assertArrayEquals(
            byteArrayOf(0x53, 0x50, 0x33, 0x48, 0x00, 0xff.toByte()),
            bytes,
        )
        assertEquals(
            "d38cc1c276c17bf2e43b69c6d751a0b779ef6d82c9ccc9bee4e4bcbe08406e61",
            LabCodec.sha256Hex(bytes),
        )
    }

    @Test
    fun rejectsOddOrInvalidGenericHex() {
        assertThrows(IllegalArgumentException::class.java) {
            LabCodec.parseHex("abc")
        }
        assertThrows(IllegalArgumentException::class.java) {
            LabCodec.parseHex("zz")
        }
    }

    @Test
    fun rejectsNonL2capDiscoveryPayload() {
        assertNull(LabCodec.decodeL2capPsm(byteArrayOf(1, 2, 3)))

        val wrong = byteArrayOf(
            'X'.code.toByte(),
            'P'.code.toByte(),
            '3'.code.toByte(),
            'L'.code.toByte(),
            0, 0, 1,
        )
        assertNull(LabCodec.decodeL2capPsm(wrong))
    }
}