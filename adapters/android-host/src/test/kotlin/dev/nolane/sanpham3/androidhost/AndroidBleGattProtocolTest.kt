package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertArrayEquals
import org.junit.Assert.assertEquals
import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidBleGattProtocolTest {
    @Test
    fun envelopeRoundtrips() {
        val payload = byteArrayOf(1, 2, 3, 4, 5)
        val bytes = AndroidBleGattProtocol.encode(
            AndroidBleGattProtocol.OPCODE_HANDSHAKE,
            payload,
        )
        val decoded = AndroidBleGattProtocol.decode(bytes)

        assertEquals(
            AndroidBleGattProtocol.OPCODE_HANDSHAKE,
            decoded.opcode,
        )
        assertArrayEquals(payload, decoded.payload)
    }

    @Test
    fun prototypeMtuAcceptsCurrentHandshakeSizedEnvelope() {
        val handshakeSized = ByteArray(70)
        val envelope = AndroidBleGattProtocol.encode(
            AndroidBleGattProtocol.OPCODE_HANDSHAKE,
            handshakeSized,
        )

        AndroidBleGattProtocol.requireFitsMtu(
            envelope,
            AndroidBleGattProtocol.TARGET_MTU,
        )
    }

    @Test
    fun targetMtuHasHeadroomForWorstCaseHotspotBootstrapFrame() {
        val required =
            AndroidBleGattProtocol.requiredMtuForEncryptedProjectPayload(
                AndroidLocalHotspotBootstrap.MAX_ENCODED_BYTES,
            )

        assertEquals(148, required)
        org.junit.Assert.assertTrue(
            AndroidBleGattProtocol.TARGET_MTU >= required,
        )
    }

    @Test
    fun undersizedMtuFailsInsteadOfPretendingFragmentationExists() {
        val envelope = AndroidBleGattProtocol.encode(
            AndroidBleGattProtocol.OPCODE_FRAME,
            ByteArray(68),
        )

        assertThrows(IllegalArgumentException::class.java) {
            AndroidBleGattProtocol.requireFitsMtu(envelope, 64)
        }
    }

    @Test
    fun malformedEnvelopeIsRejected() {
        assertThrows(IllegalArgumentException::class.java) {
            AndroidBleGattProtocol.decode(
                byteArrayOf(1, 0, 3, 9),
            )
        }
    }
}