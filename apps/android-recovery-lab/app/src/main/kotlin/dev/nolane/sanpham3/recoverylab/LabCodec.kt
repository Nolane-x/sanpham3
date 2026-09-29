package dev.nolane.sanpham3.recoverylab

object LabCodec {
    private val l2capMagic = byteArrayOf(
        'S'.code.toByte(),
        'P'.code.toByte(),
        '3'.code.toByte(),
        'L'.code.toByte(),
    )

    fun parsePeerKey(value: String): ByteArray {
        val normalized = value.trim()
        require(normalized.length == 64) {
            "PSK must be exactly 64 hexadecimal characters"
        }

        return ByteArray(32) { index ->
            val high = hexNibble(normalized[index * 2])
            val low = hexNibble(normalized[index * 2 + 1])
            ((high shl 4) or low).toByte()
        }
    }

    fun decodeL2capPsm(bytes: ByteArray): Int? {
        if (bytes.size != 7) return null
        for (index in l2capMagic.indices) {
            if (bytes[index] != l2capMagic[index]) return null
        }
        if ((bytes[4].toInt() and 0xff) != 0) return null

        val psm =
            ((bytes[5].toInt() and 0xff) shl 8) or
                (bytes[6].toInt() and 0xff)
        return psm.takeIf { it != 0 }
    }

    fun hex(bytes: ByteArray): String {
        val digits = "0123456789abcdef"
        return buildString(bytes.size * 2) {
            for (byte in bytes) {
                val value = byte.toInt() and 0xff
                append(digits[value ushr 4])
                append(digits[value and 0x0f])
            }
        }
    }

    private fun hexNibble(value: Char): Int =
        when (value) {
            in '0'..'9' -> value - '0'
            in 'a'..'f' -> value - 'a' + 10
            in 'A'..'F' -> value - 'A' + 10
            else -> throw IllegalArgumentException(
                "PSK contains a non-hexadecimal character",
            )
        }
}