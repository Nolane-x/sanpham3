package dev.nolane.sanpham3.androidhost

import java.util.UUID

internal object AndroidBleGattProtocol {
    val SERVICE_UUID: UUID = UUID.fromString(
        "7d7938f8-4e12-4f1d-a12b-940c54fd2601",
    )
    val COMMAND_UUID: UUID = UUID.fromString(
        "7d7938f8-4e12-4f1d-a12b-940c54fd2602",
    )
    val RESPONSE_UUID: UUID = UUID.fromString(
        "7d7938f8-4e12-4f1d-a12b-940c54fd2603",
    )

    const val OPCODE_HANDSHAKE: Int = 0x01
    const val OPCODE_FRAME: Int = 0x02
    const val TARGET_MTU: Int = 128
    const val MIN_REQUIRED_MTU: Int = 96

    data class Envelope(
        val opcode: Int,
        val payload: ByteArray,
    )

    fun encode(
        opcode: Int,
        payload: ByteArray,
    ): ByteArray {
        require(opcode in 1..255)
        require(payload.size <= 0xffff) {
            "GATT payload exceeds u16 envelope length"
        }

        return byteArrayOf(
            opcode.toByte(),
            ((payload.size ushr 8) and 0xff).toByte(),
            (payload.size and 0xff).toByte(),
        ) + payload
    }

    fun decode(bytes: ByteArray): Envelope {
        require(bytes.size >= 3) {
            "GATT envelope is shorter than header"
        }
        val length =
            ((bytes[1].toInt() and 0xff) shl 8) or
                (bytes[2].toInt() and 0xff)
        require(bytes.size == 3 + length) {
            "GATT envelope length mismatch"
        }

        return Envelope(
            opcode = bytes[0].toInt() and 0xff,
            payload = bytes.copyOfRange(3, bytes.size),
        )
    }

    fun usablePayloadBytes(mtu: Int): Int =
        (mtu - 3 - 3).coerceAtLeast(0)

    fun requireFitsMtu(
        envelope: ByteArray,
        mtu: Int,
    ) {
        require(mtu >= MIN_REQUIRED_MTU) {
            "negotiated BLE GATT MTU $mtu is below prototype minimum $MIN_REQUIRED_MTU"
        }
        require(envelope.size <= mtu - 3) {
            "GATT envelope ${envelope.size} exceeds ATT payload ${mtu - 3}"
        }
    }
}