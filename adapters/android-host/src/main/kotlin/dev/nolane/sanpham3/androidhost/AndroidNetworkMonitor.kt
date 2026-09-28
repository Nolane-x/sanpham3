package dev.nolane.sanpham3.androidhost

import android.content.Context
import android.net.ConnectivityManager
import android.net.LinkProperties
import android.net.Network
import android.net.NetworkCapabilities
import android.net.NetworkRequest
import android.os.Build
import java.io.Closeable
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean

class AndroidNetworkMonitor(
    context: Context,
) : Closeable {
    private val connectivityManager =
        context.getSystemService(ConnectivityManager::class.java)

    private val started = AtomicBoolean(false)
    private val observed = ConcurrentHashMap<Long, ObservedNetwork>()

    private val callback = object : ConnectivityManager.NetworkCallback() {
        override fun onAvailable(network: Network) {
            refresh(network)
        }

        override fun onCapabilitiesChanged(
            network: Network,
            networkCapabilities: NetworkCapabilities,
        ) {
            refresh(network, capabilities = networkCapabilities)
        }

        override fun onLinkPropertiesChanged(
            network: Network,
            linkProperties: LinkProperties,
        ) {
            refresh(network, linkProperties = linkProperties)
        }

        override fun onLost(network: Network) {
            observed.remove(network.networkHandle)
        }
    }

    data class ObservedNetwork(
        val network: Network,
        val snapshot: AndroidNetworkSnapshot,
    )

    fun start() {
        if (!started.compareAndSet(false, true)) {
            return
        }

        val request = NetworkRequest.Builder()
            .clearCapabilities()
            .build()

        try {
            connectivityManager.registerNetworkCallback(request, callback)
        } catch (error: RuntimeException) {
            started.set(false)
            throw error
        }
    }

    fun current(): List<ObservedNetwork> =
        observed.values
            .sortedBy { it.snapshot.networkHandle }

    fun snapshot(): List<AndroidNetworkSnapshot> =
        current().map(ObservedNetwork::snapshot)

    override fun close() {
        if (!started.compareAndSet(true, false)) {
            return
        }

        try {
            connectivityManager.unregisterNetworkCallback(callback)
        } finally {
            observed.clear()
        }
    }

    private fun refresh(
        network: Network,
        capabilities: NetworkCapabilities? = null,
        linkProperties: LinkProperties? = null,
    ) {
        val actualCapabilities =
            capabilities ?: connectivityManager.getNetworkCapabilities(network)
                ?: return
        val actualLinkProperties =
            linkProperties ?: connectivityManager.getLinkProperties(network)

        val snapshot = AndroidNetworkSnapshot(
            networkHandle = network.networkHandle,
            transports = transports(actualCapabilities),
            interfaceName = actualLinkProperties?.interfaceName,
            linkAddresses = actualLinkProperties
                ?.linkAddresses
                ?.mapNotNull { it.address?.hostAddress }
                ?.sorted()
                .orEmpty(),
            dnsServers = actualLinkProperties
                ?.dnsServers
                ?.mapNotNull { it.hostAddress }
                ?.sorted()
                .orEmpty(),
            mtu = actualLinkProperties?.mtu,
            internetCapability = actualCapabilities.hasCapability(
                NetworkCapabilities.NET_CAPABILITY_INTERNET,
            ),
            validated = actualCapabilities.hasCapability(
                NetworkCapabilities.NET_CAPABILITY_VALIDATED,
            ),
            captivePortal = actualCapabilities.hasCapability(
                NetworkCapabilities.NET_CAPABILITY_CAPTIVE_PORTAL,
            ),
            metered = !actualCapabilities.hasCapability(
                NetworkCapabilities.NET_CAPABILITY_NOT_METERED,
            ),
            roaming = !actualCapabilities.hasCapability(
                NetworkCapabilities.NET_CAPABILITY_NOT_ROAMING,
            ),
            downstreamKbps = actualCapabilities.linkDownstreamBandwidthKbps,
            upstreamKbps = actualCapabilities.linkUpstreamBandwidthKbps,
            defaultNetwork = connectivityManager.activeNetwork == network,
        )

        observed[network.networkHandle] = ObservedNetwork(network, snapshot)
    }

    private fun transports(
        capabilities: NetworkCapabilities,
    ): Set<AndroidTransport> {
        val result = linkedSetOf<AndroidTransport>()

        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI)) {
            result += AndroidTransport.WIFI
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_CELLULAR)) {
            result += AndroidTransport.CELLULAR
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_ETHERNET)) {
            result += AndroidTransport.ETHERNET
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_VPN)) {
            result += AndroidTransport.VPN
        }
        if (capabilities.hasTransport(NetworkCapabilities.TRANSPORT_BLUETOOTH)) {
            result += AndroidTransport.BLUETOOTH
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_WIFI_AWARE)
        ) {
            result += AndroidTransport.WIFI_AWARE
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S &&
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_USB)
        ) {
            result += AndroidTransport.USB
        }
        if (Build.VERSION.SDK_INT >= 35 &&
            capabilities.hasTransport(NetworkCapabilities.TRANSPORT_SATELLITE)
        ) {
            result += AndroidTransport.SATELLITE
        }

        if (result.isEmpty()) {
            result += AndroidTransport.OTHER
        }

        return result
    }
}
