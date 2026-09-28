package dev.nolane.sanpham3.androidhost

import java.nio.ByteBuffer
import java.nio.ByteOrder
import org.junit.Assert.assertEquals
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidRecoveryPlannerCodecTest {
    private fun direct(
        id: Long,
        bitrate: Long,
    ) = AndroidMeasuredInternetPath(
        pathId = "android:$id",
        networkHandle = id,
        transport = AndroidTransport.WIFI,
        state = AndroidMeasuredLinkState.UP,
        observedUsefulBitrateBps = bitrate,
        lossPpm = 0,
        latencyMillis = 20,
        metered = false,
        defaultNetwork = true,
        interfaceName = "wlan0",
    )

    private fun response(
        kind: Int,
        mode: Int = 0xff,
        pathKind: Int = 0xff,
        reason: Int = 0,
        experimental: Boolean = false,
        selectedIndex: Int = 0xffff,
        externalId: Long = 0,
        effectiveBps: Long = 0,
        expectedMillis: Long = 0,
        lossPpm: Int = 0,
        totalRttMillis: Long = 0,
        intermittentHops: Int = 0,
        meteredHops: Int = 0,
    ): ByteArray {
        val buffer = ByteBuffer.allocate(52).order(ByteOrder.BIG_ENDIAN)
        buffer.put(byteArrayOf(
            'S'.code.toByte(),
            'P'.code.toByte(),
            '3'.code.toByte(),
            'R'.code.toByte(),
        ))
        buffer.put(0)
        buffer.put(kind.toByte())
        buffer.put(mode.toByte())
        buffer.put(pathKind.toByte())
        buffer.put(reason.toByte())
        buffer.put(if (experimental) 1 else 0)
        buffer.putShort(selectedIndex.toShort())
        buffer.putLong(externalId)
        buffer.putLong(effectiveBps)
        buffer.putLong(expectedMillis)
        buffer.putInt(lossPpm)
        buffer.putLong(totalRttMillis)
        buffer.putShort(intermittentHops.toShort())
        buffer.putShort(meteredHops.toShort())
        return buffer.array()
    }

    @Test
    fun requestCodecUsesStableBigEndianLayout() {
        val paths = AndroidRecoveryPlannerCodec.paths(
            direct = listOf(direct(7, 120)),
            peers = listOf(
                AndroidPeerEgressCandidate(
                    peerNodeId = 300,
                    transport = AndroidTransport.WIFI,
                    state = AndroidMeasuredLinkState.INTERMITTENT,
                    estimatedBitrateBps = 100,
                    lossPpm = 100_000,
                    rttMillis = 180,
                ),
            ),
        )
        val task = AndroidRecoveryTaskSpec(
            trafficClass = AndroidRecoveryTrafficClass.TINY_SEMANTIC,
            estimatedWireBytes = 232,
            estimatedRoundTrips = 2,
            allowMetered = true,
            allowDelayTolerant = true,
        )

        val bytes = AndroidRecoveryPlannerCodec.encodeRequest(paths, task)
        assertEquals(27 + 32 * 2, bytes.size)

        val buffer = ByteBuffer.wrap(bytes).order(ByteOrder.BIG_ENDIAN)
        val magic = ByteArray(4)
        buffer.get(magic)
        assertTrue(magic.contentEquals("SP3P".encodeToByteArray()))
        assertEquals(0, buffer.get().toInt())
        assertEquals(2, buffer.short.toInt())
        assertEquals(1, buffer.get().toInt())
        assertEquals(3, buffer.get().toInt())
        assertEquals(2, buffer.short.toInt())
        assertEquals(232L, buffer.long)
        assertEquals(-1L, buffer.long)

        assertEquals(0, buffer.get().toInt())
        assertEquals(0, buffer.get().toInt())
        assertEquals(0, buffer.get().toInt())
        assertEquals(0, buffer.get().toInt())
        assertEquals(7L, buffer.long)
        assertEquals(120L, buffer.long)
        assertEquals(0, buffer.int)
        assertEquals(20L, buffer.long)

        assertEquals(1, buffer.get().toInt())
        assertEquals(0, buffer.get().toInt())
        assertEquals(1, buffer.get().toInt())
        assertEquals(0, buffer.get().toInt())
        assertEquals(300L, buffer.long)
        assertEquals(100L, buffer.long)
        assertEquals(100_000, buffer.int)
        assertEquals(180L, buffer.long)
        assertTrue(!buffer.hasRemaining())
    }

    @Test
    fun liveResponseMapsSelectedPeerByIndexAndAuthenticatedNodeId() {
        val paths = AndroidRecoveryPlannerCodec.paths(
            direct = listOf(direct(7, 120)),
            peers = listOf(
                AndroidPeerEgressCandidate(
                    peerNodeId = 300,
                    transport = AndroidTransport.WIFI,
                    state = AndroidMeasuredLinkState.UP,
                    estimatedBitrateBps = 100,
                    lossPpm = 100_000,
                    rttMillis = 180,
                ),
            ),
        )

        val decision = AndroidRecoveryPlannerCodec.decodeResponse(
            response(
                kind = 0,
                mode = 3,
                pathKind = 2,
                selectedIndex = 1,
                externalId = 300,
                effectiveBps = 90,
                expectedMillis = 21_000,
                lossPpm = 100_000,
                totalRttMillis = 180,
            ),
            paths,
        )

        val live = decision as AndroidLiveRecoveryDecision
        assertEquals(AndroidRecoveryDeliveryMode.TINY_SEMANTIC, live.mode)
        assertEquals(AndroidRecoveryPathKind.PEER_EGRESS, live.pathKind)
        assertEquals(
            AndroidRecoverySelectedPath.PeerEgress(300),
            live.selectedPath,
        )
        assertEquals(90L, live.effectiveBitrateBps)
    }

    @Test
    fun dtnResponseMapsPolicyReason() {
        val decision = AndroidRecoveryPlannerCodec.decodeResponse(
            response(kind = 1, reason = 2),
            emptyList(),
        )

        assertEquals(
            AndroidDelayTolerantDecision(
                AndroidRecoveryReason.METERED_PATH_DISALLOWED,
            ),
            decision,
        )
    }

}