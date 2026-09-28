package dev.nolane.sanpham3.androidhost

import android.net.Network
import java.net.DatagramPacket
import java.net.DatagramSocket
import java.net.InetAddress
import java.net.InetSocketAddress
import kotlin.time.Duration.Companion.milliseconds

data class AndroidDnsProbeResult(
    val networkHandle: Long,
    val resolver: String,
    val elapsedMillis: Long,
    val responseBytes: Int,
    val rcode: Int,
)

class AndroidBoundDnsProbe(
    private val timeoutMillis: Int = 900,
) {
    init {
        require(timeoutMillis > 0)
    }

    fun probe(
        network: Network,
        resolver: InetAddress,
    ): AndroidDnsProbeResult {
        val socket = DatagramSocket(null)

        socket.use {
            network.bindSocket(socket)
            socket.soTimeout = timeoutMillis
            socket.bind(InetSocketAddress(0))

            val query = DnsWire.rootAQuery()
            val request = DatagramPacket(
                query,
                query.size,
                InetSocketAddress(resolver, 53),
            )

            val started = System.nanoTime()
            socket.send(request)

            val responseBytes = ByteArray(512)
            val response = DatagramPacket(
                responseBytes,
                responseBytes.size,
            )
            socket.receive(response)

            require(response.address == resolver) {
                "DNS response came from unexpected resolver"
            }

            val rcode = DnsWire.validateResponse(
                responseBytes,
                response.length,
            )
            val elapsedNanos = System.nanoTime() - started

            return AndroidDnsProbeResult(
                networkHandle = network.networkHandle,
                resolver = resolver.hostAddress ?: resolver.toString(),
                elapsedMillis = elapsedNanos / 1_000_000,
                responseBytes = response.length,
                rcode = rcode,
            )
        }
    }
}
