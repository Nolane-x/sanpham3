package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidProbeSeriesTest {
    @Test
    fun summarizesLossLatencyUsefulRateAndIntermittency() {
        val summary = summarizeAndroidProbeSeries(
            listOf(
                AndroidAttemptSample(true, 10, 100, null),
                AndroidAttemptSample(false, 20, 0, "timeout"),
                AndroidAttemptSample(true, 30, 50, null),
                AndroidAttemptSample(false, 40, 0, "timeout"),
                AndroidAttemptSample(false, 50, 0, "timeout"),
            ),
        )

        assertEquals(5, summary.attempts)
        assertEquals(2, summary.successes)
        assertEquals(3, summary.failures)
        assertEquals(600_000, summary.lossPpm)
        assertEquals(10L, summary.minMillis)
        assertEquals(10L, summary.medianMillis)
        assertEquals(30L, summary.p95Millis)
        assertEquals(150L, summary.totalUsefulBytes)
        assertEquals(8_000L, summary.observedUsefulBitrateBps)
        assertEquals(2, summary.longestFailureRun)
        assertEquals(3, summary.stateTransitions)
        assertTrue(summary.intermittent)
    }

    @Test
    fun emptySeriesIsNotIntermittent() {
        val summary = summarizeAndroidProbeSeries(emptyList())

        assertEquals(0, summary.attempts)
        assertEquals(0, summary.lossPpm)
        assertFalse(summary.intermittent)
    }
}
