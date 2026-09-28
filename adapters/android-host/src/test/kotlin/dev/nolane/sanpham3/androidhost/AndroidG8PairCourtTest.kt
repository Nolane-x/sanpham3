package dev.nolane.sanpham3.androidhost

import java.security.SecureRandom
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidG8PairCourtTest {
    private class FixedRandom : SecureRandom() {
        override fun nextBytes(bytes: ByteArray) {
            for (index in bytes.indices) {
                bytes[index] = index.toByte()
            }
        }
    }

    @Test
    fun challengeUsesSharedG8WireFormat() {
        val challenge = AndroidG8PairCourt.newChallenge(FixedRandom())

        assertEquals(36, challenge.size)
        assertEquals('G'.code.toByte(), challenge[0])
        assertEquals('8'.code.toByte(), challenge[1])
        assertEquals('P'.code.toByte(), challenge[2])
        assertEquals('0'.code.toByte(), challenge[3])

        for (index in 0 until 32) {
            assertEquals(index.toByte(), challenge[index + 4])
        }

        AndroidG8PairCourt.validateChallenge(challenge)
    }

    @Test
    fun validatorRejectsWrongMagic() {
        val challenge = AndroidG8PairCourt.newChallenge(FixedRandom())
        challenge[0] = 'X'.code.toByte()

        assertThrows(IllegalArgumentException::class.java) {
            AndroidG8PairCourt.validateChallenge(challenge)
        }
    }

    @Test
    fun validatorRejectsWrongLength() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidG8PairCourt.validateChallenge(ByteArray(35))
        }
    }
}