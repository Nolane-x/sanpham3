package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidPeerSessionBenchmarkTest {
    @Test
    fun payloadBindsSequenceAndDeterministicBody() {
        val first = AndroidPeerSessionBenchmark.benchmarkPayload(
            sequence = 7,
            payloadBytes = 64,
        )
        val second = AndroidPeerSessionBenchmark.benchmarkPayload(
            sequence = 7,
            payloadBytes = 64,
        )
        val other = AndroidPeerSessionBenchmark.benchmarkPayload(
            sequence = 8,
            payloadBytes = 64,
        )

        assertTrue(first.contentEquals(second))
        assertTrue(!first.contentEquals(other))
        assertEquals(
            7L,
            AndroidPeerSessionBenchmark.benchmarkSequence(first),
        )
    }

    @Test
    fun evidenceUsesNearestRankLatencyAndUsefulByteAccounting() {
        val config = AndroidPeerBenchmarkConfig(
            rounds = 5,
            payloadBytes = 100,
        )
        val evidence = AndroidPeerSessionBenchmark.benchmarkEvidence(
            peerNodeId = 200,
            config = config,
            elapsedNanos = 1_000_000_000L,
            rtts = longArrayOf(
                50_000_000L,
                10_000_000L,
                30_000_000L,
                20_000_000L,
                40_000_000L,
            ),
        )

        assertEquals(500L, evidence.oneWayUsefulBytes)
        assertEquals(1_000L, evidence.roundTripUsefulBytes)
        assertEquals(10_000_000L, evidence.minRttNanos)
        assertEquals(30_000_000L, evidence.medianRttNanos)
        assertEquals(50_000_000L, evidence.p95RttNanos)
        assertEquals(50_000_000L, evidence.maxRttNanos)
        assertEquals(
            4_000.0,
            evidence.oneWayUsefulBitsPerSecond,
            0.001,
        )
        assertEquals(
            8_000.0,
            evidence.roundTripUsefulBitsPerSecond,
            0.001,
        )
    }

    @Test(expected = IllegalArgumentException::class)
    fun configRejectsZeroRounds() {
        AndroidPeerBenchmarkConfig(
            rounds = 0,
            payloadBytes = 64,
        )
    }

    @Test(expected = IllegalArgumentException::class)
    fun configRejectsTinyPayload() {
        AndroidPeerBenchmarkConfig(
            rounds = 1,
            payloadBytes = 8,
        )
    }
}
