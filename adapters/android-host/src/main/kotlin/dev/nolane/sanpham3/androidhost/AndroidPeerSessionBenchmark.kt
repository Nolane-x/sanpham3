package dev.nolane.sanpham3.androidhost

import java.nio.ByteBuffer
import java.nio.ByteOrder

data class AndroidPeerBenchmarkConfig(
    val rounds: Int = 32,
    val payloadBytes: Int = 1024,
) {
    init {
        require(rounds in 1..10_000) {
            "benchmark rounds must be in 1..10000"
        }
        require(payloadBytes in 16..1_048_576) {
            "benchmark payloadBytes must be in 16..1048576"
        }
    }
}

data class AndroidPeerBenchmarkEvidence(
    val peerNodeId: Long,
    val rounds: Int,
    val payloadBytes: Int,
    val oneWayUsefulBytes: Long,
    val roundTripUsefulBytes: Long,
    val elapsedNanos: Long,
    val minRttNanos: Long,
    val medianRttNanos: Long,
    val p95RttNanos: Long,
    val maxRttNanos: Long,
) {
    val oneWayUsefulBitsPerSecond: Double
        get() = bitsPerSecond(oneWayUsefulBytes, elapsedNanos)

    val roundTripUsefulBitsPerSecond: Double
        get() = bitsPerSecond(roundTripUsefulBytes, elapsedNanos)
}

object AndroidPeerSessionBenchmark {
    const val KIND_PROBE: Int = 0x60
    const val KIND_ACK: Int = 0x61

    private const val sequenceBytes = 8

    fun runClient(
        session: AndroidPeerSession,
        config: AndroidPeerBenchmarkConfig = AndroidPeerBenchmarkConfig(),
    ): AndroidPeerBenchmarkEvidence {
        val rtts = LongArray(config.rounds)
        val started = System.nanoTime()

        repeat(config.rounds) { index ->
            val payload = benchmarkPayload(
                sequence = index.toLong(),
                payloadBytes = config.payloadBytes,
            )
            val roundStarted = System.nanoTime()
            session.send(KIND_PROBE, payload)

            val response = session.receive()
            val roundFinished = System.nanoTime()
            require(response.kind == KIND_ACK) {
                "unexpected benchmark ACK kind ${response.kind}"
            }
            require(response.payload.contentEquals(payload)) {
                "benchmark ACK payload mismatch at round $index"
            }

            rtts[index] = positiveDuration(
                roundStarted,
                roundFinished,
            )
        }

        val finished = System.nanoTime()
        return benchmarkEvidence(
            peerNodeId = session.peerNodeId,
            config = config,
            elapsedNanos = positiveDuration(started, finished),
            rtts = rtts,
        )
    }

    fun serve(
        session: AndroidPeerSession,
        config: AndroidPeerBenchmarkConfig = AndroidPeerBenchmarkConfig(),
    ) {
        repeat(config.rounds) { expectedRound ->
            val request = session.receive()
            require(request.kind == KIND_PROBE) {
                "unexpected benchmark probe kind ${request.kind}"
            }
            require(request.payload.size == config.payloadBytes) {
                "benchmark probe size ${request.payload.size} != ${config.payloadBytes}"
            }
            require(
                benchmarkSequence(request.payload) ==
                    expectedRound.toLong(),
            ) {
                "benchmark probe sequence mismatch at round $expectedRound"
            }

            session.send(KIND_ACK, request.payload)
        }
    }

    internal fun benchmarkPayload(
        sequence: Long,
        payloadBytes: Int,
    ): ByteArray {
        require(sequence >= 0) {
            "benchmark sequence must be non-negative"
        }
        require(payloadBytes >= sequenceBytes) {
            "benchmark payload must fit sequence field"
        }

        val payload = ByteArray(payloadBytes)
        ByteBuffer
            .wrap(payload, 0, sequenceBytes)
            .order(ByteOrder.BIG_ENDIAN)
            .putLong(sequence)

        for (index in sequenceBytes until payload.size) {
            payload[index] =
                ((sequence + index.toLong() * 31L) and 0xffL)
                    .toByte()
        }
        return payload
    }

    internal fun benchmarkSequence(payload: ByteArray): Long {
        require(payload.size >= sequenceBytes) {
            "benchmark payload is shorter than sequence field"
        }
        val sequence = ByteBuffer
            .wrap(payload, 0, sequenceBytes)
            .order(ByteOrder.BIG_ENDIAN)
            .long
        require(sequence >= 0) {
            "benchmark sequence must be non-negative"
        }
        return sequence
    }

    internal fun benchmarkEvidence(
        peerNodeId: Long,
        config: AndroidPeerBenchmarkConfig,
        elapsedNanos: Long,
        rtts: LongArray,
    ): AndroidPeerBenchmarkEvidence {
        require(peerNodeId >= 0) {
            "peerNodeId must be non-negative"
        }
        require(elapsedNanos > 0) {
            "benchmark elapsedNanos must be positive"
        }
        require(rtts.size == config.rounds) {
            "RTT sample count must equal configured rounds"
        }
        require(rtts.all { it > 0 }) {
            "all RTT samples must be positive"
        }

        val sorted = rtts.sortedArray()
        val oneWayUsefulBytes =
            Math.multiplyExact(
                config.rounds.toLong(),
                config.payloadBytes.toLong(),
            )
        val roundTripUsefulBytes =
            Math.multiplyExact(oneWayUsefulBytes, 2L)

        return AndroidPeerBenchmarkEvidence(
            peerNodeId = peerNodeId,
            rounds = config.rounds,
            payloadBytes = config.payloadBytes,
            oneWayUsefulBytes = oneWayUsefulBytes,
            roundTripUsefulBytes = roundTripUsefulBytes,
            elapsedNanos = elapsedNanos,
            minRttNanos = sorted.first(),
            medianRttNanos = percentileNearestRank(sorted, 0.50),
            p95RttNanos = percentileNearestRank(sorted, 0.95),
            maxRttNanos = sorted.last(),
        )
    }

    private fun percentileNearestRank(
        sorted: LongArray,
        fraction: Double,
    ): Long {
        require(sorted.isNotEmpty())
        require(fraction > 0.0 && fraction <= 1.0)

        val rank =
            kotlin.math.ceil(fraction * sorted.size.toDouble())
                .toInt()
                .coerceIn(1, sorted.size)
        return sorted[rank - 1]
    }

    private fun positiveDuration(
        startNanos: Long,
        endNanos: Long,
    ): Long =
        (endNanos - startNanos).coerceAtLeast(1L)
}

private fun bitsPerSecond(
    bytes: Long,
    elapsedNanos: Long,
): Double {
    require(bytes >= 0)
    require(elapsedNanos > 0)
    return bytes.toDouble() * 8.0 * 1_000_000_000.0 /
        elapsedNanos.toDouble()
}
