package dev.nolane.sanpham3.androidhost

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.net.wifi.WifiManager
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
    private val appContext = context.applicationContext
    private val connectivityManager =
        appContext.getSystemService(
            ConnectivityManager::class.java,
        )
    private val wifiManager =
        appContext.getSystemService(WifiManager::class.java)
    private val started = AtomicBoolean(false)
    private var callback: ConnectivityManager.NetworkCallback? = null
    private var failureListener:
        WifiManager.LocalOnlyConnectionFailureListener? = null

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

        val specifier = specifierBuilder.build()
        val request = NetworkRequest.Builder()
            .addTransportType(NetworkCapabilities.TRANSPORT_WIFI)
            .setNetworkSpecifier(specifier)
            .build()

        if (Build.VERSION.SDK_INT >= 34) {
            val listener =
                object : WifiManager.LocalOnlyConnectionFailureListener {
                    override fun onConnectionFailed(
                        wifiNetworkSpecifier: WifiNetworkSpecifier,
                        failureReason: Int,
                    ) {
                        if (wifiNetworkSpecifier != specifier) return
                        onEvent(
                            AndroidLocalHotspotClientEvent.Unavailable(
                                "Local-only Wi-Fi failure: " +
                                    describeLocalOnlyFailure(failureReason),
                            ),
                        )
                    }
                }
            failureListener = listener
            wifiManager?.addLocalOnlyConnectionFailureListener(
                appContext.mainExecutor,
                listener,
            )
        }

        val networkCallback = object : ConnectivityManager.NetworkCallback() {
            override fun onAvailable(network: Network) {
                connectivityManager.getLinkProperties(network)?.let {
                    publishRoute(
                        network = network,
                        properties = it,
                        port = endpoint.port,
                        onEvent = onEvent,
                    )
                }
            }

            override fun onLinkPropertiesChanged(
                network: Network,
                linkProperties: LinkProperties,
            ) {
                publishRoute(
                    network = network,
                    properties = linkProperties,
                    port = endpoint.port,
                    onEvent = onEvent,
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

        if (Build.VERSION.SDK_INT >= 34) {
            failureListener?.let { listener ->
                try {
                    wifiManager?.removeLocalOnlyConnectionFailureListener(
                        listener,
                    )
                } catch (_: IllegalArgumentException) {
                }
            }
        }
        failureListener = null
        route = null
    }

    private fun publishRoute(
        network: Network,
        properties: LinkProperties,
        port: Int,
        onEvent: (AndroidLocalHotspotClientEvent) -> Unit,
    ) {
        val address = findHotspotHost(properties) ?: return
        val value = AndroidLocalHotspotClientRoute(
            network = network,
            serverAddress = address,
            port = port,
        )

        if (route == value) return
        route = value
        onEvent(AndroidLocalHotspotClientEvent.NetworkAvailable(value))
    }

    private fun findHotspotHost(
        properties: LinkProperties,
    ): InetAddress? {
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

internal fun describeLocalOnlyFailure(reason: Int): String =
    when (reason) {
        WifiManager.STATUS_LOCAL_ONLY_CONNECTION_FAILURE_ASSOCIATION ->
            "association"
        WifiManager.STATUS_LOCAL_ONLY_CONNECTION_FAILURE_AUTHENTICATION ->
            "authentication"
        WifiManager.STATUS_LOCAL_ONLY_CONNECTION_FAILURE_IP_PROVISIONING ->
            "ip-provisioning"
        WifiManager.STATUS_LOCAL_ONLY_CONNECTION_FAILURE_NOT_FOUND ->
            "not-found"
        WifiManager.STATUS_LOCAL_ONLY_CONNECTION_FAILURE_NO_RESPONSE ->
            "no-response"
        WifiManager.STATUS_LOCAL_ONLY_CONNECTION_FAILURE_USER_REJECT ->
            "user-reject"
        else -> "unknown($reason)"
    }
