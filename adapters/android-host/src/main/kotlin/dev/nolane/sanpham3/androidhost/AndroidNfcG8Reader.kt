package dev.nolane.sanpham3.androidhost

import android.app.Activity
import android.nfc.NfcAdapter
import android.nfc.NfcManager
import android.nfc.Tag
import android.nfc.tech.IsoDep
import java.io.Closeable
import java.security.SecureRandom
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidNfcG8Evidence(
    val authenticatedPeerNodeId: Long,
    val challenge: ByteArray,
    val maxTransceiveLength: Int,
)

sealed interface AndroidNfcReaderEvent {
    data object Started : AndroidNfcReaderEvent

    data class Passed(
        val evidence: AndroidNfcG8Evidence,
    ) : AndroidNfcReaderEvent

    data class Failed(
        val detail: String,
    ) : AndroidNfcReaderEvent
}

class AndroidNfcG8Reader(
    private val activity: Activity,
    private val nodeId: Long,
    peerKey: ByteArray,
) : Closeable, NfcAdapter.ReaderCallback {
    private val key = peerKey.copyOf()
    private val active = AtomicBoolean(false)
    private var listener: ((AndroidNfcReaderEvent) -> Unit)? = null

    init {
        require(nodeId >= 0)
        require(peerKey.size == 32)
    }

    private val adapter: NfcAdapter?
        get() = activity
            .getSystemService(NfcManager::class.java)
            ?.defaultAdapter

    fun start(
        onEvent: (AndroidNfcReaderEvent) -> Unit,
    ) {
        if (!active.compareAndSet(false, true)) return
        listener = onEvent

        val nfc = adapter
        if (nfc == null || !nfc.isEnabled) {
            active.set(false)
            onEvent(
                AndroidNfcReaderEvent.Failed(
                    "NFC adapter unavailable or disabled",
                ),
            )
            return
        }

        nfc.enableReaderMode(
            activity,
            this,
            NfcAdapter.FLAG_READER_NFC_A or
                NfcAdapter.FLAG_READER_NFC_B or
                NfcAdapter.FLAG_READER_SKIP_NDEF_CHECK or
                NfcAdapter.FLAG_READER_NO_PLATFORM_SOUNDS,
            null,
        )
        onEvent(AndroidNfcReaderEvent.Started)
    }

    override fun onTagDiscovered(tag: Tag) {
        if (!active.get()) return

        val callback = listener ?: return
        try {
            val evidence = runCourt(tag)
            callback(AndroidNfcReaderEvent.Passed(evidence))
            close()
        } catch (error: Throwable) {
            callback(
                AndroidNfcReaderEvent.Failed(
                    error.message ?: error.javaClass.simpleName,
                ),
            )
        }
    }

    private fun runCourt(tag: Tag): AndroidNfcG8Evidence {
        val isoDep = requireNotNull(IsoDep.get(tag)) {
            "NFC tag does not expose ISO-DEP"
        }

        var pendingHandle: Long? = null
        var sessionHandle: Long? = null

        try {
            isoDep.connect()
            isoDep.timeout = 5_000

            AndroidNfcApdu.parseSuccessResponse(
                isoDep.transceive(AndroidNfcApdu.selectAidCommand()),
            )

            val keyCopy = key.copyOf()
            val beginPackage = try {
                AndroidPeerSessionNative.clientBegin(nodeId, keyCopy)
            } finally {
                keyCopy.fill(0)
            }
            require(beginPackage.size > 8) {
                "invalid NFC client-begin package"
            }
            pendingHandle = AndroidPeerSession.decodeU64(
                beginPackage,
                0,
            )
            val clientHello = beginPackage.copyOfRange(
                8,
                beginPackage.size,
            )

            val serverHello = AndroidNfcApdu.parseSuccessResponse(
                isoDep.transceive(
                    AndroidNfcApdu.command(
                        AndroidNfcApdu.INS_HANDSHAKE,
                        clientHello,
                    ),
                ),
            )

            val finishPackage = AndroidPeerSessionNative.clientFinish(
                pendingHandle,
                serverHello,
            )
            require(finishPackage.size == 16) {
                "invalid NFC client-finish package"
            }
            pendingHandle = null
            sessionHandle = AndroidPeerSession.decodeU64(
                finishPackage,
                0,
            )
            val peerNodeId = AndroidPeerSession.decodeU64(
                finishPackage,
                8,
            )

            val challenge = AndroidG8PairCourt.newChallenge(
                SecureRandom(),
            )
            val frame = AndroidPeerSessionNative.seal(
                sessionHandle,
                AndroidG8PairCourt.KIND_CHALLENGE,
                challenge,
            )
            require(frame.size <= AndroidNfcApdu.MAX_SHORT_PAYLOAD) {
                "encrypted NFC challenge exceeds short APDU budget"
            }

            val responseFrame = AndroidNfcApdu.parseSuccessResponse(
                isoDep.transceive(
                    AndroidNfcApdu.command(
                        AndroidNfcApdu.INS_FRAME,
                        frame,
                    ),
                ),
            )
            val opened = AndroidPeerSessionNative.open(
                sessionHandle,
                responseFrame,
            )
            require(opened.isNotEmpty()) {
                "NFC encrypted ACK is empty"
            }
            require(
                (opened[0].toInt() and 0xff) ==
                    AndroidG8PairCourt.KIND_ACK,
            ) {
                "NFC response is not G8 ACK"
            }
            val replyChallenge = opened.copyOfRange(1, opened.size)
            AndroidG8PairCourt.validateChallenge(replyChallenge)
            require(replyChallenge.contentEquals(challenge)) {
                "NFC G8 ACK does not echo the original challenge"
            }

            return AndroidNfcG8Evidence(
                authenticatedPeerNodeId = peerNodeId,
                challenge = challenge,
                maxTransceiveLength = isoDep.maxTransceiveLength,
            )
        } finally {
            pendingHandle?.let {
                try {
                    AndroidPeerSessionNative.closeHandle(it)
                } catch (_: Throwable) {
                }
            }
            sessionHandle?.let {
                try {
                    AndroidPeerSessionNative.closeHandle(it)
                } catch (_: Throwable) {
                }
            }
            try {
                isoDep.close()
            } catch (_: Throwable) {
            }
        }
    }

    override fun close() {
        if (!active.compareAndSet(true, false)) return
        try {
            adapter?.disableReaderMode(activity)
        } finally {
            listener = null
        }
    }

    fun destroy() {
        close()
        key.fill(0)
    }
}