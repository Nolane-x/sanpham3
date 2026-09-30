package dev.nolane.sanpham3.androidhost

import java.util.concurrent.atomic.AtomicReference

internal data class AndroidNfcHceConfig(
    val nodeId: Long,
    val peerKey: ByteArray,
)

data class AndroidNfcHceEvidence(
    val authenticatedPeerNodeId: Long,
    val lastChallenge: ByteArray?,
    val benchmarkFramesCompleted: Int,
)

object AndroidNfcHceCourt {
    private val config = AtomicReference<AndroidNfcHceConfig?>(null)
    private val evidence = AtomicReference<AndroidNfcHceEvidence?>(null)

    fun configure(
        nodeId: Long,
        peerKey: ByteArray,
    ) {
        require(nodeId >= 0)
        require(peerKey.size == 32)

        val previous = config.getAndSet(
            AndroidNfcHceConfig(
                nodeId = nodeId,
                peerKey = peerKey.copyOf(),
            ),
        )
        previous?.peerKey?.fill(0)
        evidence.set(null)
    }

    fun clear() {
        config.getAndSet(null)?.peerKey?.fill(0)
        evidence.set(null)
    }

    fun evidence(): AndroidNfcHceEvidence? =
        evidence.get()?.let {
            AndroidNfcHceEvidence(
                authenticatedPeerNodeId = it.authenticatedPeerNodeId,
                lastChallenge = it.lastChallenge?.copyOf(),
                benchmarkFramesCompleted = it.benchmarkFramesCompleted,
            )
        }

    internal fun snapshotConfig(): AndroidNfcHceConfig? =
        config.get()?.let {
            AndroidNfcHceConfig(
                nodeId = it.nodeId,
                peerKey = it.peerKey.copyOf(),
            )
        }

    internal fun recordEvidence(
        peerNodeId: Long,
        challenge: ByteArray,
    ) {
        evidence.set(
            AndroidNfcHceEvidence(
                authenticatedPeerNodeId = peerNodeId,
                lastChallenge = challenge.copyOf(),
                benchmarkFramesCompleted = 0,
            ),
        )
    }

    internal fun recordBenchmarkFrame() {
        evidence.updateAndGet { current ->
            current?.copy(
                benchmarkFramesCompleted =
                    current.benchmarkFramesCompleted + 1,
            )
        }
    }


}