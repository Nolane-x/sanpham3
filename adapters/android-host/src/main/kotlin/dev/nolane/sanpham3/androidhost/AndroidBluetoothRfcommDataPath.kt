package dev.nolane.sanpham3.androidhost

import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothServerSocket
import android.bluetooth.BluetoothSocket
import android.content.Context
import java.io.Closeable
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

object AndroidBluetoothRfcommProtocol {
    val SERVICE_UUID: UUID = UUID.fromString(
        "bdbd3af8-7b52-4e1c-96fb-0fd6b4ba1a72",
    )
    const val SERVICE_NAME: String = "sanpham3-recovery-rfcomm"
}

class AndroidBluetoothRfcommServerDataPath(
    context: Context,
) : Closeable {
    private val bluetoothManager =
        context.applicationContext.getSystemService(BluetoothManager::class.java)
    private val started = AtomicBoolean(false)
    private var serverSocket: BluetoothServerSocket? = null

    fun start() {
        if (!started.compareAndSet(false, true)) return
        val adapter = requireNotNull(bluetoothManager?.adapter) {
            "Bluetooth adapter unavailable"
        }
        require(adapter.isEnabled) {
            "Bluetooth adapter disabled"
        }

        try {
            serverSocket = adapter.listenUsingInsecureRfcommWithServiceRecord(
                AndroidBluetoothRfcommProtocol.SERVICE_NAME,
                AndroidBluetoothRfcommProtocol.SERVICE_UUID,
            )
        } catch (error: Throwable) {
            started.set(false)
            serverSocket = null
            throw error
        }
    }

    fun accept(timeoutMillis: Int): BluetoothSocket {
        require(timeoutMillis > 0)
        val socket = checkNotNull(serverSocket) {
            "RFCOMM server has not been started"
        }
        return socket.accept(timeoutMillis)
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) return
        try {
            serverSocket?.close()
        } finally {
            serverSocket = null
        }
    }
}

class AndroidBluetoothRfcommClientDataPath(
    context: Context,
) {
    private val bluetoothManager =
        context.applicationContext.getSystemService(BluetoothManager::class.java)

    /** Blocking connect. Run on a worker thread. */
    fun connect(
        peer: AndroidBluetoothClassicPeer,
    ): BluetoothSocket {
        val adapter = requireNotNull(bluetoothManager?.adapter) {
            "Bluetooth adapter unavailable"
        }
        require(adapter.isEnabled) {
            "Bluetooth adapter disabled"
        }

        try {
            adapter.cancelDiscovery()
        } catch (_: SecurityException) {
        }

        val socket = peer.device
            .createInsecureRfcommSocketToServiceRecord(
                AndroidBluetoothRfcommProtocol.SERVICE_UUID,
            )

        try {
            socket.connect()
            return socket
        } catch (error: Throwable) {
            try {
                socket.close()
            } catch (_: Throwable) {
            }
            throw error
        }
    }
}