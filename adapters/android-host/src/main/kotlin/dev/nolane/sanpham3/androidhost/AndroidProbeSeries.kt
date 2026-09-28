package dev.nolane.sanpham3.androidhost

data class AndroidAttemptSample(
    val success: Boolean,
    val elapsedMillis: Long,
    val usefulBytes: Int,
    val detail: String?,
)

data class AndroidProbeSeriesSummary(
    val attempts: Int,
    val successes: Int,
    val failures: Int,
    val lossPpm: Int,
    val minMillis: Long?,
    val medianMillis: Long?,
    val p95Millis: Long?,
    val totalUsefulBytes: Long,
    val observedUsefulBitrateBps: Long,
    val longestFailureRun: Int,
    val stateTransitions: Int,
    val intermittent: Boolean,
)

internal fun summarizeAndroidProbeSeries(
    samples: List<AndroidAttemptSample>,
): AndroidProbeSeriesSummary {
    if (samples.isEmpty()) {
        return AndroidProbeSeriesSummary(
            attempts = 0,
            successes = 0,
            failures = 0,
            lossPpm = 0,
            minMillis = null,
            medianMillis = null,
            p95Millis = null,
            totalUsefulBytes = 0,
            observedUsefulBitrateBps = 0,
            longestFailureRun = 0,
            stateTransitions = 0,
            intermittent = false,
        )
    }

    val successes = samples.count(AndroidAttemptSample::success)
    val failures = samples.size - successes
    val lossPpm = ((failures.toLong() * 1_000_000L) / samples.size)
        .coerceIn(0L, 1_000_000L)
        .toInt()

    val successfulTimes = samples
        .asSequence()
        .filter(AndroidAttemptSample::success)
        .map(AndroidAttemptSample::elapsedMillis)
        .sorted()
        .toList()

    val totalUsefulBytes = samples.sumOf {
        it.usefulBytes.coerceAtLeast(0).toLong()
    }
    val totalMillis = samples.sumOf {
        it.elapsedMillis.coerceAtLeast(0)
    }
    val usefulBps = if (totalMillis <= 0) {
        0
    } else {
        ((totalUsefulBytes * 8_000L) / totalMillis)
            .coerceAtLeast(0)
    }

    var longestFailureRun = 0
    var currentFailureRun = 0
    var stateTransitions = 0
    var previous: Boolean? = null

    for (sample in samples) {
        if (sample.success) {
            currentFailureRun = 0
        } else {
            currentFailureRun += 1
            longestFailureRun = maxOf(
                longestFailureRun,
                currentFailureRun,
            )
        }

        if (previous != null && previous != sample.success) {
            stateTransitions += 1
        }
        previous = sample.success
    }

    return AndroidProbeSeriesSummary(
        attempts = samples.size,
        successes = successes,
        failures = failures,
        lossPpm = lossPpm,
        minMillis = successfulTimes.firstOrNull(),
        medianMillis = percentile(successfulTimes, 50),
        p95Millis = percentile(successfulTimes, 95),
        totalUsefulBytes = totalUsefulBytes,
        observedUsefulBitrateBps = usefulBps,
        longestFailureRun = longestFailureRun,
        stateTransitions = stateTransitions,
        intermittent = successes > 0 && failures > 0,
    )
}

private fun percentile(
    sorted: List<Long>,
    percentile: Int,
): Long? {
    if (sorted.isEmpty()) {
        return null
    }

    val rank = (
        (sorted.size * percentile + 99) / 100 - 1
    ).coerceIn(0, sorted.lastIndex)

    return sorted[rank]
}
