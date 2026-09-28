package dev.nolane.sanpham3.androidhost

import android.net.Network
import java.io.ByteArrayOutputStream
import java.net.InetAddress
import java.net.InetSocketAddress
import javax.net.ssl.SNIHostName
import javax.net.ssl.SSLSocket
import javax.net.ssl.SSLSocketFactory

data class AndroidHttpsProbeTarget(
    val address: InetAddress,
    val port: Int = 443,
    val serverName: String,
    val path: String = "/",
    val maxResponseBytes: Int = 1024,
) {
    init {
        require(port in 1..65535)
        require(serverName.isNotBlank())
        require(path.startsWith("/"))
        require(maxResponseBytes >= 16)
    }
}

data class AndroidHttpsProbeResult(
    val networkHandle: Long,
    val targetAddress: String,
    val serverName: String,
    val elapsedMillis: Long,
    val responseBytes: Int,
    val statusCode: Int,
)

class AndroidBoundHttpsProbe(
    private val timeoutMillis: Int = 1500,
) {
    init {
        require(timeoutMillis > 0)
    }

    fun probe(
        network: Network,
        target: AndroidHttpsProbeTarget,
    ): AndroidHttpsProbeResult {
        val started = System.nanoTime()
        val rawSocket = network.socketFactory.createSocket()

        rawSocket.soTimeout = timeoutMillis
        rawSocket.connect(
            InetSocketAddress(target.address, target.port),
            timeoutMillis,
        )

        val sslFactory =
            SSLSocketFactory.getDefault() as SSLSocketFactory
        val tls = sslFactory.createSocket(
            rawSocket,
            target.serverName,
            target.port,
            true,
        ) as SSLSocket

        tls.use { socket ->
            socket.soTimeout = timeoutMillis

            val parameters = socket.sslParameters
            parameters.endpointIdentificationAlgorithm = "HTTPS"
            parameters.serverNames = listOf(
                SNIHostName(target.serverName),
            )
            socket.sslParameters = parameters
            socket.startHandshake()

            val request = buildString {
                append("HEAD ")
                append(target.path)
                append(" HTTP/1.1\r\nHost: ")
                append(target.serverName)
                append("\r\nUser-Agent: sanpham3-android-probe/0")
                append("\r\nAccept: */*")
                append("\r\nConnection: close\r\n\r\n")
            }.encodeToByteArray()

            socket.outputStream.write(request)
            socket.outputStream.flush()

            val response = readHeaders(
                socket,
                target.maxResponseBytes,
            )
            val status = parseHttpStatus(response)
            val elapsedNanos = System.nanoTime() - started

            return AndroidHttpsProbeResult(
                networkHandle = network.networkHandle,
                targetAddress =
                    target.address.hostAddress ?: target.address.toString(),
                serverName = target.serverName,
                elapsedMillis = elapsedNanos / 1_000_000,
                responseBytes = response.size,
                statusCode = status,
            )
        }
    }

    private fun readHeaders(
        socket: SSLSocket,
        maxBytes: Int,
    ): ByteArray {
        val output = ByteArrayOutputStream(
            minOf(maxBytes, 1024),
        )
        val buffer = ByteArray(512)

        while (output.size() < maxBytes) {
            val remaining = maxBytes - output.size()
            val count = socket.inputStream.read(
                buffer,
                0,
                minOf(buffer.size, remaining),
            )
            if (count < 0) {
                break
            }
            if (count == 0) {
                continue
            }

            output.write(buffer, 0, count)
            val bytes = output.toByteArray()
            if (containsHeaderTerminator(bytes)) {
                return bytes
            }
        }

        return output.toByteArray()
    }

    private fun containsHeaderTerminator(bytes: ByteArray): Boolean {
        if (bytes.size < 4) {
            return false
        }

        for (index in 0..bytes.size - 4) {
            if (
                bytes[index] == '\r'.code.toByte() &&
                bytes[index + 1] == '\n'.code.toByte() &&
                bytes[index + 2] == '\r'.code.toByte() &&
                bytes[index + 3] == '\n'.code.toByte()
            ) {
                return true
            }
        }
        return false
    }

    internal fun parseHttpStatus(bytes: ByteArray): Int {
        val text = bytes.toString(Charsets.ISO_8859_1)
        val firstLine = text
            .substringBefore("\r\n")
            .trim()

        val parts = firstLine.split(Regex("\\s+"))
        require(parts.size >= 2 && parts[0].startsWith("HTTP/")) {
            "invalid HTTP status line"
        }

        val status = parts[1].toIntOrNull()
            ?: throw IllegalArgumentException(
                "invalid HTTP status code",
            )
        require(status in 100..599) {
            "HTTP status outside valid range"
        }

        return status
    }
}
