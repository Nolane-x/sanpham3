package dev.nolane.sanpham3.androidhost

import java.nio.ByteBuffer
import java.nio.ByteOrder

enum class AndroidRecoveryTrafficClass {
    CRITICAL,
    TINY_SEMANTIC,
    INTERACTIVE,
    BULK,
}

enum class AndroidRecoveryDeliveryMode {
    FULL,
    COMPACT,
    SEMANTIC,
    TINY_SEMANTIC,
    EMERGENCY,
}

enum class AndroidRecoveryPathKind {
    LOCAL_EGRESS,
    DIRECT_INTERNET,
    PEER_EGRESS,
}

enum class AndroidRecoveryReason {
    BEST_LIVE_PATH,
    NO_LIVE_EGRESS,
    METERED_PATH_DISALLOWED,
    TRAFFIC_TOO_HEAVY_FOR_PATH,
    LIVE_WAIT_BUDGET_EXCEEDED,
}

sealed interface AndroidRecoverySelectedPath {
    data class DirectNetwork(
        val networkHandle: Long,
    ) : AndroidRecoverySelectedPath

    data class PeerEgress(
        val peerNodeId: Long,
    ) : AndroidRecoverySelectedPath
}

data class AndroidPeerEgressCandidate(
    val peerNodeId: Long,
    val transport: AndroidTransport,
    val state: AndroidMeasuredLinkState,
    val estimatedBitrateBps: Long,
    val lossPpm: Int,
    val rttMillis: Long,
    val metered: Boolean = false,
) {
    init {
        require(peerNodeId >= 0)
        require(estimatedBitrateBps >= 0)
        require(lossPpm in 0..1_000_000)
        require(rttMillis >= 0)
    }
}

data class AndroidRecoveryTaskSpec(
    val trafficClass: AndroidRecoveryTrafficClass,
    val estimatedWireBytes: Long,
    val estimatedRoundTrips: Int = 2,
    val allowMetered: Boolean = true,
    val allowDelayTolerant: Boolean = true,
    val maxLiveWaitMillis: Long? = null,
) {
    init {
        require(estimatedWireBytes >= 0)
        require(estimatedRoundTrips in 1..65_535)
        require(maxLiveWaitMillis == null || maxLiveWaitMillis >= 0)
    }
}

sealed interface AndroidRecoveryDecision

data class AndroidLiveRecoveryDecision(
    val mode: AndroidRecoveryDeliveryMode,
    val pathKind: AndroidRecoveryPathKind,
    val selectedPath: AndroidRecoverySelectedPath?,
    val effectiveBitrateBps: Long,
    val expectedCompletionMillis: Long,
    val worstLossPpm: Int,
    val totalRttMillis: Long,
    val intermittentHops: Int,
    val meteredHops: Int,
    val experimental: Boolean,
) : AndroidRecoveryDecision

data class AndroidDelayTolerantDecision(
    val reason: AndroidRecoveryReason,
) : AndroidRecoveryDecision

data class AndroidLocalOnlyDecision(
    val reason: AndroidRecoveryReason,
) : AndroidRecoveryDecision

object AndroidRecoveryPlanner {
    fun plan(
        snapshot: AndroidRecoverySnapshot,
        peerEgress: List<AndroidPeerEgressCandidate>,
        task: AndroidRecoveryTaskSpec,
    ): AndroidRecoveryDecision {
        val paths = AndroidRecoveryPlannerCodec.paths(
            snapshot.measuredInternetPaths(),
            peerEgress,
        )
        val request = AndroidRecoveryPlannerCodec.encodeRequest(
            paths,
            task,
        )
        val response = AndroidRecoveryPlannerNative.plan(request)
        return AndroidRecoveryPlannerCodec.decodeResponse(
            response,
            paths,
        )
    }
}

internal data class AndroidPlannerPath(
    val kind: Int,
    val externalId: Long,
    val transport: AndroidTransport,
    val state: AndroidMeasuredLinkState,
    val bitrateBps: Long,
    val lossPpm: Int,
    val rttMillis: Long,
    val metered: Boolean,
    val selected: AndroidRecoverySelectedPath,
)

internal object AndroidRecoveryPlannerCodec {
    private val requestMagic = byteArrayOf(
        'S'.code.toByte(),
        'P'.code.toByte(),
        '3'.code.toByte(),
        'P'.code.toByte(),
    )
    private val responseMagic = byteArrayOf(
        'S'.code.toByte(),
        'P'.code.toByte(),
        '3'.code.toByte(),
        'R'.code.toByte(),
    )
    private const val version = 0
    private const val requestHeaderLength = 27
    private const val pathLength = 32
    private const val responseLength = 52
    private const val maxPaths = 256
    private const val noPathIndex = 0xffff

    fun paths(
        direct: List<AndroidMeasuredInternetPath>,
        peers: List<AndroidPeerEgressCandidate>,
    ): List<AndroidPlannerPath> {
        val result = ArrayList<AndroidPlannerPath>(
            direct.size + peers.size,
        )

        for (path in direct) {
            require(path.networkHandle >= 0)
            result += AndroidPlannerPath(
                kind = 0,
                externalId = path.networkHandle,
                transport = path.transport,
                state = path.state,
                bitrateBps = path.observedUsefulBitrateBps,
                lossPpm = path.lossPpm,
                rttMillis = path.latencyMillis,
                metered = path.metered,
                selected = AndroidRecoverySelectedPath.DirectNetwork(
                    path.networkHandle,
                ),
            )
        }

        for (peer in peers) {
            result += AndroidPlannerPath(
                kind = 1,
                externalId = peer.peerNodeId,
                transport = peer.transport,
                state = peer.state,
                bitrateBps = peer.estimatedBitrateBps.coerceAtLeast(1),
                lossPpm = peer.lossPpm,
                rttMillis = peer.rttMillis,
                metered = peer.metered,
                selected = AndroidRecoverySelectedPath.PeerEgress(
                    peer.peerNodeId,
                ),
            )
        }

        require(result.size <= maxPaths) {
            "Android recovery planner supports at most $maxPaths paths"
        }
        return result
    }

    fun encodeRequest(
        paths: List<AndroidPlannerPath>,
        task: AndroidRecoveryTaskSpec,
    ): ByteArray {
        require(paths.size <= maxPaths)
        val size = requestHeaderLength + pathLength * paths.size
        val buffer = ByteBuffer.allocate(size).order(ByteOrder.BIG_ENDIAN)

        buffer.put(requestMagic)
        buffer.put(version.toByte())
        buffer.putShort(paths.size.toShort())
        buffer.put(task.trafficClass.ordinal.toByte())

        var flags = 0
        if (task.allowMetered) flags = flags or 0b0000_0001
        if (task.allowDelayTolerant) flags = flags or 0b0000_0010
        buffer.put(flags.toByte())
        buffer.putShort(task.estimatedRoundTrips.toShort())
        buffer.putLong(task.estimatedWireBytes)
        buffer.putLong(task.maxLiveWaitMillis ?: -1L)

        for (path in paths) {
            require(path.externalId >= 0)
            require(path.bitrateBps >= 0)
            require(path.lossPpm in 0..1_000_000)
            require(path.rttMillis >= 0)

            buffer.put(path.kind.toByte())
            buffer.put(transportCode(path.transport).toByte())
            buffer.put(stateCode(path.state).toByte())
            buffer.put((if (path.metered) 1 else 0).toByte())
            buffer.putLong(path.externalId)
            buffer.putLong(path.bitrateBps.coerceAtLeast(1))
            buffer.putInt(path.lossPpm)
            buffer.putLong(path.rttMillis)
        }

        check(buffer.position() == size)
        return buffer.array()
    }

    fun decodeResponse(
        bytes: ByteArray,
        paths: List<AndroidPlannerPath>,
    ): AndroidRecoveryDecision {
        require(bytes.size == responseLength) {
            "invalid Android planner response length ${bytes.size}"
        }

        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.BIG_ENDIAN)
        val magic = ByteArray(4)
        buffer.get(magic)
        require(magic.contentEquals(responseMagic)) {
            "invalid Android planner response magic"
        }
        require(unsigned(buffer.get()) == version) {
            "unsupported Android planner response version"
        }

        val resultKind = unsigned(buffer.get())
        val modeCode = unsigned(buffer.get())
        val pathKindCode = unsigned(buffer.get())
        val reasonCode = unsigned(buffer.get())
        val experimentalCode = unsigned(buffer.get())
        require(experimentalCode in 0..1)

        val selectedIndex = unsigned(buffer.short)
        val externalId = buffer.long
        val effectiveBps = buffer.long
        val expectedMillis = buffer.long
        val lossPpm = buffer.int
        val totalRttMillis = buffer.long
        val intermittentHops = unsigned(buffer.short)
        val meteredHops = unsigned(buffer.short)

        require(!buffer.hasRemaining())
        require(externalId >= 0)
        require(effectiveBps >= 0)
        require(expectedMillis >= 0)
        require(lossPpm in 0..1_000_000)
        require(totalRttMillis >= 0)

        return when (resultKind) {
            0 -> {
                require(reasonCode == 0)
                val mode = decodeMode(modeCode)
                val pathKind = decodePathKind(pathKindCode)
                val selected = if (selectedIndex == noPathIndex) {
                    require(pathKind == AndroidRecoveryPathKind.LOCAL_EGRESS)
                    require(externalId == 0L)
                    null
                } else {
                    require(selectedIndex < paths.size)
                    val path = paths[selectedIndex]
                    require(path.externalId == externalId)
                    when (pathKind) {
                        AndroidRecoveryPathKind.DIRECT_INTERNET ->
                            require(path.kind == 0)
                        AndroidRecoveryPathKind.PEER_EGRESS ->
                            require(path.kind == 1)
                        AndroidRecoveryPathKind.LOCAL_EGRESS ->
                            error("local egress cannot select an external path")
                    }
                    path.selected
                }

                AndroidLiveRecoveryDecision(
                    mode = mode,
                    pathKind = pathKind,
                    selectedPath = selected,
                    effectiveBitrateBps = effectiveBps,
                    expectedCompletionMillis = expectedMillis,
                    worstLossPpm = lossPpm,
                    totalRttMillis = totalRttMillis,
                    intermittentHops = intermittentHops,
                    meteredHops = meteredHops,
                    experimental = experimentalCode == 1,
                )
            }
            1 -> AndroidDelayTolerantDecision(
                decodeReason(reasonCode),
            )
            2 -> AndroidLocalOnlyDecision(
                decodeReason(reasonCode),
            )
            else -> error("invalid Android planner result kind $resultKind")
        }
    }

    private fun transportCode(transport: AndroidTransport): Int =
        when (transport) {
            AndroidTransport.WIFI -> 0
            AndroidTransport.CELLULAR -> 1
            AndroidTransport.ETHERNET -> 2
            AndroidTransport.VPN -> 3
            AndroidTransport.BLUETOOTH -> 4
            AndroidTransport.WIFI_AWARE -> 5
            AndroidTransport.SATELLITE -> 6
            AndroidTransport.USB -> 7
            AndroidTransport.OTHER -> 8
        }

    private fun stateCode(state: AndroidMeasuredLinkState): Int =
        when (state) {
            AndroidMeasuredLinkState.UP -> 0
            AndroidMeasuredLinkState.INTERMITTENT -> 1
        }

    private fun decodeMode(value: Int): AndroidRecoveryDeliveryMode =
        when (value) {
            0 -> AndroidRecoveryDeliveryMode.FULL
            1 -> AndroidRecoveryDeliveryMode.COMPACT
            2 -> AndroidRecoveryDeliveryMode.SEMANTIC
            3 -> AndroidRecoveryDeliveryMode.TINY_SEMANTIC
            4 -> AndroidRecoveryDeliveryMode.EMERGENCY
            else -> error("invalid Android planner delivery mode $value")
        }

    private fun decodePathKind(value: Int): AndroidRecoveryPathKind =
        when (value) {
            0 -> AndroidRecoveryPathKind.LOCAL_EGRESS
            1 -> AndroidRecoveryPathKind.DIRECT_INTERNET
            2 -> AndroidRecoveryPathKind.PEER_EGRESS
            else -> error("invalid Android planner path kind $value")
        }

    private fun decodeReason(value: Int): AndroidRecoveryReason =
        when (value) {
            0 -> AndroidRecoveryReason.BEST_LIVE_PATH
            1 -> AndroidRecoveryReason.NO_LIVE_EGRESS
            2 -> AndroidRecoveryReason.METERED_PATH_DISALLOWED
            3 -> AndroidRecoveryReason.TRAFFIC_TOO_HEAVY_FOR_PATH
            4 -> AndroidRecoveryReason.LIVE_WAIT_BUDGET_EXCEEDED
            else -> error("invalid Android planner reason $value")
        }

    private fun unsigned(value: Byte): Int = value.toInt() and 0xff
    private fun unsigned(value: Short): Int = value.toInt() and 0xffff
}

internal object AndroidRecoveryPlannerNative {
    init {
        System.loadLibrary("sp3_android_peer_session")
    }

    external fun plan(request: ByteArray): ByteArray
}