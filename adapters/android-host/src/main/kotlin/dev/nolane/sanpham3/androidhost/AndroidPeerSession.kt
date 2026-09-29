package dev.nolane.sanpham3.androidhost

import java.io.Closeable
import java.io.EOFException
import java.io.InputStream
import java.io.OutputStream
import java.net.Socket
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidPeerMessage(
    val kind: Int,
    val payload: ByteArray,
)

/**
 * Authenticated/encrypted project session over an already-established duplex
 * byte stream.
 *
 * java.net.Socket is the common case, but the session intentionally owns only
 * InputStream / OutputStream + Closeable semantics. That lets Android-specific
 * carriers such as Bluetooth LE L2CAP CoC reuse the exact same Rust
 * peer-session implementation instead of inventing a second trust layer.
 */
class AndroidPeerSession private constructor(
    private val input: InputStream,
    private val output: OutputStream,
    private val transport: Closeable,
    private val nativeHandle: Long,
    val peerNodeId: Long,
    private val frameHeaderLength: Int,
) : Closeable {
    private val closed = AtomicBoolean(false)

    companion object {
        private const val HANDLE_BYTES = 8
        private const val CLIENT_FINISH_BYTES = 16
        private const val KEY_BYTES = 32

        fun client(
            socket: Socket,
            nodeId: Long,
            peerKey: ByteArray,
        ): AndroidPeerSession =
            client(
                input = socket.getInputStream(),
                output = socket.getOutputStream(),
                transport = socket,
                nodeId = nodeId,
                peerKey = peerKey,
            )

        fun server(
            socket: Socket,
            nodeId: Long,
            peerKey: ByteArray,
        ): AndroidPeerSession =
            server(
                input = socket.getInputStream(),
                output = socket.getOutputStream(),
                transport = socket,
                nodeId = nodeId,
                peerKey = peerKey,
            )

        internal fun client(
            input: InputStream,
            output: OutputStream,
            transport: Closeable,
            nodeId: Long,
            peerKey: ByteArray,
        ): AndroidPeerSession {
            validateNodeId(nodeId)
            validateKey(peerKey)

            var pendingHandle: Long? = null
            var sessionHandle: Long? = null

            try {
                val keyCopy = peerKey.copyOf()
                val beginPackage = try {
                    AndroidPeerSessionNative.clientBegin(nodeId, keyCopy)
                } finally {
                    keyCopy.fill(0)
                }

                require(beginPackage.size > HANDLE_BYTES) {
                    "invalid client-begin package"
                }

                pendingHandle = decodeU64(beginPackage, 0)
                val clientHello = beginPackage.copyOfRange(
                    HANDLE_BYTES,
                    beginPackage.size,
                )

                val handshakeLength =
                    AndroidPeerSessionNative.handshakeLength()
                require(clientHello.size == handshakeLength) {
                    "unexpected client hello length"
                }

                output.apply {
                    write(clientHello)
                    flush()
                }

                val serverHello = input.readExactly(handshakeLength)
                val finishPackage = AndroidPeerSessionNative.clientFinish(
                    pendingHandle,
                    serverHello,
                )
                require(finishPackage.size == CLIENT_FINISH_BYTES) {
                    "invalid client-finish package"
                }

                sessionHandle = decodeU64(finishPackage, 0)
                val peerNodeId = decodeU64(finishPackage, HANDLE_BYTES)
                pendingHandle = null

                return AndroidPeerSession(
                    input = input,
                    output = output,
                    transport = transport,
                    nativeHandle = sessionHandle,
                    peerNodeId = peerNodeId,
                    frameHeaderLength =
                        AndroidPeerSessionNative.frameHeaderLength(),
                )
            } catch (error: Throwable) {
                pendingHandle?.let(AndroidPeerSessionNative::closeHandle)
                sessionHandle?.let(AndroidPeerSessionNative::closeHandle)
                closeQuietly(transport)
                throw error
            }
        }

        internal fun server(
            input: InputStream,
            output: OutputStream,
            transport: Closeable,
            nodeId: Long,
            peerKey: ByteArray,
        ): AndroidPeerSession {
            validateNodeId(nodeId)
            validateKey(peerKey)

            var sessionHandle: Long? = null

            try {
                val handshakeLength =
                    AndroidPeerSessionNative.handshakeLength()
                val clientHello = input.readExactly(handshakeLength)

                val keyCopy = peerKey.copyOf()
                val acceptPackage = try {
                    AndroidPeerSessionNative.serverAccept(
                        nodeId,
                        keyCopy,
                        clientHello,
                    )
                } finally {
                    keyCopy.fill(0)
                }

                require(
                    acceptPackage.size ==
                        HANDLE_BYTES * 2 + handshakeLength,
                ) {
                    "invalid server-accept package"
                }

                sessionHandle = decodeU64(acceptPackage, 0)
                val peerNodeId = decodeU64(
                    acceptPackage,
                    HANDLE_BYTES,
                )
                val serverHello = acceptPackage.copyOfRange(
                    HANDLE_BYTES * 2,
                    acceptPackage.size,
                )

                output.apply {
                    write(serverHello)
                    flush()
                }

                return AndroidPeerSession(
                    input = input,
                    output = output,
                    transport = transport,
                    nativeHandle = sessionHandle,
                    peerNodeId = peerNodeId,
                    frameHeaderLength =
                        AndroidPeerSessionNative.frameHeaderLength(),
                )
            } catch (error: Throwable) {
                sessionHandle?.let(AndroidPeerSessionNative::closeHandle)
                closeQuietly(transport)
                throw error
            }
        }

        internal fun decodeU64(bytes: ByteArray, offset: Int): Long {
            require(offset >= 0 && offset + 8 <= bytes.size) {
                "u64 field is outside byte array"
            }

            val value = ByteBuffer
                .wrap(bytes, offset, 8)
                .order(ByteOrder.BIG_ENDIAN)
                .long

            require(value >= 0) {
                "peer-session bridge returned unsupported u64 value"
            }
            return value
        }

        private fun validateNodeId(nodeId: Long) {
            require(nodeId >= 0) {
                "nodeId must be non-negative"
            }
        }

        private fun validateKey(peerKey: ByteArray) {
            require(peerKey.size == KEY_BYTES) {
                "peerKey must contain exactly 32 bytes"
            }
        }

        private fun closeQuietly(closeable: Closeable) {
            try {
                closeable.close()
            } catch (_: Exception) {
                // Preserve the original session/transport error.
            }
        }
    }

    @Synchronized
    fun send(kind: Int, payload: ByteArray) {
        check(!closed.get()) {
            "peer session is closed"
        }
        require(kind in 0..255) {
            "message kind must be in 0..255"
        }

        val frame = AndroidPeerSessionNative.seal(
            nativeHandle,
            kind,
            payload,
        )
        output.apply {
            write(frame)
            flush()
        }
    }

    @Synchronized
    fun receive(): AndroidPeerMessage {
        check(!closed.get()) {
            "peer session is closed"
        }

        val header = input.readExactly(frameHeaderLength)
        val ciphertextLength =
            AndroidPeerSessionNative.frameCiphertextLength(header)
        require(ciphertextLength > 0) {
            "invalid encrypted frame length"
        }

        val ciphertext = input.readExactly(ciphertextLength)
        val frame = ByteArray(header.size + ciphertext.size)
        header.copyInto(frame, 0)
        ciphertext.copyInto(frame, header.size)

        val opened = AndroidPeerSessionNative.open(
            nativeHandle,
            frame,
        )
        require(opened.isNotEmpty()) {
            "decrypted peer frame is missing kind byte"
        }

        return AndroidPeerMessage(
            kind = opened[0].toInt() and 0xff,
            payload = opened.copyOfRange(1, opened.size),
        )
    }

    override fun close() {
        if (!closed.compareAndSet(false, true)) {
            return
        }

        try {
            AndroidPeerSessionNative.closeHandle(nativeHandle)
        } finally {
            transport.close()
        }
    }
}

internal object AndroidPeerSessionNative {
    init {
        System.loadLibrary("sp3_android_peer_session")
    }

    external fun handshakeLength(): Int

    external fun frameHeaderLength(): Int

    external fun clientBegin(
        nodeId: Long,
        peerKey: ByteArray,
    ): ByteArray

    external fun serverAccept(
        nodeId: Long,
        peerKey: ByteArray,
        clientHello: ByteArray,
    ): ByteArray

    external fun clientFinish(
        pendingHandle: Long,
        serverHello: ByteArray,
    ): ByteArray

    external fun seal(
        handle: Long,
        kind: Int,
        plaintext: ByteArray,
    ): ByteArray

    external fun open(
        handle: Long,
        frame: ByteArray,
    ): ByteArray

    external fun frameCiphertextLength(header: ByteArray): Int

    external fun closeHandle(handle: Long)
}

internal fun InputStream.readExactly(length: Int): ByteArray {
    require(length >= 0) {
        "length must be non-negative"
    }

    val bytes = ByteArray(length)
    var offset = 0

    while (offset < length) {
        val count = read(bytes, offset, length - offset)
        if (count < 0) {
            throw EOFException(
                "stream ended after $offset of $length bytes",
            )
        }
        offset += count
    }

    return bytes
}
