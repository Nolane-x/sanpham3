package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertThrows
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidNfcApduTest {
    @Test
    fun selectAidRoundtripsExpectedAid() {
        val command = AndroidNfcApdu.selectAidCommand()

        assertTrue(AndroidNfcApdu.isSelectAid(command))

        val bad = command.copyOf()
        bad[bad.lastIndex] = (bad.last().toInt() xor 0x01).toByte()
        assertFalse(AndroidNfcApdu.isSelectAid(bad))
    }

    @Test
    fun proprietaryCommandRoundtripsPayload() {
        val payload = byteArrayOf(1, 2, 3, 4)
        val command = AndroidNfcApdu.command(
            AndroidNfcApdu.INS_HANDSHAKE,
            payload,
        )

        val parsed = requireNotNull(AndroidNfcApdu.parseCommand(command))
        assertEquals(AndroidNfcApdu.INS_HANDSHAKE, parsed.instruction)
        assertArrayEquals(payload, parsed.payload)
    }

    @Test
    fun responseParserStripsSuccessStatusWord() {
        val payload = byteArrayOf(7, 8, 9)
        val response = AndroidNfcApdu.success(payload)

        assertArrayEquals(
            payload,
            AndroidNfcApdu.parseSuccessResponse(response),
        )
    }

    @Test
    fun responseParserRejectsFailureStatus() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidNfcApdu.parseSuccessResponse(
                byteArrayOf(0x6a, 0x80.toByte()),
            )
        }
    }

    @Test
    fun shortApduBudgetIsEnforced() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidNfcApdu.command(
                AndroidNfcApdu.INS_FRAME,
                ByteArray(AndroidNfcApdu.MAX_SHORT_PAYLOAD + 1),
            )
        }
    }
}
