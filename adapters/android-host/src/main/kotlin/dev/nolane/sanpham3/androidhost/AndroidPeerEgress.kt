package dev.nolane.sanpham3.androidhost

import java.io.ByteArrayOutputStream
import java.io.DataOutputStream
import java.net.Inet4Address
import java.net.Inet6Address
import java.net.InetAddress
import java.nio.ByteBuffer
import java.nio.ByteOrder

/**
 * Android-side implementation of the constrained peer-egress DNS operation.
 *
 * Wire compatibility intentionally mirrors crates/peer-egress:
 * request  = request_id(u32 BE) | relays(u8) | hostname_len(u8) | hostname
 * response = request_id(u32 BE) | status(u8) | count(u8) | addresses...
 *
 * This is not a general-purpose proxy. Only public-host resolution is exposed.
 */
object AndroidPeerEgress {
    const val KIND_RESOLVE_REQUEST: Int = 0x20
    const val KIND_RESOLVE_RESPONSE: Int = 0x21
    const val DEFAULT_RELAY_BUDGET: Int = 8
    const val MAX_HOSTNAME_LEN: Int = 253
    const val MAX_RESULT_ADDRESSES: Int = 8

    enum class ResolveStatus(val wire: Int) {
        OK(0),
        INVALID_HOSTNAME(1),
        RESOLUTION_FAILED(2),
        NO_PUBLIC_ADDRESS(3),
        PROTOCOL_ERROR(4),
        HOP_LIMIT_EXCEEDED(5);

        companion object {
            fun fromWire(value: Int): ResolveStatus =
                entries.firstOrNull { it.wire == value }
                    ?: error("invalid peer-egress status $value")
        }
    }

    data class ResolveRequest(
        val requestId: Long,
        val relaysRemaining: Int,
        val hostname: String,
    )

    data class ResolveResponse(
        val requestId: Long,
        val status: ResolveStatus,
        val addresses: List<InetAddress>,
    )

    data class ServeEvidence(
        val requestId: Long,
        val hostname: String,
        val status: ResolveStatus,
        val addresses: List<InetAddress>,
    )

    fun interface Resolver {
        fun resolve(hostname: String): List<InetAddress>
    }

    object SystemResolver : Resolver {
        override fun resolve(hostname: String): List<InetAddress> =
            InetAddress.getAllByName(hostname).toList()
    }

    fun resolveViaPeer(
        session: AndroidPeerSession,
        requestId: Long,
        hostname: String,
        relaysRemaining: Int = DEFAULT_RELAY_BUDGET,
    ): List<InetAddress> {
        val request = ResolveRequest(
            requestId = requestId,
            relaysRemaining = relaysRemaining,
            hostname = hostname,
        )

        session.send(KIND_RESOLVE_REQUEST, encodeRequest(request))
        val message = session.receive()
        require(message.kind == KIND_RESOLVE_RESPONSE) {
            "unexpected peer-egress response kind ${message.kind}"
        }

        val response = decodeResponse(message.payload)
        require(response.requestId == requestId) {
            "peer-egress request ID mismatch: expected $requestId, got ${response.requestId}"
        }
        require(response.status == ResolveStatus.OK) {
            "peer-egress remote status ${response.status}"
        }
        return response.addresses
    }

    fun serveOne(
        session: AndroidPeerSession,
        resolver: Resolver = SystemResolver,
    ): ServeEvidence {
        val message = session.receive()
        require(message.kind == KIND_RESOLVE_REQUEST) {
            "unexpected peer-egress request kind ${message.kind}"
        }

        val request = try {
            decodeRequest(message.payload)
        } catch (_: IllegalArgumentException) {
            val requestId = message.payload
                .takeIf { it.size >= 4 }
                ?.let { readU32(it, 0) }
                ?: 0L

            val response = ResolveResponse(
                requestId = requestId,
                status = ResolveStatus.INVALID_HOSTNAME,
                addresses = emptyList(),
            )
            session.send(KIND_RESOLVE_RESPONSE, encodeResponse(response))
            return ServeEvidence(
                requestId = requestId,
                hostname = "",
                status = response.status,
                addresses = emptyList(),
            )
        }

        val response = if (request.relaysRemaining <= 0) {
            ResolveResponse(
                requestId = request.requestId,
                status = ResolveStatus.HOP_LIMIT_EXCEEDED,
                addresses = emptyList(),
            )
        } else {
            resolveRequest(request, resolver)
        }

        session.send(KIND_RESOLVE_RESPONSE, encodeResponse(response))

        return ServeEvidence(
            requestId = request.requestId,
            hostname = request.hostname,
            status = response.status,
            addresses = response.addresses,
        )
    }

    fun resolveRequest(
        request: ResolveRequest,
        resolver: Resolver,
    ): ResolveResponse {
        if (!isValidPublicHostname(request.hostname)) {
            return ResolveResponse(
                requestId = request.requestId,
                status = ResolveStatus.INVALID_HOSTNAME,
                addresses = emptyList(),
            )
        }

        val resolved = try {
            resolver.resolve(request.hostname)
        } catch (_: Throwable) {
            return ResolveResponse(
                requestId = request.requestId,
                status = ResolveStatus.RESOLUTION_FAILED,
                addresses = emptyList(),
            )
        }

        val public = resolved
            .filter(::isPublicDestination)
            .distinctBy { it.hostAddress }
            .take(MAX_RESULT_ADDRESSES)

        return if (public.isEmpty()) {
            ResolveResponse(
                requestId = request.requestId,
                status = ResolveStatus.NO_PUBLIC_ADDRESS,
                addresses = emptyList(),
            )
        } else {
            ResolveResponse(
                requestId = request.requestId,
                status = ResolveStatus.OK,
                addresses = public,
            )
        }
    }

    fun encodeRequest(request: ResolveRequest): ByteArray {
        require(request.requestId in 0..0xffff_ffffL) {
            "request ID must fit u32"
        }
        require(request.relaysRemaining in 0..255) {
            "relay budget must fit u8"
        }
        require(isValidPublicHostname(request.hostname)) {
            "invalid public hostname"
        }

        val hostname = request.hostname.toByteArray(Charsets.US_ASCII)
        require(hostname.size <= 255) {
            "hostname exceeds one-byte wire length"
        }

        val out = ByteArrayOutputStream(6 + hostname.size)
        DataOutputStream(out).use { data ->
            data.writeInt(request.requestId.toInt())
            data.writeByte(request.relaysRemaining)
            data.writeByte(hostname.size)
            data.write(hostname)
        }
        return out.toByteArray()
    }

    fun decodeRequest(bytes: ByteArray): ResolveRequest {
        require(bytes.size >= 6) {
            "truncated peer-egress request"
        }

        val requestId = readU32(bytes, 0)
        val relays = bytes[4].toInt() and 0xff
        val hostnameLen = bytes[5].toInt() and 0xff
        require(bytes.size == 6 + hostnameLen) {
            "peer-egress request length mismatch"
        }

        val hostname = bytes
            .copyOfRange(6, bytes.size)
            .toString(Charsets.US_ASCII)
        require(isValidPublicHostname(hostname)) {
            "invalid public hostname"
        }

        return ResolveRequest(
            requestId = requestId,
            relaysRemaining = relays,
            hostname = hostname,
        )
    }

    fun encodeResponse(response: ResolveResponse): ByteArray {
        require(response.requestId in 0..0xffff_ffffL) {
            "request ID must fit u32"
        }
        require(response.addresses.size <= MAX_RESULT_ADDRESSES) {
            "too many response addresses"
        }

        val out = ByteArrayOutputStream()
        DataOutputStream(out).use { data ->
            data.writeInt(response.requestId.toInt())
            data.writeByte(response.status.wire)
            data.writeByte(response.addresses.size)

            for (address in response.addresses) {
                when (address) {
                    is Inet4Address -> {
                        data.writeByte(4)
                        data.write(address.address)
                    }
                    is Inet6Address -> {
                        data.writeByte(6)
                        data.write(address.address)
                    }
                    else -> error("unsupported InetAddress family")
                }
            }
        }
        return out.toByteArray()
    }

    fun decodeResponse(bytes: ByteArray): ResolveResponse {
        require(bytes.size >= 6) {
            "truncated peer-egress response"
        }

        val requestId = readU32(bytes, 0)
        val status = ResolveStatus.fromWire(bytes[4].toInt() and 0xff)
        val count = bytes[5].toInt() and 0xff
        require(count <= MAX_RESULT_ADDRESSES) {
            "peer-egress response contains too many addresses"
        }

        var cursor = 6
        val addresses = ArrayList<InetAddress>(count)

        repeat(count) {
            require(cursor < bytes.size) {
                "truncated peer-egress address family"
            }
            val family = bytes[cursor].toInt() and 0xff
            cursor += 1
            val length = when (family) {
                4 -> 4
                6 -> 16
                else -> error("invalid peer-egress address family $family")
            }
            require(cursor + length <= bytes.size) {
                "truncated peer-egress address"
            }
            addresses += InetAddress.getByAddress(
                bytes.copyOfRange(cursor, cursor + length),
            )
            cursor += length
        }

        require(cursor == bytes.size) {
            "trailing peer-egress response bytes"
        }

        return ResolveResponse(
            requestId = requestId,
            status = status,
            addresses = addresses,
        )
    }

    fun isValidPublicHostname(hostname: String): Boolean {
        if (hostname.isEmpty() ||
            hostname.length > MAX_HOSTNAME_LEN ||
            !hostname.all { it.code in 0..127 }
        ) {
            return false
        }

        val normalized = hostname.trimEnd('.')
        if (normalized.isEmpty() || !normalized.contains('.')) {
            return false
        }

        if (looksLikeIpLiteral(normalized)) {
            return false
        }

        val lower = normalized.lowercase()
        val blockedSuffixes = listOf(
            ".local",
            ".localhost",
            ".internal",
            ".home",
            ".lan",
            ".localdomain",
            ".invalid",
        )
        if (blockedSuffixes.any(lower::endsWith)) {
            return false
        }

        return normalized.split('.').all { label ->
            label.isNotEmpty() &&
                label.length <= 63 &&
                label.first() != '-' &&
                label.last() != '-' &&
                label.all { it.isLetterOrDigit() || it == '-' }
        }
    }

    fun isPublicDestination(address: InetAddress): Boolean =
        when (address) {
            is Inet4Address -> isPublicV4(address.address)
            is Inet6Address -> {
                mappedV4(address.address)?.let(::isPublicV4)
                    ?: isPublicV6(address)
            }
            else -> false
        }

    private fun looksLikeIpLiteral(value: String): Boolean {
        if (value.contains(':')) {
            return true
        }

        val parts = value.split('.')
        return parts.size == 4 &&
            parts.all { part ->
                part.isNotEmpty() &&
                    part.all(Char::isDigit) &&
                    part.toIntOrNull() in 0..255
            }
    }

    private fun isPublicV4(raw: ByteArray): Boolean {
        if (raw.size != 4) return false
        val a = raw[0].toInt() and 0xff
        val b = raw[1].toInt() and 0xff
        val c = raw[2].toInt() and 0xff
        val d = raw[3].toInt() and 0xff

        if (a == 0 ||
            a == 10 ||
            a == 127 ||
            (a == 100 && b in 64..127) ||
            (a == 169 && b == 254) ||
            (a == 172 && b in 16..31) ||
            (a == 192 && b == 168) ||
            (a == 192 && b == 0 && c == 0) ||
            (a == 192 && b == 0 && c == 2) ||
            (a == 198 && b in 18..19) ||
            (a == 198 && b == 51 && c == 100) ||
            (a == 203 && b == 0 && c == 113) ||
            a >= 224 ||
            (a == 255 && b == 255 && c == 255 && d == 255)
        ) {
            return false
        }

        return true
    }

    private fun isPublicV6(address: Inet6Address): Boolean {
        if (address.isAnyLocalAddress ||
            address.isLoopbackAddress ||
            address.isMulticastAddress ||
            address.isLinkLocalAddress
        ) {
            return false
        }

        val raw = address.address
        val first = raw[0].toInt() and 0xff
        if (first and 0xfe == 0xfc) {
            return false
        }

        // 2001:db8::/32 documentation range.
        if ((raw[0].toInt() and 0xff) == 0x20 &&
            (raw[1].toInt() and 0xff) == 0x01 &&
            (raw[2].toInt() and 0xff) == 0x0d &&
            (raw[3].toInt() and 0xff) == 0xb8
        ) {
            return false
        }

        return true
    }

    private fun mappedV4(raw: ByteArray): ByteArray? {
        if (raw.size != 16) return null
        for (index in 0 until 10) {
            if (raw[index].toInt() != 0) return null
        }
        if ((raw[10].toInt() and 0xff) != 0xff ||
            (raw[11].toInt() and 0xff) != 0xff
        ) {
            return null
        }
        return raw.copyOfRange(12, 16)
    }

    private fun readU32(bytes: ByteArray, offset: Int): Long {
        require(offset >= 0 && offset + 4 <= bytes.size) {
            "u32 field outside byte array"
        }
        return ByteBuffer
            .wrap(bytes, offset, 4)
            .order(ByteOrder.BIG_ENDIAN)
            .int
            .toLong() and 0xffff_ffffL
    }
}
