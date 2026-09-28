package dev.nolane.sanpham3.androidhost

import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import java.net.InetAddress

enum class AndroidProbeKind {
    IPV4,
    IPV6,
    DNS,
    TINY_HTTPS,
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
    val networkHandle: Long? = null,
    val seriesSummary: AndroidProbeSeriesSummary? = null,
)

data class AndroidRecoverySnapshot(
    val networks: List<AndroidNetworkSnapshot>,
    val probes: List<AndroidProbeRecord>,
) {
    val informationPathFound: Boolean
        get() = probes.any {
            (
                it.kind == AndroidProbeKind.DNS ||
                    it.kind == AndroidProbeKind.TINY_HTTPS
            ) &&
                it.status == AndroidProbeStatus.SUCCEEDED
        }
}

class AndroidRecoveryProbe(
    context: Context,
    private val dnsProbe: AndroidBoundDnsProbe = AndroidBoundDnsProbe(),
    private val httpsProbe: AndroidBoundHttpsProbe = AndroidBoundHttpsProbe(),
    private val httpsTargets: List<AndroidHttpsProbeTarget> = emptyList(),
    private val httpsAttempts: Int = 2,
    private val httpsPauseMillis: Long = 120,
) {
    init {
        require(httpsAttempts > 0)
        require(httpsPauseMillis >= 0)
    }
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
                networkHandle = snapshot.networkHandle,
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
                networkHandle = snapshot.networkHandle,
            )

            if (httpsTargets.isEmpty()) {
                records += AndroidProbeRecord(
                    id = "${snapshot.networkHandle}:https",
                    kind = AndroidProbeKind.TINY_HTTPS,
                    status = AndroidProbeStatus.UNSUPPORTED,
                    detail = "no HTTPS probe target configured",
                    networkHandle = snapshot.networkHandle,
                )
            } else {
                for (target in httpsTargets) {
                    records += probeHttpsSeries(network, target)
                }
            }

            val resolvers = linkProperties?.dnsServers.orEmpty()
            if (resolvers.isEmpty()) {
                records += AndroidProbeRecord(
                    id = "${snapshot.networkHandle}:dns",
                    kind = AndroidProbeKind.DNS,
                    status = AndroidProbeStatus.UNSUPPORTED,
                    detail = "network exposes no DNS server",
                    networkHandle = snapshot.networkHandle,
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
                networkHandle = network.networkHandle,
            )
        } catch (error: SecurityException) {
            AndroidProbeRecord(
                id = id,
                kind = AndroidProbeKind.DNS,
                status = AndroidProbeStatus.BLOCKED,
                detail = error.message ?: error.javaClass.simpleName,
                networkHandle = network.networkHandle,
            )
        } catch (error: UnsupportedOperationException) {
            AndroidProbeRecord(
                id = id,
                kind = AndroidProbeKind.DNS,
                status = AndroidProbeStatus.UNSUPPORTED,
                detail = error.message ?: error.javaClass.simpleName,
                networkHandle = network.networkHandle,
            )
        } catch (error: Exception) {
            AndroidProbeRecord(
                id = id,
                kind = AndroidProbeKind.DNS,
                status = AndroidProbeStatus.FAILED,
                detail = error.message ?: error.javaClass.simpleName,
                networkHandle = network.networkHandle,
            )
        }
    }
    private fun probeHttpsSeries(
        network: Network,
        target: AndroidHttpsProbeTarget,
    ): AndroidProbeRecord {
        val samples = buildList {
            repeat(httpsAttempts) { attempt ->
                val started = System.nanoTime()
                val sample = try {
                    val result = httpsProbe.probe(network, target)
                    AndroidAttemptSample(
                        success = true,
                        elapsedMillis = result.elapsedMillis,
                        usefulBytes = result.responseBytes,
                        detail = "status=${result.statusCode}",
                    )
                } catch (error: SecurityException) {
                    AndroidAttemptSample(
                        success = false,
                        elapsedMillis =
                            (System.nanoTime() - started) / 1_000_000,
                        usefulBytes = 0,
                        detail = "blocked:${error.message ?: error.javaClass.simpleName}",
                    )
                } catch (error: UnsupportedOperationException) {
                    AndroidAttemptSample(
                        success = false,
                        elapsedMillis =
                            (System.nanoTime() - started) / 1_000_000,
                        usefulBytes = 0,
                        detail = "unsupported:${error.message ?: error.javaClass.simpleName}",
                    )
                } catch (error: Exception) {
                    AndroidAttemptSample(
                        success = false,
                        elapsedMillis =
                            (System.nanoTime() - started) / 1_000_000,
                        usefulBytes = 0,
                        detail = error.message ?: error.javaClass.simpleName,
                    )
                }

                add(sample)

                if (
                    attempt + 1 < httpsAttempts &&
                    httpsPauseMillis > 0
                ) {
                    Thread.sleep(httpsPauseMillis)
                }
            }
        }

        val summary = summarizeAndroidProbeSeries(samples)
        val firstFailure = samples.firstOrNull { !it.success }?.detail
        val status = when {
            summary.successes > 0 -> AndroidProbeStatus.SUCCEEDED
            samples.any {
                it.detail?.startsWith("blocked:") == true
            } -> AndroidProbeStatus.BLOCKED
            samples.any {
                it.detail?.startsWith("unsupported:") == true
            } -> AndroidProbeStatus.UNSUPPORTED
            else -> AndroidProbeStatus.FAILED
        }

        val targetAddress =
            target.address.hostAddress ?: target.address.toString()
        val id =
            "${network.networkHandle}:https:${target.serverName}:$targetAddress:${target.port}"

        return AndroidProbeRecord(
            id = id,
            kind = AndroidProbeKind.TINY_HTTPS,
            status = status,
            detail = buildString {
                append("attempts=")
                append(summary.attempts)
                append(" successes=")
                append(summary.successes)
                append(" loss_ppm=")
                append(summary.lossPpm)
                append(" useful_bytes=")
                append(summary.totalUsefulBytes)
                append(" useful_bps=")
                append(summary.observedUsefulBitrateBps)
                append(" median_ms=")
                append(summary.medianMillis ?: "-")
                append(" p95_ms=")
                append(summary.p95Millis ?: "-")
                append(" longest_failure_run=")
                append(summary.longestFailureRun)
                append(" transitions=")
                append(summary.stateTransitions)
                append(" intermittent=")
                append(summary.intermittent)
                if (firstFailure != null) {
                    append(" first_failure=")
                    append(firstFailure)
                }
            },
            networkHandle = network.networkHandle,
            seriesSummary = summary,
        )
    }

}
