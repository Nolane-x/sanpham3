package dev.nolane.sanpham3.androidhost

internal object AndroidNfcApdu {
    val aid: ByteArray = byteArrayOf(
        0xf0.toByte(),
        'S'.code.toByte(),
        'P'.code.toByte(),
        '3'.code.toByte(),
        'F'.code.toByte(),
        'R'.code.toByte(),
        'N'.code.toByte(),
        'T'.code.toByte(),
    )

    const val CLA_PROJECT: Int = 0x80
    const val INS_HANDSHAKE: Int = 0x10
    const val INS_FRAME: Int = 0x20
    const val MAX_SHORT_PAYLOAD: Int = 240

    private val success = byteArrayOf(0x90.toByte(), 0x00)
    private val badData = byteArrayOf(0x6a, 0x80.toByte())
    private val instructionUnsupported = byteArrayOf(0x6d, 0x00)

    data class Command(
        val instruction: Int,
        val payload: ByteArray,
    )

    fun selectAidCommand(): ByteArray {
        require(aid.size <= 255)
        return byteArrayOf(
            0x00,
            0xa4.toByte(),
            0x04,
            0x00,
            aid.size.toByte(),
        ) + aid
    }

    fun isSelectAid(command: ByteArray): Boolean {
        if (command.size != 5 + aid.size) return false
        if ((command[0].toInt() and 0xff) != 0x00) return false
        if ((command[1].toInt() and 0xff) != 0xa4) return false
        if ((command[2].toInt() and 0xff) != 0x04) return false
        if ((command[3].toInt() and 0xff) != 0x00) return false
        if ((command[4].toInt() and 0xff) != aid.size) return false
        return command.copyOfRange(5, command.size).contentEquals(aid)
    }

    fun command(
        instruction: Int,
        payload: ByteArray,
    ): ByteArray {
        require(instruction in 0..255)
        require(payload.size <= MAX_SHORT_PAYLOAD) {
            "NFC short APDU payload exceeds $MAX_SHORT_PAYLOAD bytes"
        }

        return byteArrayOf(
            CLA_PROJECT.toByte(),
            instruction.toByte(),
            0x00,
            0x00,
            payload.size.toByte(),
        ) + payload
    }

    fun parseCommand(command: ByteArray): Command? {
        if (command.size < 5) return null
        if ((command[0].toInt() and 0xff) != CLA_PROJECT) return null
        val length = command[4].toInt() and 0xff
        if (length > MAX_SHORT_PAYLOAD) return null
        if (command.size != 5 + length) return null

        return Command(
            instruction = command[1].toInt() and 0xff,
            payload = command.copyOfRange(5, command.size),
        )
    }

    fun success(payload: ByteArray = ByteArray(0)): ByteArray =
        payload + success

    fun badData(): ByteArray = badData.copyOf()

    fun unsupported(): ByteArray = instructionUnsupported.copyOf()

    fun parseSuccessResponse(response: ByteArray): ByteArray {
        require(response.size >= 2) { "NFC response missing status word" }
        val sw1 = response[response.size - 2].toInt() and 0xff
        val sw2 = response[response.size - 1].toInt() and 0xff
        require(sw1 == 0x90 && sw2 == 0x00) {
            "NFC APDU failed with status %02x%02x".format(sw1, sw2)
        }
        return response.copyOfRange(0, response.size - 2)
    }
}