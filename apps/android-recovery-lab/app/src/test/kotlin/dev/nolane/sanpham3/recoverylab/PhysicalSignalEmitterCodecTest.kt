package dev.nolane.sanpham3.recoverylab

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class PhysicalSignalEmitterCodecTest {
    @Test
    fun hexToBitsUsesNetworkBitOrder() {
        assertArrayEquals(
            byteArrayOf(
                1, 0, 1, 1, 0, 0, 1, 0,
            ),
            PhysicalSignalEmitterCodec.bitsFromHex("b2"),
        )
    }

    @Test
    fun acousticPcmMatchesReferenceSymbolLengthAndBounds() {
        val bits = byteArrayOf(0, 1)
        val pcm =
            PhysicalSignalEmitterCodec.acousticPcm16(
                bits,
            )

        assertEquals(
            2 *
                PhysicalSignalEmitterCodec
                    .acousticSamplesPerBit,
            pcm.size,
        )
        assertTrue(
            pcm.all {
                it.toInt() in
                    Short.MIN_VALUE.toInt()..
                        Short.MAX_VALUE.toInt()
            },
        )
        assertEquals(0, pcm[0].toInt())
        assertEquals(
            0,
            pcm[
                PhysicalSignalEmitterCodec
                    .acousticSamplesPerBit
            ].toInt(),
        )
    }

    @Test
    fun vibrationPatternPreservesExactBitDuration() {
        val bits = byteArrayOf(1, 0, 1)
        val pattern =
            PhysicalSignalEmitterCodec
                .vibrationPattern(bits)

        assertEquals(
            bits.size *
                PhysicalSignalEmitterCodec
                    .vibrationBitDurationMs,
            pattern.durationMs,
        )
        assertEquals(
            pattern.timingsMs.size,
            pattern.amplitudes.size,
        )
        assertTrue(
            pattern.amplitudes.any { it > 0 },
        )
        assertTrue(
            pattern.amplitudes.any { it == 0 },
        )
    }
}
