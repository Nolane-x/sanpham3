package dev.nolane.sanpham3.androidhost

import java.security.SecureRandom

data class AndroidG8PairEvidence(
    val peerNodeId: Long,
    val challenge: ByteArray,
)

object AndroidG8PairCourt {
    const val KIND_CHALLENGE: Int = 0x50
    const val KIND_ACK: Int = 0x51

    private val magic = byteArrayOf(
        'G'.code.toByte(),
        '8'.code.toByte(),
        'P'.code.toByte(),
        '0'.code.toByte(),
    )
    private const val nonceLength = 32
    const val challengeLength: Int = 4 + nonceLength

    fun runClient(
        session: AndroidPeerSession,
        random: SecureRandom = SecureRandom(),
    ): AndroidG8PairEvidence {
        val challenge = newChallenge(random)
        session.send(KIND_CHALLENGE, challenge)

        val response = session.receive()
        require(response.kind == KIND_ACK) {
            "unexpected G8 ACK kind ${response.kind}"
        }
        validateChallenge(response.payload)
        require(response.payload.contentEquals(challenge)) {
            "G8 ACK challenge does not match request"
        }

        return AndroidG8PairEvidence(
            peerNodeId = session.peerNodeId,
            challenge = challenge,
        )
    }

    fun serveOnce(
        session: AndroidPeerSession,
    ): AndroidG8PairEvidence {
        val request = session.receive()
        require(request.kind == KIND_CHALLENGE) {
            "unexpected G8 challenge kind ${request.kind}"
        }
        validateChallenge(request.payload)

        session.send(KIND_ACK, request.payload)

        return AndroidG8PairEvidence(
            peerNodeId = session.peerNodeId,
            challenge = request.payload.copyOf(),
        )
    }

    internal fun newChallenge(
        random: SecureRandom,
    ): ByteArray {
        val bytes = ByteArray(challengeLength)
        magic.copyInto(bytes, 0)

        val nonce = ByteArray(nonceLength)
        random.nextBytes(nonce)
        nonce.copyInto(bytes, magic.size)
        nonce.fill(0)

        return bytes
    }

    internal fun validateChallenge(
        bytes: ByteArray,
    ) {
        require(bytes.size == challengeLength) {
            "invalid G8 challenge length ${bytes.size}"
        }

        for (index in magic.indices) {
            require(bytes[index] == magic[index]) {
                "invalid G8 challenge magic"
            }
        }
    }
}