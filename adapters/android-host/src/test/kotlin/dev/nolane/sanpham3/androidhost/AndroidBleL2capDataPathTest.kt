package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidBleL2capDataPathTest {
    @Test
    fun advertisementRoundtripPreservesPsm() {
        for (psm in listOf(1, 0x0080, 0x1234, 0xffff)) {
            val encoded = AndroidBleL2capAdvertisement.encode(psm)
            assertEquals(7, encoded.size)
            assertEquals(
                psm,
                AndroidBleL2capAdvertisement.decode(encoded).psm,
            )
        }
    }

    @Test
    fun stableWirePrefixIsSmall() {
        val encoded = AndroidBleL2capAdvertisement.encode(0x1234)

        assertArrayEquals(
            byteArrayOf(
                'S'.code.toByte(),
                'P'.code.toByte(),
                '3'.code.toByte(),
                'L'.code.toByte(),
                0,
            ),
            encoded.copyOfRange(0, 5),
        )
    }

    @Test
    fun rejectsMalformedDiscoveryPayload() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidBleL2capAdvertisement.decode(byteArrayOf(1, 2, 3))
        }

        val wrongMagic = AndroidBleL2capAdvertisement.encode(1234)
        wrongMagic[0] = 'X'.code.toByte()
        assertThrows(IllegalArgumentException::class.java) {
            AndroidBleL2capAdvertisement.decode(wrongMagic)
        }

        assertThrows(IllegalArgumentException::class.java) {
            AndroidBleL2capAdvertisement.encode(0)
        }
        assertThrows(IllegalArgumentException::class.java) {
            AndroidBleL2capAdvertisement.encode(65_536)
        }
    }
}
