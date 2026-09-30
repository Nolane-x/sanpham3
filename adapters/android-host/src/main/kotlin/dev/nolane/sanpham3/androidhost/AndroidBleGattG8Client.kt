package dev.nolane.sanpham3.androidhost

import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCallback
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.BluetoothStatusCodes
import android.content.Context
import android.os.Build
import java.util.UUID
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit

class AndroidBleGattG8Client(
    context: Context,
) {
    private sealed interface Event {
        data class Connection(
            val status: Int,
            val newState: Int,
        ) : Event

        data class Mtu(
            val mtu: Int,
            val status: Int,
        ) : Event

        data class Services(
            val status: Int,
        ) : Event

        data class Write(
            val uuid: UUID,
            val status: Int,
        ) : Event

        data class Read(
            val uuid: UUID,
            val value: ByteArray,
            val status: Int,
        ) : Event
    }

    private val appContext = context.applicationContext
    private val bluetoothManager =
        appContext.getSystemService(BluetoothManager::class.java)

    /**
     * Blocking G8 court. Call from a worker thread, never the main thread.
     */
    fun run(
        peer: AndroidBlePeer,
        nodeId: Long,
        peerKey: ByteArray,
        timeoutMillis: Long = 15_000,
    ): AndroidBleGattEvidence {
        require(nodeId >= 0)
        require(peerKey.size == 32)
        require(timeoutMillis > 0)
        require(AndroidBleGattAdvertisement.matches(peer.serviceData)) {
            "BLE peer is not advertising the sanpham3 GATT fallback marker"
        }

        val adapter = requireNotNull(bluetoothManager?.adapter) {
            "Bluetooth adapter unavailable"
        }
        require(adapter.isEnabled) {
            "Bluetooth adapter disabled"
        }
        val device = peer.device ?: run {
            require(peer.addressHint.isNotBlank()) {
                "BLE peer has neither scan device nor address hint"
            }
            adapter.getRemoteDevice(peer.addressHint)
        }

        val events = LinkedBlockingQueue<Event>()
        val callback = callback(events)
        var pendingHandle: Long? = null
        var sessionHandle: Long? = null
        var gatt: BluetoothGatt? = null

        try {
            val deadline = System.nanoTime() +
                TimeUnit.MILLISECONDS.toNanos(timeoutMillis)

            gatt = device.connectGatt(
                appContext,
                false,
                callback,
                android.bluetooth.BluetoothDevice.TRANSPORT_LE,
            )

            val connection = await<Event.Connection>(events, deadline)
            require(
                connection.status == BluetoothGatt.GATT_SUCCESS &&
                    connection.newState == BluetoothProfile.STATE_CONNECTED,
            ) {
                "BLE GATT connection failed status=${connection.status} state=${connection.newState}"
            }

            events.clear()
            require(gatt.requestMtu(AndroidBleGattProtocol.TARGET_MTU)) {
                "BLE GATT MTU request was not started"
            }
            val mtuEvent = await<Event.Mtu>(events, deadline)
            require(mtuEvent.status == BluetoothGatt.GATT_SUCCESS) {
                "BLE GATT MTU negotiation failed status=${mtuEvent.status}"
            }
            require(mtuEvent.mtu >= AndroidBleGattProtocol.MIN_REQUIRED_MTU) {
                "BLE GATT negotiated MTU ${mtuEvent.mtu} below prototype minimum ${AndroidBleGattProtocol.MIN_REQUIRED_MTU}"
            }
            val mtu = mtuEvent.mtu

            events.clear()
            require(gatt.discoverServices()) {
                "BLE GATT service discovery did not start"
            }
            val services = await<Event.Services>(events, deadline)
            require(services.status == BluetoothGatt.GATT_SUCCESS) {
                "BLE GATT service discovery failed status=${services.status}"
            }

            val service = requireNotNull(
                gatt.getService(AndroidBleGattProtocol.SERVICE_UUID),
            ) {
                "peer does not expose sanpham3 GATT service"
            }
            val command = requireNotNull(
                service.getCharacteristic(AndroidBleGattProtocol.COMMAND_UUID),
            ) {
                "peer GATT command characteristic missing"
            }
            val response = requireNotNull(
                service.getCharacteristic(AndroidBleGattProtocol.RESPONSE_UUID),
            ) {
                "peer GATT response characteristic missing"
            }

            val keyCopy = peerKey.copyOf()
            val beginPackage = try {
                AndroidPeerSessionNative.clientBegin(nodeId, keyCopy)
            } finally {
                keyCopy.fill(0)
            }
            require(beginPackage.size > 8)
            pendingHandle = AndroidPeerSession.decodeU64(beginPackage, 0)
            val clientHello = beginPackage.copyOfRange(8, beginPackage.size)

            val handshakeResponse = exchange(
                gatt,
                command,
                response,
                mtu,
                AndroidBleGattProtocol.OPCODE_HANDSHAKE,
                clientHello,
                events,
                deadline,
            )
            require(
                handshakeResponse.opcode ==
                    AndroidBleGattProtocol.OPCODE_HANDSHAKE,
            )

            val finishPackage = AndroidPeerSessionNative.clientFinish(
                pendingHandle,
                handshakeResponse.payload,
            )
            require(finishPackage.size == 16)
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
                java.security.SecureRandom(),
            )
            val frame = AndroidPeerSessionNative.seal(
                sessionHandle,
                AndroidG8PairCourt.KIND_CHALLENGE,
                challenge,
            )
            val frameResponse = exchange(
                gatt,
                command,
                response,
                mtu,
                AndroidBleGattProtocol.OPCODE_FRAME,
                frame,
                events,
                deadline,
            )
            require(
                frameResponse.opcode == AndroidBleGattProtocol.OPCODE_FRAME,
            )

            val opened = AndroidPeerSessionNative.open(
                sessionHandle,
                frameResponse.payload,
            )
            require(opened.isNotEmpty())
            require(
                (opened[0].toInt() and 0xff) ==
                    AndroidG8PairCourt.KIND_ACK,
            )
            val replyChallenge = opened.copyOfRange(1, opened.size)
            AndroidG8PairCourt.validateChallenge(replyChallenge)
            require(replyChallenge.contentEquals(challenge)) {
                "BLE GATT G8 ACK does not match challenge"
            }

            val maxBenchmarkPayload =
                AndroidBleGattProtocol.maxEncryptedProjectPlaintextBytes(mtu)
            require(maxBenchmarkPayload >= 16) {
                "BLE GATT MTU leaves less than 16 benchmark plaintext bytes"
            }
            val benchmarkConfig = AndroidPeerBenchmarkConfig(
                rounds = 32,
                payloadBytes = minOf(64, maxBenchmarkPayload),
            )
            val rtts = LongArray(benchmarkConfig.rounds)
            val benchmarkStarted = System.nanoTime()

            repeat(benchmarkConfig.rounds) { index ->
                val benchmarkPayload =
                    AndroidPeerSessionBenchmark.benchmarkPayload(
                        sequence = index.toLong(),
                        payloadBytes = benchmarkConfig.payloadBytes,
                    )
                val benchmarkFrame =
                    AndroidPeerSessionNative.seal(
                        sessionHandle,
                        AndroidPeerSessionBenchmark.KIND_PROBE,
                        benchmarkPayload,
                    )

                val roundStarted = System.nanoTime()
                val benchmarkResponse = exchange(
                    gatt,
                    command,
                    response,
                    mtu,
                    AndroidBleGattProtocol.OPCODE_FRAME,
                    benchmarkFrame,
                    events,
                    System.nanoTime() +
                        TimeUnit.MILLISECONDS.toNanos(timeoutMillis),
                )
                val roundFinished = System.nanoTime()
                require(
                    benchmarkResponse.opcode ==
                        AndroidBleGattProtocol.OPCODE_FRAME,
                )

                val benchmarkOpened = AndroidPeerSessionNative.open(
                    sessionHandle,
                    benchmarkResponse.payload,
                )
                require(benchmarkOpened.isNotEmpty())
                require(
                    (benchmarkOpened[0].toInt() and 0xff) ==
                        AndroidPeerSessionBenchmark.KIND_ACK,
                ) {
                    "BLE GATT benchmark response kind mismatch"
                }
                val replyPayload =
                    benchmarkOpened.copyOfRange(1, benchmarkOpened.size)
                require(replyPayload.contentEquals(benchmarkPayload)) {
                    "BLE GATT benchmark ACK payload mismatch at round $index"
                }

                rtts[index] =
                    (roundFinished - roundStarted).coerceAtLeast(1L)
            }

            val benchmark = AndroidPeerSessionBenchmark.benchmarkEvidence(
                peerNodeId = peerNodeId,
                config = benchmarkConfig,
                elapsedNanos =
                    (System.nanoTime() - benchmarkStarted)
                        .coerceAtLeast(1L),
                rtts = rtts,
            )

            return AndroidBleGattEvidence(
                authenticatedPeerNodeId = peerNodeId,
                challenge = challenge,
                mtu = mtu,
                benchmarkRounds = benchmark.rounds,
                benchmarkPayloadBytes = benchmark.payloadBytes,
                benchmarkElapsedNanos = benchmark.elapsedNanos,
                benchmarkMinRttNanos = benchmark.minRttNanos,
                benchmarkMedianRttNanos = benchmark.medianRttNanos,
                benchmarkP95RttNanos = benchmark.p95RttNanos,
                benchmarkMaxRttNanos = benchmark.maxRttNanos,
                benchmarkOneWayUsefulBitsPerSecond =
                    benchmark.oneWayUsefulBitsPerSecond,
                benchmarkRoundTripUsefulBitsPerSecond =
                    benchmark.roundTripUsefulBitsPerSecond,
            )
        } finally {
            pendingHandle?.let(::closeNativeHandle)
            sessionHandle?.let(::closeNativeHandle)
            try {
                gatt?.disconnect()
            } catch (_: Throwable) {
            }
            try {
                gatt?.close()
            } catch (_: Throwable) {
            }
        }
    }

    private fun exchange(
        gatt: BluetoothGatt,
        command: BluetoothGattCharacteristic,
        response: BluetoothGattCharacteristic,
        mtu: Int,
        opcode: Int,
        payload: ByteArray,
        events: LinkedBlockingQueue<Event>,
        deadlineNanos: Long,
    ): AndroidBleGattProtocol.Envelope {
        val envelope = AndroidBleGattProtocol.encode(opcode, payload)
        AndroidBleGattProtocol.requireFitsMtu(envelope, mtu)

        events.clear()
        require(startWrite(gatt, command, envelope)) {
            "BLE GATT characteristic write did not start"
        }
        val write = await<Event.Write>(events, deadlineNanos) {
            it.uuid == AndroidBleGattProtocol.COMMAND_UUID
        }
        require(write.status == BluetoothGatt.GATT_SUCCESS) {
            "BLE GATT command write failed status=${write.status}"
        }

        events.clear()
        require(gatt.readCharacteristic(response)) {
            "BLE GATT response read did not start"
        }
        val read = await<Event.Read>(events, deadlineNanos) {
            it.uuid == AndroidBleGattProtocol.RESPONSE_UUID
        }
        require(read.status == BluetoothGatt.GATT_SUCCESS) {
            "BLE GATT response read failed status=${read.status}"
        }
        AndroidBleGattProtocol.requireFitsMtu(read.value, mtu)
        return AndroidBleGattProtocol.decode(read.value)
    }

    private fun startWrite(
        gatt: BluetoothGatt,
        characteristic: BluetoothGattCharacteristic,
        value: ByteArray,
    ): Boolean {
        return if (Build.VERSION.SDK_INT >= 33) {
            gatt.writeCharacteristic(
                characteristic,
                value,
                BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT,
            ) == BluetoothStatusCodes.SUCCESS
        } else {
            @Suppress("DEPRECATION")
            characteristic.value = value
            characteristic.writeType =
                BluetoothGattCharacteristic.WRITE_TYPE_DEFAULT
            @Suppress("DEPRECATION")
            gatt.writeCharacteristic(characteristic)
        }
    }

    private inline fun <reified T : Event> await(
        events: LinkedBlockingQueue<Event>,
        deadlineNanos: Long,
        predicate: (T) -> Boolean = { true },
    ): T {
        while (true) {
            val remaining = deadlineNanos - System.nanoTime()
            require(remaining > 0) {
                "timed out waiting for BLE GATT ${T::class.java.simpleName}"
            }
            val event = events.poll(
                remaining,
                TimeUnit.NANOSECONDS,
            ) ?: error(
                "timed out waiting for BLE GATT ${T::class.java.simpleName}",
            )
            if (event is T && predicate(event)) return event
        }
    }

    private fun callback(
        events: LinkedBlockingQueue<Event>,
    ): BluetoothGattCallback = object : BluetoothGattCallback() {
        override fun onConnectionStateChange(
            gatt: BluetoothGatt,
            status: Int,
            newState: Int,
        ) {
            events.offer(Event.Connection(status, newState))
        }

        override fun onMtuChanged(
            gatt: BluetoothGatt,
            mtu: Int,
            status: Int,
        ) {
            events.offer(Event.Mtu(mtu, status))
        }

        override fun onServicesDiscovered(
            gatt: BluetoothGatt,
            status: Int,
        ) {
            events.offer(Event.Services(status))
        }

        override fun onCharacteristicWrite(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            events.offer(Event.Write(characteristic.uuid, status))
        }

        override fun onCharacteristicRead(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            value: ByteArray,
            status: Int,
        ) {
            events.offer(
                Event.Read(
                    characteristic.uuid,
                    value.copyOf(),
                    status,
                ),
            )
        }

        @Suppress("DEPRECATION")
        override fun onCharacteristicRead(
            gatt: BluetoothGatt,
            characteristic: BluetoothGattCharacteristic,
            status: Int,
        ) {
            if (Build.VERSION.SDK_INT >= 33) return
            events.offer(
                Event.Read(
                    characteristic.uuid,
                    characteristic.value?.copyOf() ?: ByteArray(0),
                    status,
                ),
            )
        }
    }

    private fun closeNativeHandle(handle: Long) {
        try {
            AndroidPeerSessionNative.closeHandle(handle)
        } catch (_: Throwable) {
        }
    }
}