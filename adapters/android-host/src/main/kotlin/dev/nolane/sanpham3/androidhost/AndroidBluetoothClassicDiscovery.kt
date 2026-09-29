package dev.nolane.sanpham3.androidhost

import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Build
import java.io.Closeable
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidBluetoothClassicPeer(
    val nameHint: String?,
    val addressHint: String,
    val rssi: Int?,
    internal val device: BluetoothDevice,
)

sealed interface AndroidBluetoothClassicEvent {
    data object Started : AndroidBluetoothClassicEvent

    data class PeerDiscovered(
        val peer: AndroidBluetoothClassicPeer,
    ) : AndroidBluetoothClassicEvent

    data object Finished : AndroidBluetoothClassicEvent

    data class Failed(
        val detail: String,
    ) : AndroidBluetoothClassicEvent
}

class AndroidBluetoothClassicDiscovery(
    context: Context,
) : Closeable {
    companion object {
        fun requestDiscoverableIntent(
            durationSeconds: Int = 120,
        ): Intent {
            require(durationSeconds in 1..3600) {
                "discoverable duration must be in 1..3600 seconds"
            }
            return Intent(BluetoothAdapter.ACTION_REQUEST_DISCOVERABLE).apply {
                putExtra(
                    BluetoothAdapter.EXTRA_DISCOVERABLE_DURATION,
                    durationSeconds,
                )
            }
        }
    }
    private val appContext = context.applicationContext
    private val bluetoothManager =
        appContext.getSystemService(BluetoothManager::class.java)
    private val started = AtomicBoolean(false)
    private var listener: ((AndroidBluetoothClassicEvent) -> Unit)? = null

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context, intent: Intent) {
            when (intent.action) {
                BluetoothDevice.ACTION_FOUND -> {
                    val device = intent.bluetoothDevice() ?: return
                    val address = try {
                        device.address.orEmpty()
                    } catch (_: SecurityException) {
                        ""
                    }
                    val name = try {
                        device.name
                    } catch (_: SecurityException) {
                        null
                    }
                    val rssi = if (intent.hasExtra(BluetoothDevice.EXTRA_RSSI)) {
                        intent.getShortExtra(
                            BluetoothDevice.EXTRA_RSSI,
                            Short.MIN_VALUE,
                        ).toInt().takeUnless { it == Short.MIN_VALUE.toInt() }
                    } else {
                        null
                    }

                    listener?.invoke(
                        AndroidBluetoothClassicEvent.PeerDiscovered(
                            AndroidBluetoothClassicPeer(
                                nameHint = name,
                                addressHint = address,
                                rssi = rssi,
                                device = device,
                            ),
                        ),
                    )
                }

                BluetoothAdapter.ACTION_DISCOVERY_FINISHED ->
                    listener?.invoke(AndroidBluetoothClassicEvent.Finished)
            }
        }
    }

    fun bondedPeers(): List<AndroidBluetoothClassicPeer> {
        val adapter = requireNotNull(bluetoothManager?.adapter) {
            "Bluetooth adapter unavailable"
        }
        require(adapter.isEnabled) {
            "Bluetooth adapter disabled"
        }

        return adapter.bondedDevices
            .map { device ->
                val address = try {
                    device.address.orEmpty()
                } catch (_: SecurityException) {
                    ""
                }
                val name = try {
                    device.name
                } catch (_: SecurityException) {
                    null
                }

                AndroidBluetoothClassicPeer(
                    nameHint = name,
                    addressHint = address,
                    rssi = null,
                    device = device,
                )
            }
            .sortedBy { it.addressHint }
    }

    fun start(
        onEvent: (AndroidBluetoothClassicEvent) -> Unit,
    ) {
        if (!started.compareAndSet(false, true)) return
        listener = onEvent

        val adapter = bluetoothManager?.adapter
        if (adapter == null || !adapter.isEnabled) {
            failAndClose("Bluetooth adapter unavailable or disabled")
            return
        }

        val filter = IntentFilter().apply {
            addAction(BluetoothDevice.ACTION_FOUND)
            addAction(BluetoothAdapter.ACTION_DISCOVERY_FINISHED)
        }

        try {
            if (Build.VERSION.SDK_INT >= 33) {
                appContext.registerReceiver(
                    receiver,
                    filter,
                    Context.RECEIVER_EXPORTED,
                )
            } else {
                @Suppress("DEPRECATION")
                appContext.registerReceiver(receiver, filter)
            }

            adapter.cancelDiscovery()
            require(adapter.startDiscovery()) {
                "Bluetooth Classic discovery did not start"
            }
            onEvent(AndroidBluetoothClassicEvent.Started)
        } catch (error: Throwable) {
            failAndClose(
                error.message ?: error.javaClass.simpleName,
            )
        }
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) return

        try {
            bluetoothManager?.adapter?.cancelDiscovery()
        } catch (_: SecurityException) {
        }

        try {
            appContext.unregisterReceiver(receiver)
        } catch (_: IllegalArgumentException) {
        }
        listener = null
    }

    private fun failAndClose(detail: String) {
        val callback = listener
        close()
        callback?.invoke(
            AndroidBluetoothClassicEvent.Failed(detail),
        )
    }
}

private fun Intent.bluetoothDevice(): BluetoothDevice? =
    if (Build.VERSION.SDK_INT >= 33) {
        getParcelableExtra(
            BluetoothDevice.EXTRA_DEVICE,
            BluetoothDevice::class.java,
        )
    } else {
        @Suppress("DEPRECATION")
        getParcelableExtra(BluetoothDevice.EXTRA_DEVICE)
    }