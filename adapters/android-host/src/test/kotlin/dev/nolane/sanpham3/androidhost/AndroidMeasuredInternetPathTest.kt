package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidMeasuredInternetPathTest {
    private fun network(
        handle: Long,
        transport: AndroidTransport = AndroidTransport.WIFI,
        metered: Boolean = false,
        defaultNetwork: Boolean = false,
    ) = AndroidNetworkSnapshot(
        networkHandle = handle,
        transports = setOf(transport),
        interfaceName = "if-$handle",
        linkAddresses = emptyList(),
        dnsServers = emptyList(),
        mtu = 1500,
        internetCapability = true,
        validated = false,
        captivePortal = false,
        metered = metered,
        roaming = false,
        downstreamKbps = 0,
        upstreamKbps = 0,
        defaultNetwork = defaultNetwork,
    )

    private fun https(
        handle: Long,
        bitrate: Long,
        lossPpm: Int,
        medianMillis: Long,
        intermittent: Boolean,
        successes: Int = 2,
        status: AndroidProbeStatus = AndroidProbeStatus.SUCCEEDED,
    ) = AndroidProbeRecord(
        id = "$handle:https:test",
        kind = AndroidProbeKind.TINY_HTTPS,
        status = status,
        detail = "structured-test",
        networkHandle = handle,
        seriesSummary = AndroidProbeSeriesSummary(
            attempts = if (intermittent) 3 else successes,
            successes = successes,
            failures = if (intermittent) 1 else 0,
            lossPpm = lossPpm,
            minMillis = medianMillis.coerceAtLeast(1),
            medianMillis = medianMillis,
            p95Millis = medianMillis + 20,
            totalUsefulBytes = 100,
            observedUsefulBitrateBps = bitrate,
            longestFailureRun = if (intermittent) 1 else 0,
            stateTransitions = if (intermittent) 2 else 0,
            intermittent = intermittent,
        ),
    )

    @Test
    fun choosesBestVerifiedHttpsSeriesPerNetwork() {
        val snapshot = AndroidRecoverySnapshot(
            networks = listOf(
                network(
                    handle = 7,
                    metered = true,
                    defaultNetwork = true,
                ),
            ),
            probes = listOf(
                https(7, 80, 0, 20, false),
                https(7, 120, 200_000, 40, true),
                https(7, 120, 50_000, 30, true),
            ),
        )

        val paths = snapshot.measuredInternetPaths()

        assertEquals(1, paths.size)
        val path = paths.single()
        assertEquals("android:7", path.pathId)
        assertEquals(120L, path.observedUsefulBitrateBps)
        assertEquals(50_000, path.lossPpm)
        assertEquals(30L, path.latencyMillis)
        assertEquals(AndroidMeasuredLinkState.INTERMITTENT, path.state)
        assertTrue(path.metered)
        assertTrue(path.defaultNetwork)
    }

    @Test
    fun dnsSuccessAloneNeverBecomesMeasuredInternetPath() {
        val snapshot = AndroidRecoverySnapshot(
            networks = listOf(network(9)),
            probes = listOf(
                AndroidProbeRecord(
                    id = "9:dns:resolver",
                    kind = AndroidProbeKind.DNS,
                    status = AndroidProbeStatus.SUCCEEDED,
                    detail = "dns works",
                    networkHandle = 9,
                ),
            ),
        )

        assertTrue(snapshot.measuredInternetPaths().isEmpty())
    }

    @Test
    fun failedOrUnstructuredHttpsDoesNotBecomeInternetEvidence() {
        val snapshot = AndroidRecoverySnapshot(
            networks = listOf(network(10)),
            probes = listOf(
                https(
                    handle = 10,
                    bitrate = 500,
                    lossPpm = 0,
                    medianMillis = 10,
                    intermittent = false,
                    status = AndroidProbeStatus.FAILED,
                ),
                AndroidProbeRecord(
                    id = "10:https:no-summary",
                    kind = AndroidProbeKind.TINY_HTTPS,
                    status = AndroidProbeStatus.SUCCEEDED,
                    detail = "missing structured evidence",
                    networkHandle = 10,
                ),
            ),
        )

        assertTrue(snapshot.measuredInternetPaths().isEmpty())
    }

    @Test
    fun zeroObservedRateIsClampedConservatively() {
        val snapshot = AndroidRecoverySnapshot(
            networks = listOf(
                network(11, AndroidTransport.CELLULAR, metered = true),
            ),
            probes = listOf(
                https(11, 0, 100_000, 90, false),
            ),
        )

        val path = snapshot.measuredInternetPaths().single()
        assertEquals(1L, path.observedUsefulBitrateBps)
        assertEquals(AndroidMeasuredLinkState.INTERMITTENT, path.state)
        assertEquals(AndroidTransport.CELLULAR, path.transport)
    }

    @Test
    fun pathsAreStableAndSortedByNetworkHandle() {
        val snapshot = AndroidRecoverySnapshot(
            networks = listOf(network(20), network(3)),
            probes = listOf(
                https(20, 1_000, 0, 10, false),
                https(3, 2_000, 0, 10, false),
            ),
        )

        assertEquals(
            listOf(3L, 20L),
            snapshot.measuredInternetPaths().map { it.networkHandle },
        )
    }
}