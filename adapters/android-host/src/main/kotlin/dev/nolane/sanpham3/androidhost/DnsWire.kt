package dev.nolane.sanpham3.androidhost

internal object DnsWire {
    const val TRANSACTION_ID: Int = 0x5350

    fun rootAQuery(): ByteArray = byteArrayOf(
        0x53, 0x50,
        0x01, 0x00,
        0x00, 0x01,
        0x00, 0x00,
        0x00, 0x00,
        0x00, 0x00,
        0x00,
        0x00, 0x01,
        0x00, 0x01,
    )

    fun validateResponse(
        bytes: ByteArray,
        length: Int,
    ): Int {
        require(length >= 12) {
            "DNS response shorter than header"
        }

        val transactionId =
            ((bytes[0].toInt() and 0xff) shl 8) or
                (bytes[1].toInt() and 0xff)
        require(transactionId == TRANSACTION_ID) {
            "DNS transaction ID mismatch"
        }

        require((bytes[2].toInt() and 0x80) != 0) {
            "DNS packet is not a response"
        }

        return bytes[3].toInt() and 0x0f
    }
}
