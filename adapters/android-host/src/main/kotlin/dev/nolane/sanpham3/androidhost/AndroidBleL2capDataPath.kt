package dev.nolane.sanpham3.androidhost

import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import android.content.Context
import android.os.Build
import java.io.Closeable
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidBleL2capEndpoint(
    val psm: Int,
)

sealed interface AndroidBleL2capEvent {
    data class Listening(
        val endpoint: AndroidBleL2capEndpoint,
        val discoveryInfo: ByteArray,
    ) : AndroidBleL2capEvent

    data class Failed(
        val detail: String,
    ) : AndroidBleL2capEvent
}

/**
 * Tiny-stream fallback over Bluetooth LE L2CAP CoC.
 *
 * V0 intentionally uses the "insecure" L2CAP link variant so Recovery Mode
 * does not require OS-level bonding before peers can exchange project frames.
 * This does NOT make the project session unauthenticated: every accepted
 * BluetoothSocket must be wrapped by the existing peer-session protocol,
 * which performs its own authentication, replay protection and encryption.
 *
 * Requires Android 10 / API 29+.
 */
class AndroidBleL2capServerDataPath(
    context: Context,
) : Closeable {
    private val bluetoothManager =
        context.applicationContext.getSystemService(BluetoothManager::class.java)

    private val started = AtomicBoolean(false)
    private var serverSocket: BluetoothServerSocket? = null

    fun start(
        onEvent: (AndroidBleL2capEvent) -> Unit = {},
    ): AndroidBleL2capEndpoint {
        requireL2capSupported()

        if (!started.compareAndSet(false, true)) {
            val existing = checkNotNull(serverSocket) {
                "BLE L2CAP server is marked started without a socket"
            }
            return AndroidBleL2capEndpoint(existing.psm)
        }

        val adapter = bluetoothManager?.adapter
        requireNotNull(adapter) {
            "Bluetooth adapter unavailable"
        }
        require(adapter.isEnabled) {
            "Bluetooth adapter disabled"
        }

        try {
            val server = adapter.listenUsingInsecureL2capChannel()
            serverSocket = server

            val endpoint = AndroidBleL2capEndpoint(server.psm)
            val discoveryInfo =
                AndroidBleL2capAdvertisement.encode(endpoint.psm)

            onEvent(
                AndroidBleL2capEvent.Listening(
                    endpoint = endpoint,
                    discoveryInfo = discoveryInfo,
                ),
            )
            return endpoint
        } catch (error: Exception) {
            started.set(false)
            serverSocket = null
            onEvent(
                AndroidBleL2capEvent.Failed(
                    error.message ?: error.javaClass.simpleName,
                ),
            )
            throw error
        }
    }

    fun discoveryInfo(): ByteArray {
        val socket = checkNotNull(serverSocket) {
            "BLE L2CAP server has not been started"
        }
        return AndroidBleL2capAdvertisement.encode(socket.psm)
    }

    fun accept(timeoutMillis: Int): BluetoothSocket {
        require(timeoutMillis > 0)
        val socket = checkNotNull(serverSocket) {
            "BLE L2CAP server has not been started"
        }
        return socket.accept(timeoutMillis)
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) {
            return
        }

        try {
            serverSocket?.close()
        } finally {
            serverSocket = null
        }
    }
}

class AndroidBleL2capClientDataPath(
    context: Context,
) {
    private val bluetoothManager =
        context.applicationContext.getSystemService(BluetoothManager::class.java)

    /**
     * Blocking connect. Call from a worker thread/coroutine dispatcher.
     */
    fun connect(peer: AndroidBlePeer): BluetoothSocket {
        requireL2capSupported()

        val endpoint = AndroidBleL2capAdvertisement.decode(peer.serviceData)
        val adapter = bluetoothManager?.adapter
        requireNotNull(adapter) {
            "Bluetooth adapter unavailable"
        }
        require(adapter.isEnabled) {
            "Bluetooth adapter disabled"
        }
        val device = peer.device ?: run {
            require(peer.addressHint.isNotBlank()) {
                "BLE peer has neither a scan device nor an address hint"
            }
            adapter.getRemoteDevice(peer.addressHint)
        }
        val socket = device.createInsecureL2capChannel(endpoint.psm)

        try {
            socket.connect()
            return socket
        } catch (error: Exception) {
            socket.close()
            throw error
        }
    }
}

internal object AndroidBleL2capAdvertisement {
    private val MAGIC = byteArrayOf(
        'S'.code.toByte(),
        'P'.code.toByte(),
        '3'.code.toByte(),
        'L'.code.toByte(),
    )
    private const val VERSION: Int = 0
    private const val ENCODED_LEN: Int = 7

    fun encode(psm: Int): ByteArray {
        require(psm in 1..0xffff) {
            "BLE L2CAP PSM must fit in an unsigned 16-bit field"
        }

        return byteArrayOf(
            MAGIC[0],
            MAGIC[1],
            MAGIC[2],
            MAGIC[3],
            VERSION.toByte(),
            ((psm ushr 8) and 0xff).toByte(),
            (psm and 0xff).toByte(),
        )
    }

    fun decode(bytes: ByteArray): AndroidBleL2capEndpoint {
        require(bytes.size == ENCODED_LEN) {
            "BLE L2CAP discovery payload has wrong length"
        }
        require(bytes.copyOfRange(0, 4).contentEquals(MAGIC)) {
            "BLE L2CAP discovery payload has wrong magic"
        }
        require((bytes[4].toInt() and 0xff) == VERSION) {
            "BLE L2CAP discovery payload has unsupported version"
        }

        val psm =
            ((bytes[5].toInt() and 0xff) shl 8) or
                (bytes[6].toInt() and 0xff)
        require(psm != 0) {
            "BLE L2CAP discovery payload contains PSM 0"
        }

        return AndroidBleL2capEndpoint(psm)
    }
}

private fun requireL2capSupported() {
    require(Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
        "Bluetooth LE L2CAP CoC requires Android 10+"
    }
}
