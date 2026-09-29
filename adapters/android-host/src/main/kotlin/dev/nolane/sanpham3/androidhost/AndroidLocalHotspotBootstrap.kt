package dev.nolane.sanpham3.androidhost

import java.nio.charset.StandardCharsets

internal object AndroidLocalHotspotBootstrap {
    private val MAGIC = byteArrayOf(
        'S'.code.toByte(),
        'P'.code.toByte(),
        '3'.code.toByte(),
        'H'.code.toByte(),
    )
    private const val VERSION: Int = 0
    private const val HEADER_BYTES: Int = 10
    const val MAX_ENCODED_BYTES: Int = 110

    fun encode(
        endpoint: AndroidLocalHotspotEndpoint,
    ): ByteArray {
        validateLocalHotspotPort(endpoint.port)

        val ssid = endpoint.ssid.toByteArray(StandardCharsets.UTF_8)
        require(ssid.isNotEmpty() && ssid.size <= 32) {
            "hotspot SSID must encode to 1..32 UTF-8 bytes"
        }

        val passphrase = endpoint.passphrase
            ?.toByteArray(StandardCharsets.US_ASCII)
            ?: ByteArray(0)
        require(passphrase.size <= 63) {
            "hotspot passphrase exceeds 63 ASCII bytes"
        }

        when (endpoint.security) {
            AndroidLocalHotspotSecurity.OPEN ->
                require(passphrase.isEmpty()) {
                    "open hotspot must not carry a passphrase"
                }

            AndroidLocalHotspotSecurity.WPA2_PSK,
            AndroidLocalHotspotSecurity.WPA3_SAE ->
                require(passphrase.size in 8..63) {
                    "secured hotspot passphrase must contain 8..63 ASCII bytes"
                }
        }

        val output = ByteArray(
            HEADER_BYTES + ssid.size + passphrase.size,
        )
        MAGIC.copyInto(output, 0)
        output[4] = VERSION.toByte()
        output[5] = when (endpoint.security) {
            AndroidLocalHotspotSecurity.OPEN -> 0
            AndroidLocalHotspotSecurity.WPA2_PSK -> 1
            AndroidLocalHotspotSecurity.WPA3_SAE -> 2
        }.toByte()
        output[6] = ssid.size.toByte()
        output[7] = passphrase.size.toByte()
        output[8] = ((endpoint.port ushr 8) and 0xff).toByte()
        output[9] = (endpoint.port and 0xff).toByte()
        ssid.copyInto(output, HEADER_BYTES)
        passphrase.copyInto(output, HEADER_BYTES + ssid.size)

        require(output.size <= MAX_ENCODED_BYTES) {
            "hotspot bootstrap capsule exceeds $MAX_ENCODED_BYTES bytes"
        }
        return output
    }

    fun decode(bytes: ByteArray): AndroidLocalHotspotEndpoint {
        require(bytes.size in HEADER_BYTES..MAX_ENCODED_BYTES) {
            "hotspot bootstrap capsule has invalid length"
        }
        require(bytes.copyOfRange(0, 4).contentEquals(MAGIC)) {
            "hotspot bootstrap capsule has wrong magic"
        }
        require((bytes[4].toInt() and 0xff) == VERSION) {
            "hotspot bootstrap capsule has unsupported version"
        }

        val security = when (bytes[5].toInt() and 0xff) {
            0 -> AndroidLocalHotspotSecurity.OPEN
            1 -> AndroidLocalHotspotSecurity.WPA2_PSK
            2 -> AndroidLocalHotspotSecurity.WPA3_SAE
            else -> error("hotspot bootstrap capsule has unknown security")
        }
        val ssidLength = bytes[6].toInt() and 0xff
        val passphraseLength = bytes[7].toInt() and 0xff
        require(
            bytes.size ==
                HEADER_BYTES + ssidLength + passphraseLength,
        ) {
            "hotspot bootstrap capsule length mismatch"
        }

        val port =
            ((bytes[8].toInt() and 0xff) shl 8) or
                (bytes[9].toInt() and 0xff)
        validateLocalHotspotPort(port)

        val ssid = bytes
            .copyOfRange(HEADER_BYTES, HEADER_BYTES + ssidLength)
            .toString(StandardCharsets.UTF_8)
        val passphrase = bytes
            .copyOfRange(
                HEADER_BYTES + ssidLength,
                bytes.size,
            )
            .takeIf { it.isNotEmpty() }
            ?.toString(StandardCharsets.US_ASCII)

        val endpoint = AndroidLocalHotspotEndpoint(
            ssid = ssid,
            passphrase = passphrase,
            security = security,
            port = port,
        )

        // Re-encode to apply all semantic constraints.
        encode(endpoint)
        return endpoint
    }
}
