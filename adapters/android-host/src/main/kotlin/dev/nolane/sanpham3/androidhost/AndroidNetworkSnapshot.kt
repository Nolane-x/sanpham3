package dev.nolane.sanpham3.androidhost

enum class AndroidTransport {
    WIFI,
    WIFI_DIRECT,
    CELLULAR,
    ETHERNET,
    VPN,
    BLUETOOTH,
    WIFI_AWARE,
    SATELLITE,
    USB,
    OTHER,
}

data class AndroidNetworkSnapshot(
    val networkHandle: Long,
    val transports: Set<AndroidTransport>,
    val interfaceName: String?,
    val linkAddresses: List<String>,
    val dnsServers: List<String>,
    val mtu: Int?,
    val internetCapability: Boolean,
    val validated: Boolean,
    val captivePortal: Boolean,
    val metered: Boolean,
    val roaming: Boolean,
    val downstreamKbps: Int,
    val upstreamKbps: Int,
    val defaultNetwork: Boolean,
)
