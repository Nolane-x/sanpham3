package dev.nolane.sanpham3.androidhost

import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.content.Context
import java.io.Closeable
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidBleGattEvidence(
    val authenticatedPeerNodeId: Long,
    val challenge: ByteArray,
    val mtu: Int,
)

sealed interface AndroidBleGattServerEvent {
    data object Started : AndroidBleGattServerEvent

    data class PeerAuthenticated(
        val peerNodeId: Long,
    ) : AndroidBleGattServerEvent

    data class PairPassed(
        val evidence: AndroidBleGattEvidence,
    ) : AndroidBleGattServerEvent

    data class Failed(
        val detail: String,
    ) : AndroidBleGattServerEvent
}

internal object AndroidBleGattAdvertisement {
    private val marker = byteArrayOf(
        'S'.code.toByte(),
        'P'.code.toByte(),
        '3'.code.toByte(),
        'G'.code.toByte(),
        0x00,
    )

    fun encode(): ByteArray = marker.copyOf()

    fun matches(bytes: ByteArray): Boolean =
        bytes.contentEquals(marker)
}

class AndroidBleGattServer(
    context: Context,
) : Closeable {
    private data class SessionState(
        val nativeHandle: Long,
        val peerNodeId: Long,
    )

    private val appContext = context.applicationContext
    private val bluetoothManager =
        appContext.getSystemService(BluetoothManager::class.java)
    private val started = AtomicBoolean(false)
    private val sessions = ConcurrentHashMap<BluetoothDevice, SessionState>()
    private val pendingResponses = ConcurrentHashMap<BluetoothDevice, ByteArray>()
    private val mtuByDevice = ConcurrentHashMap<BluetoothDevice, Int>()

    private var gattServer: BluetoothGattServer? = null
    private var nodeId: Long = 0
    private var peerKey: ByteArray? = null
    private var listener: ((AndroidBleGattServerEvent) -> Unit)? = null

    private val callback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(
            device: BluetoothDevice,
            status: Int,
            newState: Int,
        ) {
            if (newState == BluetoothProfile.STATE_DISCONNECTED) {
                closeSession(device)
                pendingResponses.remove(device)
                mtuByDevice.remove(device)
            }
        }

        override fun onMtuChanged(
            device: BluetoothDevice,
            mtu: Int,
        ) {
            mtuByDevice[device] = mtu
        }

        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice,
            requestId: Int,
            characteristic: BluetoothGattCharacteristic,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray,
        ) {
            if (characteristic.uuid != AndroidBleGattProtocol.COMMAND_UUID) {
                respondWrite(
                    device,
                    requestId,
                    responseNeeded,
                    BluetoothGatt.GATT_REQUEST_NOT_SUPPORTED,
                )
                return
            }
            if (preparedWrite || offset != 0) {
                respondWrite(
                    device,
                    requestId,
                    responseNeeded,
                    BluetoothGatt.GATT_INVALID_OFFSET,
                )
                return
            }

            val status = try {
                handleCommand(device, value)
                BluetoothGatt.GATT_SUCCESS
            } catch (error: Throwable) {
                pendingResponses.remove(device)
                listener?.invoke(
                    AndroidBleGattServerEvent.Failed(
                        error.message ?: error.javaClass.simpleName,
                    ),
                )
                BluetoothGatt.GATT_FAILURE
            }

            respondWrite(device, requestId, responseNeeded, status)
        }

        override fun onCharacteristicReadRequest(
            device: BluetoothDevice,
            requestId: Int,
            offset: Int,
            characteristic: BluetoothGattCharacteristic,
        ) {
            if (characteristic.uuid != AndroidBleGattProtocol.RESPONSE_UUID) {
                gattServer?.sendResponse(
                    device,
                    requestId,
                    BluetoothGatt.GATT_REQUEST_NOT_SUPPORTED,
                    offset,
                    null,
                )
                return
            }

            val response = pendingResponses[device]
            if (response == null || offset !in 0..response.size) {
                gattServer?.sendResponse(
                    device,
                    requestId,
                    BluetoothGatt.GATT_FAILURE,
                    offset,
                    null,
                )
                return
            }

            val slice = response.copyOfRange(offset, response.size)
            val sent = gattServer?.sendResponse(
                device,
                requestId,
                BluetoothGatt.GATT_SUCCESS,
                offset,
                slice,
            ) == true

            if (sent && offset == 0) {
                pendingResponses.remove(device, response)
            }
        }
    }

    fun start(
        nodeId: Long,
        peerKey: ByteArray,
        onEvent: (AndroidBleGattServerEvent) -> Unit = {},
    ) {
        require(nodeId >= 0)
        require(peerKey.size == 32)
        if (!started.compareAndSet(false, true)) return

        this.nodeId = nodeId
        this.peerKey = peerKey.copyOf()
        this.listener = onEvent

        try {
            val server = requireNotNull(
                bluetoothManager?.openGattServer(appContext, callback),
            ) {
                "unable to open Android BLE GATT server"
            }
            gattServer = server

            val service = BluetoothGattService(
                AndroidBleGattProtocol.SERVICE_UUID,
                BluetoothGattService.SERVICE_TYPE_PRIMARY,
            )
            val command = BluetoothGattCharacteristic(
                AndroidBleGattProtocol.COMMAND_UUID,
                BluetoothGattCharacteristic.PROPERTY_WRITE,
                BluetoothGattCharacteristic.PERMISSION_WRITE,
            )
            val response = BluetoothGattCharacteristic(
                AndroidBleGattProtocol.RESPONSE_UUID,
                BluetoothGattCharacteristic.PROPERTY_READ,
                BluetoothGattCharacteristic.PERMISSION_READ,
            )
            require(service.addCharacteristic(command))
            require(service.addCharacteristic(response))
            require(server.addService(service)) {
                "unable to register sanpham3 GATT service"
            }

            onEvent(AndroidBleGattServerEvent.Started)
        } catch (error: Throwable) {
            close()
            throw error
        }
    }

    fun discoveryInfo(): ByteArray =
        AndroidBleGattAdvertisement.encode()

    private fun handleCommand(
        device: BluetoothDevice,
        bytes: ByteArray,
    ) {
        val mtu = mtuByDevice[device] ?: 23
        AndroidBleGattProtocol.requireFitsMtu(bytes, mtu)
        val envelope = AndroidBleGattProtocol.decode(bytes)

        when (envelope.opcode) {
            AndroidBleGattProtocol.OPCODE_HANDSHAKE ->
                handleHandshake(device, envelope.payload)

            AndroidBleGattProtocol.OPCODE_FRAME ->
                handleFrame(device, envelope.payload, mtu)

            else -> error("unsupported BLE GATT opcode ${envelope.opcode}")
        }
    }

    private fun handleHandshake(
        device: BluetoothDevice,
        clientHello: ByteArray,
    ) {
        closeSession(device)
        val key = requireNotNull(peerKey) {
            "BLE GATT server is not armed with a peer key"
        }.copyOf()

        val acceptPackage = try {
            AndroidPeerSessionNative.serverAccept(
                nodeId,
                key,
                clientHello,
            )
        } finally {
            key.fill(0)
        }
        require(
            acceptPackage.size >=
                16 + AndroidPeerSessionNative.handshakeLength(),
        )

        val nativeHandle = AndroidPeerSession.decodeU64(
            acceptPackage,
            0,
        )
        val peerNodeId = AndroidPeerSession.decodeU64(
            acceptPackage,
            8,
        )
        sessions[device] = SessionState(nativeHandle, peerNodeId)

        val serverHello = acceptPackage.copyOfRange(
            16,
            acceptPackage.size,
        )
        pendingResponses[device] = AndroidBleGattProtocol.encode(
            AndroidBleGattProtocol.OPCODE_HANDSHAKE,
            serverHello,
        )
        listener?.invoke(
            AndroidBleGattServerEvent.PeerAuthenticated(peerNodeId),
        )
    }

    private fun handleFrame(
        device: BluetoothDevice,
        frame: ByteArray,
        mtu: Int,
    ) {
        val session = requireNotNull(sessions[device]) {
            "BLE GATT frame arrived before authenticated handshake"
        }
        val opened = AndroidPeerSessionNative.open(
            session.nativeHandle,
            frame,
        )
        require(opened.isNotEmpty())

        val kind = opened[0].toInt() and 0xff
        val challenge = opened.copyOfRange(1, opened.size)
        require(kind == AndroidG8PairCourt.KIND_CHALLENGE)
        AndroidG8PairCourt.validateChallenge(challenge)

        val replyFrame = AndroidPeerSessionNative.seal(
            session.nativeHandle,
            AndroidG8PairCourt.KIND_ACK,
            challenge,
        )
        val response = AndroidBleGattProtocol.encode(
            AndroidBleGattProtocol.OPCODE_FRAME,
            replyFrame,
        )
        AndroidBleGattProtocol.requireFitsMtu(response, mtu)
        pendingResponses[device] = response

        listener?.invoke(
            AndroidBleGattServerEvent.PairPassed(
                AndroidBleGattEvidence(
                    authenticatedPeerNodeId = session.peerNodeId,
                    challenge = challenge.copyOf(),
                    mtu = mtu,
                ),
            ),
        )
    }

    private fun respondWrite(
        device: BluetoothDevice,
        requestId: Int,
        responseNeeded: Boolean,
        status: Int,
    ) {
        if (!responseNeeded) return
        gattServer?.sendResponse(
            device,
            requestId,
            status,
            0,
            null,
        )
    }

    private fun closeSession(device: BluetoothDevice) {
        val session = sessions.remove(device) ?: return
        try {
            AndroidPeerSessionNative.closeHandle(session.nativeHandle)
        } catch (_: Throwable) {
        }
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) return

        sessions.keys.toList().forEach(::closeSession)
        pendingResponses.clear()
        mtuByDevice.clear()
        peerKey?.fill(0)
        peerKey = null
        listener = null

        try {
            gattServer?.clearServices()
        } finally {
            gattServer?.close()
            gattServer = null
        }
    }
}