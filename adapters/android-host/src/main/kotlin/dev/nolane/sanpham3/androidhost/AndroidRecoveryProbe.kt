package dev.nolane.sanpham3.androidhost

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import java.net.InetAddress

enum class AndroidProbeKind {
    IPV4,
    IPV6,
    DNS,
}

enum class AndroidProbeStatus {
    SUCCEEDED,
    FAILED,
    BLOCKED,
    UNSUPPORTED,
}

data class AndroidProbeRecord(
    val id: String,
    val kind: AndroidProbeKind,
    val status: AndroidProbeStatus,
    val detail: String,
)

data class AndroidRecoverySnapshot(
    val networks: List<AndroidNetworkSnapshot>,
    val probes: List<AndroidProbeRecord>,
) {
    val informationPathFound: Boolean
        get() = probes.any {
            it.kind == AndroidProbeKind.DNS &&
                it.status == AndroidProbeStatus.SUCCEEDED
        }
}

class AndroidRecoveryProbe(
    context: Context,
    private val dnsProbe: AndroidBoundDnsProbe = AndroidBoundDnsProbe(),
) {
    private val connectivityManager =
        context.getSystemService(ConnectivityManager::class.java)

    fun run(
        observed: List<AndroidNetworkMonitor.ObservedNetwork>,
    ): AndroidRecoverySnapshot {
        val records = mutableListOf<AndroidProbeRecord>()

        for (item in observed) {
            val network = item.network
            val snapshot = item.snapshot
            val linkProperties =
                connectivityManager.getLinkProperties(network)

            val ipv4 = linkProperties
                ?.linkAddresses
                ?.any { it.address.address.size == 4 } == true
            val ipv6 = linkProperties
                ?.linkAddresses
                ?.any { it.address.address.size == 16 } == true

            records += AndroidProbeRecord(
                id = "${snapshot.networkHandle}:ipv4:configured",
                kind = AndroidProbeKind.IPV4,
                status = if (ipv4) {
                    AndroidProbeStatus.SUCCEEDED
                } else {
                    AndroidProbeStatus.FAILED
                },
                detail = "configured=$ipv4 interface=${snapshot.interfaceName}",
            )
            records += AndroidProbeRecord(
                id = "${snapshot.networkHandle}:ipv6:configured",
                kind = AndroidProbeKind.IPV6,
                status = if (ipv6) {
                    AndroidProbeStatus.SUCCEEDED
                } else {
                    AndroidProbeStatus.FAILED
                },
                detail = "configured=$ipv6 interface=${snapshot.interfaceName}",
            )

            val resolvers = linkProperties?.dnsServers.orEmpty()
            if (resolvers.isEmpty()) {
                records += AndroidProbeRecord(
                    id = "${snapshot.networkHandle}:dns",
                    kind = AndroidProbeKind.DNS,
                    status = AndroidProbeStatus.UNSUPPORTED,
                    detail = "network exposes no DNS server",
                )
                continue
            }

            for (resolver in resolvers) {
                records += probeDns(network, resolver)
            }
        }

        return AndroidRecoverySnapshot(
            networks = observed.map { it.snapshot },
            probes = records,
        )
    }

    private fun probeDns(
        network: Network,
        resolver: InetAddress,
    ): AndroidProbeRecord {
        val id = "${network.networkHandle}:dns:${resolver.hostAddress}"

        return try {
            val result = dnsProbe.probe(network, resolver)
            AndroidProbeRecord(
                id = id,
                kind = AndroidProbeKind.DNS,
                status = AndroidProbeStatus.SUCCEEDED,
                detail = buildString {
                    append("resolver=")
                    append(result.resolver)
                    append(" elapsed_ms=")
                    append(result.elapsedMillis)
                    append(" bytes=")
                    append(result.responseBytes)
                    append(" rcode=")
                    append(result.rcode)
                },
            )
        } catch (error: SecurityException) {
            AndroidProbeRecord(
                id = id,
                kind = AndroidProbeKind.DNS,
                status = AndroidProbeStatus.BLOCKED,
                detail = error.message ?: error.javaClass.simpleName,
            )
        } catch (error: UnsupportedOperationException) {
            AndroidProbeRecord(
                id = id,
                kind = AndroidProbeKind.DNS,
                status = AndroidProbeStatus.UNSUPPORTED,
                detail = error.message ?: error.javaClass.simpleName,
            )
        } catch (error: Exception) {
            AndroidProbeRecord(
                id = id,
                kind = AndroidProbeKind.DNS,
                status = AndroidProbeStatus.FAILED,
                detail = error.message ?: error.javaClass.simpleName,
            )
        }
    }
}
