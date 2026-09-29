package dev.nolane.sanpham3.androidhost

import android.nfc.cardemulation.HostApduService
import android.os.Bundle

class AndroidNfcHostApduService : HostApduService() {
    private var nativeHandle: Long? = null
    private var authenticatedPeerNodeId: Long? = null

    override fun processCommandApdu(
        commandApdu: ByteArray,
        extras: Bundle?,
    ): ByteArray {
        if (AndroidNfcApdu.isSelectAid(commandApdu)) {
            return AndroidNfcApdu.success()
        }

        val command = AndroidNfcApdu.parseCommand(commandApdu)
            ?: return AndroidNfcApdu.badData()

        return try {
            when (command.instruction) {
                AndroidNfcApdu.INS_HANDSHAKE ->
                    handleHandshake(command.payload)

                AndroidNfcApdu.INS_FRAME ->
                    handleEncryptedFrame(command.payload)

                else -> AndroidNfcApdu.unsupported()
            }
        } catch (_: Throwable) {
            closeNativeSession()
            AndroidNfcApdu.badData()
        }
    }

    override fun onDeactivated(reason: Int) {
        closeNativeSession()
    }

    override fun onDestroy() {
        closeNativeSession()
        super.onDestroy()
    }

    private fun handleHandshake(clientHello: ByteArray): ByteArray {
        closeNativeSession()

        val config = AndroidNfcHceCourt.snapshotConfig()
            ?: return AndroidNfcApdu.badData()
        val keyCopy = config.peerKey

        return try {
            val acceptPackage = AndroidPeerSessionNative.serverAccept(
                config.nodeId,
                keyCopy,
                clientHello,
            )
            require(
                acceptPackage.size >=
                    16 + AndroidPeerSessionNative.handshakeLength(),
            ) {
                "invalid NFC server-accept package"
            }

            nativeHandle = AndroidPeerSession.decodeU64(
                acceptPackage,
                0,
            )
            authenticatedPeerNodeId = AndroidPeerSession.decodeU64(
                acceptPackage,
                8,
            )
            val serverHello = acceptPackage.copyOfRange(
                16,
                acceptPackage.size,
            )

            AndroidNfcApdu.success(serverHello)
        } finally {
            keyCopy.fill(0)
        }
    }

    private fun handleEncryptedFrame(frame: ByteArray): ByteArray {
        val handle = nativeHandle ?: return AndroidNfcApdu.badData()
        val peerNodeId = authenticatedPeerNodeId
            ?: return AndroidNfcApdu.badData()

        val opened = AndroidPeerSessionNative.open(handle, frame)
        require(opened.isNotEmpty()) {
            "decrypted NFC frame is missing kind byte"
        }

        val kind = opened[0].toInt() and 0xff
        val payload = opened.copyOfRange(1, opened.size)
        require(kind == AndroidG8PairCourt.KIND_CHALLENGE) {
            "unexpected NFC G8 message kind $kind"
        }
        AndroidG8PairCourt.validateChallenge(payload)

        AndroidNfcHceCourt.recordEvidence(
            peerNodeId = peerNodeId,
            challenge = payload,
        )

        val reply = AndroidPeerSessionNative.seal(
            handle,
            AndroidG8PairCourt.KIND_ACK,
            payload,
        )
        require(reply.size <= AndroidNfcApdu.MAX_SHORT_PAYLOAD) {
            "encrypted NFC reply exceeds short APDU budget"
        }
        return AndroidNfcApdu.success(reply)
    }

    private fun closeNativeSession() {
        val handle = nativeHandle
        nativeHandle = null
        authenticatedPeerNodeId = null
        if (handle != null) {
            try {
                AndroidPeerSessionNative.closeHandle(handle)
            } catch (_: Throwable) {
                // Service teardown must remain best-effort.
            }
        }
    }
}