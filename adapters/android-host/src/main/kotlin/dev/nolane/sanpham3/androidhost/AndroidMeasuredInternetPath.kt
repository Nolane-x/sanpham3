package dev.nolane.sanpham3.androidhost

enum class AndroidMeasuredLinkState {
    UP,
    INTERMITTENT,
}

data class AndroidMeasuredInternetPath(
    val pathId: String,
    val networkHandle: Long,
    val transport: AndroidTransport,
    val state: AndroidMeasuredLinkState,
    val observedUsefulBitrateBps: Long,
    val lossPpm: Int,
    val latencyMillis: Long,
    val metered: Boolean,
    val defaultNetwork: Boolean,
    val interfaceName: String?,
)

fun AndroidRecoverySnapshot.measuredInternetPaths():
    List<AndroidMeasuredInternetPath> {
    val paths = mutableListOf<AndroidMeasuredInternetPath>()

    for (network in networks) {
        val best = probes
            .asSequence()
            .filter { probe ->
                probe.networkHandle == network.networkHandle &&
                    probe.kind == AndroidProbeKind.TINY_HTTPS &&
                    probe.status == AndroidProbeStatus.SUCCEEDED &&
                    probe.seriesSummary?.successes?.let { it > 0 } == true
            }
            .sortedWith(
                compareByDescending<AndroidProbeRecord> {
                    it.seriesSummary!!.observedUsefulBitrateBps
                }
                    .thenBy {
                        it.seriesSummary!!.lossPpm
                    }
                    .thenBy {
                        it.seriesSummary!!.medianMillis ?: Long.MAX_VALUE
                    },
            )
            .firstOrNull()
            ?: continue

        val summary = requireNotNull(best.seriesSummary)
        val state = if (
            summary.intermittent ||
            summary.lossPpm > 0
        ) {
            AndroidMeasuredLinkState.INTERMITTENT
        } else {
            AndroidMeasuredLinkState.UP
        }

        paths += AndroidMeasuredInternetPath(
            pathId = "android:${network.networkHandle}",
            networkHandle = network.networkHandle,
            transport = network.preferredTransport(),
            state = state,
            observedUsefulBitrateBps =
                summary.observedUsefulBitrateBps.coerceAtLeast(1),
            lossPpm = summary.lossPpm.coerceIn(0, 1_000_000),
            latencyMillis =
                summary.medianMillis
                    ?: summary.minMillis
                    ?: 1_000L,
            metered = network.metered,
            defaultNetwork = network.defaultNetwork,
            interfaceName = network.interfaceName,
        )
    }

    return paths.sortedBy(AndroidMeasuredInternetPath::networkHandle)
}

private fun AndroidNetworkSnapshot.preferredTransport(): AndroidTransport {
    val priority = listOf(
        AndroidTransport.ETHERNET,
        AndroidTransport.WIFI,
        AndroidTransport.CELLULAR,
        AndroidTransport.SATELLITE,
        AndroidTransport.VPN,
        AndroidTransport.WIFI_AWARE,
        AndroidTransport.USB,
        AndroidTransport.BLUETOOTH,
        AndroidTransport.OTHER,
    )

    return priority.firstOrNull(transports::contains)
        ?: AndroidTransport.OTHER
}