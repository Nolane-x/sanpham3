package dev.nolane.sanpham3.androidhost

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.aware.PeerHandle
import android.net.wifi.aware.PublishDiscoverySession
import android.net.wifi.aware.SubscribeDiscoverySession
import android.net.wifi.aware.WifiAwareNetworkInfo
import android.net.wifi.aware.WifiAwareNetworkSpecifier
import android.os.Build
import java.io.Closeable
import java.net.Inet6Address
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidWifiAwareEndpoint(
    val network: Network,
    val peerIpv6: Inet6Address,
    val peerPort: Int,
)

sealed interface AndroidWifiAwareDataPathEvent {
    data class NetworkAvailable(
        val network: Network,
    ) : AndroidWifiAwareDataPathEvent

    data class PeerEndpointReady(
        val endpoint: AndroidWifiAwareEndpoint,
    ) : AndroidWifiAwareDataPathEvent

    data object Lost : AndroidWifiAwareDataPathEvent

    data class Unavailable(
        val detail: String,
    ) : AndroidWifiAwareDataPathEvent
}

/**
 * Publisher/server half of a Wi-Fi Aware data path.
 *
 * The Wi-Fi Aware PSK protects the radio data path. Project traffic should
 * still use the existing authenticated/encrypted peer-session protocol.
 */
class AndroidWifiAwareServerDataPath(
    context: Context,
) : Closeable {
    private val connectivityManager =
        context.applicationContext.getSystemService(
            ConnectivityManager::class.java,
        )

    private val started = AtomicBoolean(false)
    private var callback: ConnectivityManager.NetworkCallback? = null
    private var serverSocket: ServerSocket? = null

    val localPort: Int
        get() = serverSocket?.localPort ?: 0

    fun start(
        discoverySession: PublishDiscoverySession,
        peerHandle: PeerHandle,
        passphrase: String,
        onEvent: (AndroidWifiAwareDataPathEvent) -> Unit,
    ) {
        requireSupported()
        validatePassphrase(passphrase)

        if (!started.compareAndSet(false, true)) {
            return
        }

        val socket = try {
            ServerSocket(0)
        } catch (error: Exception) {
            started.set(false)
            throw error
        }
        serverSocket = socket

        val specifier = WifiAwareNetworkSpecifier.Builder(
            discoverySession,
            peerHandle,
        )
            .setPskPassphrase(passphrase)
            .setPort(socket.localPort)
            .build()

        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI_AWARE)
            .setNetworkSpecifier(specifier)
            .build()

        val networkCallback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                onEvent(
                    AndroidWifiAwareDataPathEvent.NetworkAvailable(network),
                )
            }

            override fun onLost(network: Network) {
                onEvent(AndroidWifiAwareDataPathEvent.Lost)
            }

            override fun onUnavailable() {
                onEvent(
                    AndroidWifiAwareDataPathEvent.Unavailable(
                        "Wi-Fi Aware server network request unavailable",
                    ),
                )
            }
        }

        callback = networkCallback

        try {
            connectivityManager.requestNetwork(request, networkCallback)
        } catch (error: RuntimeException) {
            close()
            throw error
        }
    }

    fun accept(timeoutMillis: Int): Socket {
        require(timeoutMillis > 0)
        val socket = checkNotNull(serverSocket) {
            "Wi-Fi Aware server data path is not started"
        }
        socket.soTimeout = timeoutMillis
        return socket.accept()
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) {
            return
        }

        callback?.let { networkCallback ->
            try {
                connectivityManager.unregisterNetworkCallback(networkCallback)
            } catch (_: IllegalArgumentException) {
                // Callback may already have been removed by the system.
            }
        }
        callback = null

        try {
            serverSocket?.close()
        } finally {
            serverSocket = null
        }
    }
}

/**
 * Subscriber/client half of a Wi-Fi Aware data path.
 *
 * Once NetworkCapabilities exposes WifiAwareNetworkInfo, the peer's scoped
 * IPv6 address and advertised port are used to open a socket from the exact
 * Wi-Fi Aware Network.
 */
class AndroidWifiAwareClientDataPath(
    context: Context,
) : Closeable {
    private val connectivityManager =
        context.applicationContext.getSystemService(
            ConnectivityManager::class.java,
        )

    private val started = AtomicBoolean(false)
    private var callback: ConnectivityManager.NetworkCallback? = null

    @Volatile
    private var endpoint: AndroidWifiAwareEndpoint? = null

    fun start(
        discoverySession: SubscribeDiscoverySession,
        peerHandle: PeerHandle,
        passphrase: String,
        onEvent: (AndroidWifiAwareDataPathEvent) -> Unit,
    ) {
        requireSupported()
        validatePassphrase(passphrase)

        if (!started.compareAndSet(false, true)) {
            return
        }

        val specifier = WifiAwareNetworkSpecifier.Builder(
            discoverySession,
            peerHandle,
        )
            .setPskPassphrase(passphrase)
            .build()

        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI_AWARE)
            .setNetworkSpecifier(specifier)
            .build()

        val networkCallback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                onEvent(
                    AndroidWifiAwareDataPathEvent.NetworkAvailable(network),
                )
            }

            override fun onCapabilitiesChanged(
                network: Network,
                networkCapabilities: NetworkCapabilities,
            ) {
                if (Build.VERSION.SDK_INT < Build.VERSION_CODES.Q) {
                    return
                }

                val info = networkCapabilities.transportInfo
                if (info !is WifiAwareNetworkInfo) {
                    return
                }

                val address = info.peerIpv6Addr ?: return
                val port = info.port
                if (port !in 1..65535) {
                    return
                }

                val value = AndroidWifiAwareEndpoint(
                    network = network,
                    peerIpv6 = address,
                    peerPort = port,
                )
                endpoint = value
                onEvent(
                    AndroidWifiAwareDataPathEvent.PeerEndpointReady(value),
                )
            }

            override fun onLost(network: Network) {
                endpoint = null
                onEvent(AndroidWifiAwareDataPathEvent.Lost)
            }

            override fun onUnavailable() {
                endpoint = null
                onEvent(
                    AndroidWifiAwareDataPathEvent.Unavailable(
                        "Wi-Fi Aware client network request unavailable",
                    ),
                )
            }
        }

        callback = networkCallback

        try {
            connectivityManager.requestNetwork(request, networkCallback)
        } catch (error: RuntimeException) {
            close()
            throw error
        }
    }

    fun connect(timeoutMillis: Int): Socket {
        require(timeoutMillis > 0)

        val current = checkNotNull(endpoint) {
            "Wi-Fi Aware peer endpoint is not ready"
        }

        val socket = current.network.socketFactory.createSocket()
        try {
            socket.connect(
                InetSocketAddress(
                    current.peerIpv6,
                    current.peerPort,
                ),
                timeoutMillis,
            )
            return socket
        } catch (error: Exception) {
            socket.close()
            throw error
        }
    }

    fun currentEndpoint(): AndroidWifiAwareEndpoint? = endpoint

    override fun close() {
        if (!started.compareAndSet(true, false)) {
            return
        }

        callback?.let { networkCallback ->
            try {
                connectivityManager.unregisterNetworkCallback(networkCallback)
            } catch (_: IllegalArgumentException) {
                // Callback may already have been removed by the system.
            }
        }
        callback = null
        endpoint = null
    }
}

internal fun validatePassphrase(passphrase: String) {
    require(passphrase.length in 8..63) {
        "Wi-Fi Aware passphrase must contain 8..63 characters"
    }
    require(passphrase.all { character ->
        character.code in 32..126
    }) {
        "Wi-Fi Aware passphrase must contain printable ASCII only"
    }
}

private fun requireSupported() {
    require(Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        "Wi-Fi Aware requires Android 8.0+"
    }
}
