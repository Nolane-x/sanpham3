package dev.nolane.sanpham3.androidhost

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.WifiNetworkSpecifier
import android.os.Build
import java.io.Closeable
import java.net.Inet4Address
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidLocalHotspotClientRoute(
    val network: Network,
    val serverAddress: InetAddress,
    val port: Int,
)

sealed interface AndroidLocalHotspotClientEvent {
    data class NetworkAvailable(
        val route: AndroidLocalHotspotClientRoute,
    ) : AndroidLocalHotspotClientEvent

    data object Lost : AndroidLocalHotspotClientEvent

    data class Unavailable(
        val detail: String,
    ) : AndroidLocalHotspotClientEvent
}

class AndroidLocalHotspotClientDataPath(
    context: Context,
) : Closeable {
    private val connectivityManager =
        context.applicationContext.getSystemService(
            ConnectivityManager::class.java,
        )
    private val started = AtomicBoolean(false)
    private var callback: ConnectivityManager.NetworkCallback? = null

    @Volatile
    private var route: AndroidLocalHotspotClientRoute? = null

    fun start(
        endpoint: AndroidLocalHotspotEndpoint,
        onEvent: (AndroidLocalHotspotClientEvent) -> Unit,
    ) {
        require(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            "programmatic Local-Only Hotspot join requires Android 10+"
        }
        validateLocalHotspotPort(endpoint.port)
        if (!started.compareAndSet(false, true)) return

        val specifierBuilder = WifiNetworkSpecifier.Builder()
            .setSsid(endpoint.ssid)

        when (endpoint.security) {
            AndroidLocalHotspotSecurity.OPEN -> Unit
            AndroidLocalHotspotSecurity.WPA2_PSK -> {
                val passphrase = requireNotNull(endpoint.passphrase) {
                    "WPA2 Local-Only Hotspot passphrase missing"
                }
                specifierBuilder.setWpa2Passphrase(passphrase)
            }
            AndroidLocalHotspotSecurity.WPA3_SAE -> {
                val passphrase = requireNotNull(endpoint.passphrase) {
                    "WPA3 Local-Only Hotspot passphrase missing"
                }
                specifierBuilder.setWpa3Passphrase(passphrase)
            }
        }

        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .setNetworkSpecifier(specifierBuilder.build())
            .build()

        val networkCallback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                val address = findHotspotHost(network)
                if (address == null) {
                    onEvent(
                        AndroidLocalHotspotClientEvent.Unavailable(
                            "local hotspot network has no DHCP server/gateway address",
                        ),
                    )
                    return
                }

                val value = AndroidLocalHotspotClientRoute(
                    network = network,
                    serverAddress = address,
                    port = endpoint.port,
                )
                route = value
                onEvent(
                    AndroidLocalHotspotClientEvent.NetworkAvailable(value),
                )
            }

            override fun onLost(network: Network) {
                route = null
                onEvent(AndroidLocalHotspotClientEvent.Lost)
            }

            override fun onUnavailable() {
                route = null
                onEvent(
                    AndroidLocalHotspotClientEvent.Unavailable(
                        "Local-Only Hotspot Wi-Fi request unavailable or denied",
                    ),
                )
            }
        }

        callback = networkCallback
        try {
            connectivityManager.requestNetwork(request, networkCallback)
        } catch (error: Throwable) {
            close()
            throw error
        }
    }

    fun currentRoute(): AndroidLocalHotspotClientRoute? = route

    fun connect(timeoutMillis: Int): Socket {
        require(timeoutMillis > 0)
        val current = checkNotNull(route) {
            "Local-Only Hotspot client route is not ready"
        }

        val socket = current.network.socketFactory.createSocket()
        try {
            socket.connect(
                InetSocketAddress(
                    current.serverAddress,
                    current.port,
                ),
                timeoutMillis,
            )
            return socket
        } catch (error: Throwable) {
            socket.close()
            throw error
        }
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) return
        callback?.let { current ->
            try {
                connectivityManager.unregisterNetworkCallback(current)
            } catch (_: IllegalArgumentException) {
            }
        }
        callback = null
        route = null
    }

    private fun findHotspotHost(network: Network): InetAddress? {
        val properties =
            connectivityManager.getLinkProperties(network) ?: return null

        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
            properties.dhcpServerAddress?.let { return it }
        }

        return properties.routes
            .asSequence()
            .filter { it.isDefaultRoute }
            .mapNotNull { it.gateway }
            .filterIsInstance<Inet4Address>()
            .firstOrNull()
    }
}