package dev.nolane.sanpham3.androidhost

import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothManager
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.bluetooth.le.ScanCallback
import android.bluetooth.le.ScanFilter
import android.bluetooth.le.ScanResult
import android.bluetooth.le.ScanSettings
import android.content.Context
import android.os.ParcelUuid
import java.io.Closeable
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidBlePeer(
    val addressHint: String,
    val rssi: Int,
    val serviceData: ByteArray,
    internal val device: BluetoothDevice? = null,
)

sealed interface AndroidBleEvent {
    data object Started : AndroidBleEvent

    data class PeerDiscovered(
        val peer: AndroidBlePeer,
    ) : AndroidBleEvent

    data class Failed(
        val code: Int?,
        val detail: String,
    ) : AndroidBleEvent
}

/**
 * BLE is a discovery/control fallback, not the high-bandwidth data plane.
 *
 * Bluetooth addresses may be randomized and are treated only as route hints.
 * Project identity is established later by the authenticated peer session.
 */
class AndroidBleDiscovery(
    context: Context,
) : Closeable {
    companion object {
        private val SERVICE_UUID = ParcelUuid(
            UUID.fromString("4a934f10-7c8c-4b74-9cb2-7be5806f3a31"),
        )

        // Conservative payload limit for broad legacy-advertising compatibility.
        private const val MAX_SERVICE_DATA_BYTES = 12
    }

    private val bluetoothManager =
        context.applicationContext.getSystemService(BluetoothManager::class.java)
    private val adapter
        get() = bluetoothManager?.adapter

    private val started = AtomicBoolean(false)
    private var listener: ((AndroidBleEvent) -> Unit)? = null

    private val advertiseCallback = object : AdvertiseCallback() {
        override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
            listener?.invoke(AndroidBleEvent.Started)
        }

        override fun onStartFailure(errorCode: Int) {
            listener?.invoke(
                AndroidBleEvent.Failed(
                    code = errorCode,
                    detail = "BLE advertising failed",
                ),
            )
        }
    }

    private val scanCallback = object : ScanCallback() {
        override fun onScanResult(
            callbackType: Int,
            result: ScanResult,
        ) {
            val payload = result.scanRecord
                ?.getServiceData(SERVICE_UUID)
                ?.copyOf()
                ?: return

            val device = result.device
            val addressHint = try {
                device.address.orEmpty()
            } catch (_: SecurityException) {
                ""
            }

            listener?.invoke(
                AndroidBleEvent.PeerDiscovered(
                    AndroidBlePeer(
                        addressHint = addressHint,
                        rssi = result.rssi,
                        serviceData = payload,
                        device = device,
                    ),
                ),
            )
        }

        override fun onScanFailed(errorCode: Int) {
            listener?.invoke(
                AndroidBleEvent.Failed(
                    code = errorCode,
                    detail = "BLE scan failed",
                ),
            )
        }
    }

    fun start(
        discoveryInfo: ByteArray,
        onEvent: (AndroidBleEvent) -> Unit,
    ) {
        require(discoveryInfo.size <= MAX_SERVICE_DATA_BYTES) {
            "BLE discoveryInfo must be <= $MAX_SERVICE_DATA_BYTES bytes"
        }

        if (!started.compareAndSet(false, true)) {
            return
        }

        listener = onEvent

        val bluetoothAdapter = adapter
        if (bluetoothAdapter == null || !bluetoothAdapter.isEnabled) {
            failAndClose("Bluetooth adapter unavailable or disabled")
            return
        }

        val advertiser = bluetoothAdapter.bluetoothLeAdvertiser
        val scanner = bluetoothAdapter.bluetoothLeScanner
        if (advertiser == null || scanner == null) {
            failAndClose("BLE advertising/scanning unavailable")
            return
        }

        val advertiseSettings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setConnectable(false)
            .setTimeout(0)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .build()

        // Keep the 128-bit UUID in the primary legacy packet and move
        // service data to the scan response. Putting both in the same 31-byte
        // legacy advertisement can overflow once discoveryInfo is non-trivial.
        val advertiseData = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .addServiceUuid(SERVICE_UUID)
            .build()

        val scanResponse = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .setIncludeTxPowerLevel(false)
            .addServiceData(SERVICE_UUID, discoveryInfo)
            .build()

        val filter = ScanFilter.Builder()
            .setServiceUuid(SERVICE_UUID)
            .build()

        val scanSettings = ScanSettings.Builder()
            .setScanMode(ScanSettings.SCAN_MODE_LOW_LATENCY)
            .build()

        try {
            advertiser.startAdvertising(
                advertiseSettings,
                advertiseData,
                scanResponse,
                advertiseCallback,
            )
            scanner.startScan(
                listOf(filter),
                scanSettings,
                scanCallback,
            )
        } catch (error: SecurityException) {
            failAndClose(error.message ?: "Bluetooth permission denied")
        } catch (error: RuntimeException) {
            failAndClose(error.message ?: error.javaClass.simpleName)
        }
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) {
            return
        }

        val bluetoothAdapter = adapter

        try {
            bluetoothAdapter
                ?.bluetoothLeAdvertiser
                ?.stopAdvertising(advertiseCallback)
        } catch (_: SecurityException) {
            // Permission may have been revoked while Recovery Mode was active.
        }

        try {
            bluetoothAdapter
                ?.bluetoothLeScanner
                ?.stopScan(scanCallback)
        } catch (_: SecurityException) {
            // Permission may have been revoked while Recovery Mode was active.
        }

        listener = null
    }

    private fun failAndClose(detail: String) {
        val callback = listener
        close()
        callback?.invoke(
            AndroidBleEvent.Failed(
                code = null,
                detail = detail,
            ),
        )
    }
}
