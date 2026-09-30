package dev.nolane.sanpham3.androidhost

import android.content.Context
import android.content.pm.PackageManager
import android.hardware.usb.UsbConstants
import android.hardware.usb.UsbDevice
import android.hardware.usb.UsbDeviceConnection
import android.hardware.usb.UsbEndpoint
import android.hardware.usb.UsbInterface
import android.hardware.usb.UsbManager
import java.io.Closeable
import java.io.IOException
import java.io.InputStream
import java.io.OutputStream
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.min

private const val USB_MAX_TRANSFER_BYTES = 16 * 1024

data class AndroidUsbEndpointDescriptor(
    val address: Int,
    val direction: Int,
    val type: Int,
    val maxPacketSize: Int,
)

data class AndroidUsbBulkPair(
    val inputAddress: Int,
    val outputAddress: Int,
    val inputMaxPacketSize: Int,
    val outputMaxPacketSize: Int,
)

internal fun chooseUsbBulkPair(
    endpoints: List<AndroidUsbEndpointDescriptor>,
): AndroidUsbBulkPair? {
    val input = endpoints.firstOrNull {
        it.type == UsbConstants.USB_ENDPOINT_XFER_BULK &&
            it.direction == UsbConstants.USB_DIR_IN
    } ?: return null
    val output = endpoints.firstOrNull {
        it.type == UsbConstants.USB_ENDPOINT_XFER_BULK &&
            it.direction == UsbConstants.USB_DIR_OUT
    } ?: return null

    return AndroidUsbBulkPair(
        inputAddress = input.address,
        outputAddress = output.address,
        inputMaxPacketSize = input.maxPacketSize,
        outputMaxPacketSize = output.maxPacketSize,
    )
}

data class AndroidUsbBulkCandidate(
    val deviceId: Int,
    val vendorId: Int,
    val productId: Int,
    val interfaceId: Int,
    val inputEndpointAddress: Int,
    val outputEndpointAddress: Int,
    val inputMaxPacketSize: Int,
    val outputMaxPacketSize: Int,
    val permissionGranted: Boolean,
)

enum class AndroidUsbOpenFailure {
    USB_HOST_UNAVAILABLE,
    DEVICE_NOT_PRESENT,
    USB_PERMISSION_REQUIRED,
    INTERFACE_OR_ENDPOINTS_CHANGED,
    OPEN_DEVICE_FAILED,
    CLAIM_INTERFACE_FAILED,
}

sealed interface AndroidUsbOpenResult {
    data class Ready(
        val transport: AndroidUsbBulkTransport,
    ) : AndroidUsbOpenResult

    data class Blocked(
        val reason: AndroidUsbOpenFailure,
    ) : AndroidUsbOpenResult
}

class AndroidUsbBulkDataPath(
    context: Context,
) {
    private val appContext = context.applicationContext
    private val usbManager =
        appContext.getSystemService(UsbManager::class.java)

    fun scan(): List<AndroidUsbBulkCandidate> {
        if (!appContext.packageManager.hasSystemFeature(
                PackageManager.FEATURE_USB_HOST,
            )
        ) {
            return emptyList()
        }

        val manager = usbManager ?: return emptyList()
        val candidates = mutableListOf<AndroidUsbBulkCandidate>()

        for (device in manager.deviceList.values) {
            for (interfaceIndex in 0 until device.interfaceCount) {
                val usbInterface = device.getInterface(interfaceIndex)
                val endpoints = endpointDescriptors(usbInterface)
                val pair = chooseUsbBulkPair(endpoints) ?: continue

                candidates += AndroidUsbBulkCandidate(
                    deviceId = device.deviceId,
                    vendorId = device.vendorId,
                    productId = device.productId,
                    interfaceId = usbInterface.id,
                    inputEndpointAddress = pair.inputAddress,
                    outputEndpointAddress = pair.outputAddress,
                    inputMaxPacketSize = pair.inputMaxPacketSize,
                    outputMaxPacketSize = pair.outputMaxPacketSize,
                    permissionGranted = manager.hasPermission(device),
                )
            }
        }

        return candidates.sortedWith(
            compareBy<AndroidUsbBulkCandidate>(
                { it.deviceId },
                { it.interfaceId },
                { it.inputEndpointAddress },
                { it.outputEndpointAddress },
            ),
        )
    }

    fun open(
        candidate: AndroidUsbBulkCandidate,
        timeoutMillis: Int = 5_000,
    ): AndroidUsbOpenResult {
        require(timeoutMillis > 0) {
            "timeoutMillis must be positive"
        }

        if (!appContext.packageManager.hasSystemFeature(
                PackageManager.FEATURE_USB_HOST,
            )
        ) {
            return AndroidUsbOpenResult.Blocked(
                AndroidUsbOpenFailure.USB_HOST_UNAVAILABLE,
            )
        }

        val manager = usbManager
            ?: return AndroidUsbOpenResult.Blocked(
                AndroidUsbOpenFailure.USB_HOST_UNAVAILABLE,
            )

        val device = manager.deviceList.values.firstOrNull {
            it.deviceId == candidate.deviceId
        } ?: return AndroidUsbOpenResult.Blocked(
            AndroidUsbOpenFailure.DEVICE_NOT_PRESENT,
        )

        if (!manager.hasPermission(device)) {
            return AndroidUsbOpenResult.Blocked(
                AndroidUsbOpenFailure.USB_PERMISSION_REQUIRED,
            )
        }

        val selected = findSelection(device, candidate)
            ?: return AndroidUsbOpenResult.Blocked(
                AndroidUsbOpenFailure.INTERFACE_OR_ENDPOINTS_CHANGED,
            )

        val connection = manager.openDevice(device)
            ?: return AndroidUsbOpenResult.Blocked(
                AndroidUsbOpenFailure.OPEN_DEVICE_FAILED,
            )

        if (!connection.claimInterface(selected.usbInterface, true)) {
            connection.close()
            return AndroidUsbOpenResult.Blocked(
                AndroidUsbOpenFailure.CLAIM_INTERFACE_FAILED,
            )
        }

        return AndroidUsbOpenResult.Ready(
            AndroidUsbBulkTransport(
                connection = connection,
                usbInterface = selected.usbInterface,
                inputEndpoint = selected.inputEndpoint,
                outputEndpoint = selected.outputEndpoint,
                timeoutMillis = timeoutMillis,
            ),
        )
    }

    private fun endpointDescriptors(
        usbInterface: UsbInterface,
    ): List<AndroidUsbEndpointDescriptor> =
        (0 until usbInterface.endpointCount).map { index ->
            val endpoint = usbInterface.getEndpoint(index)
            AndroidUsbEndpointDescriptor(
                address = endpoint.address,
                direction = endpoint.direction,
                type = endpoint.type,
                maxPacketSize = endpoint.maxPacketSize,
            )
        }

    private fun findSelection(
        device: UsbDevice,
        candidate: AndroidUsbBulkCandidate,
    ): SelectedEndpoints? {
        for (interfaceIndex in 0 until device.interfaceCount) {
            val usbInterface = device.getInterface(interfaceIndex)
            if (usbInterface.id != candidate.interfaceId) {
                continue
            }

            var input: UsbEndpoint? = null
            var output: UsbEndpoint? = null

            for (endpointIndex in 0 until usbInterface.endpointCount) {
                val endpoint = usbInterface.getEndpoint(endpointIndex)
                if (endpoint.type != UsbConstants.USB_ENDPOINT_XFER_BULK) {
                    continue
                }
                if (endpoint.address == candidate.inputEndpointAddress &&
                    endpoint.direction == UsbConstants.USB_DIR_IN
                ) {
                    input = endpoint
                }
                if (endpoint.address == candidate.outputEndpointAddress &&
                    endpoint.direction == UsbConstants.USB_DIR_OUT
                ) {
                    output = endpoint
                }
            }

            if (input != null && output != null) {
                return SelectedEndpoints(
                    usbInterface = usbInterface,
                    inputEndpoint = input,
                    outputEndpoint = output,
                )
            }
        }
        return null
    }

    private data class SelectedEndpoints(
        val usbInterface: UsbInterface,
        val inputEndpoint: UsbEndpoint,
        val outputEndpoint: UsbEndpoint,
    )
}

class AndroidUsbBulkTransport internal constructor(
    private val connection: UsbDeviceConnection,
    private val usbInterface: UsbInterface,
    inputEndpoint: UsbEndpoint,
    outputEndpoint: UsbEndpoint,
    timeoutMillis: Int,
) : Closeable {
    private val closed = AtomicBoolean(false)

    internal val inputStream: InputStream = UsbBulkInputStream(
        connection = connection,
        endpoint = inputEndpoint,
        timeoutMillis = timeoutMillis,
        closed = closed,
    )

    internal val outputStream: OutputStream = UsbBulkOutputStream(
        connection = connection,
        endpoint = outputEndpoint,
        timeoutMillis = timeoutMillis,
        closed = closed,
    )

    override fun close() {
        if (!closed.compareAndSet(false, true)) {
            return
        }

        try {
            connection.releaseInterface(usbInterface)
        } finally {
            connection.close()
        }
    }
}

private class UsbBulkInputStream(
    private val connection: UsbDeviceConnection,
    private val endpoint: UsbEndpoint,
    private val timeoutMillis: Int,
    private val closed: AtomicBoolean,
) : InputStream() {
    override fun read(): Int {
        val single = ByteArray(1)
        val count = read(single, 0, 1)
        return if (count < 0) -1 else single[0].toInt() and 0xff
    }

    override fun read(
        bytes: ByteArray,
        offset: Int,
        length: Int,
    ): Int {
        checkBounds(bytes, offset, length)
        check(!closed.get()) {
            "USB bulk transport is closed"
        }
        if (length == 0) {
            return 0
        }

        val request = min(length, USB_MAX_TRANSFER_BYTES)
        val count = connection.bulkTransfer(
            endpoint,
            bytes,
            offset,
            request,
            timeoutMillis,
        )
        if (count <= 0) {
            throw IOException(
                "USB bulk IN failed or timed out (result=$count)",
            )
        }
        return count
    }
}

private class UsbBulkOutputStream(
    private val connection: UsbDeviceConnection,
    private val endpoint: UsbEndpoint,
    private val timeoutMillis: Int,
    private val closed: AtomicBoolean,
) : OutputStream() {
    override fun write(value: Int) {
        write(byteArrayOf(value.toByte()), 0, 1)
    }

    override fun write(
        bytes: ByteArray,
        offset: Int,
        length: Int,
    ) {
        checkBounds(bytes, offset, length)
        check(!closed.get()) {
            "USB bulk transport is closed"
        }

        var written = 0
        while (written < length) {
            val request = min(
                length - written,
                USB_MAX_TRANSFER_BYTES,
            )
            val count = connection.bulkTransfer(
                endpoint,
                bytes,
                offset + written,
                request,
                timeoutMillis,
            )
            if (count <= 0) {
                throw IOException(
                    "USB bulk OUT failed or timed out (result=$count)",
                )
            }
            written += count
        }
    }
}

private fun checkBounds(
    bytes: ByteArray,
    offset: Int,
    length: Int,
) {
    if (offset < 0 ||
        length < 0 ||
        offset > bytes.size ||
        length > bytes.size - offset
    ) {
        throw IndexOutOfBoundsException(
            "offset=$offset length=$length size=${bytes.size}",
        )
    }
}
