package dev.nolane.sanpham3.androidhost

import android.hardware.usb.UsbConstants
import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AndroidUsbBulkTransportTest {
    @Test
    fun selectsBulkInAndOutAndIgnoresOtherEndpointTypes() {
        val pair = chooseUsbBulkPair(
            listOf(
                AndroidUsbEndpointDescriptor(
                    address = 1,
                    direction = UsbConstants.USB_DIR_OUT,
                    type = UsbConstants.USB_ENDPOINT_XFER_INT,
                    maxPacketSize = 16,
                ),
                AndroidUsbEndpointDescriptor(
                    address = 0x82,
                    direction = UsbConstants.USB_DIR_IN,
                    type = UsbConstants.USB_ENDPOINT_XFER_BULK,
                    maxPacketSize = 512,
                ),
                AndroidUsbEndpointDescriptor(
                    address = 0x03,
                    direction = UsbConstants.USB_DIR_OUT,
                    type = UsbConstants.USB_ENDPOINT_XFER_BULK,
                    maxPacketSize = 512,
                ),
            ),
        )

        assertEquals(
            AndroidUsbBulkPair(
                inputAddress = 0x82,
                outputAddress = 0x03,
                inputMaxPacketSize = 512,
                outputMaxPacketSize = 512,
            ),
            pair,
        )
    }

    @Test
    fun requiresBothBulkDirections() {
        assertNull(
            chooseUsbBulkPair(
                listOf(
                    AndroidUsbEndpointDescriptor(
                        address = 0x81,
                        direction = UsbConstants.USB_DIR_IN,
                        type = UsbConstants.USB_ENDPOINT_XFER_BULK,
                        maxPacketSize = 64,
                    ),
                ),
            ),
        )
    }

    @Test
    fun deterministicChoiceUsesFirstBulkEndpointPerDirection() {
        val pair = chooseUsbBulkPair(
            listOf(
                AndroidUsbEndpointDescriptor(
                    address = 0x81,
                    direction = UsbConstants.USB_DIR_IN,
                    type = UsbConstants.USB_ENDPOINT_XFER_BULK,
                    maxPacketSize = 64,
                ),
                AndroidUsbEndpointDescriptor(
                    address = 0x82,
                    direction = UsbConstants.USB_DIR_IN,
                    type = UsbConstants.USB_ENDPOINT_XFER_BULK,
                    maxPacketSize = 512,
                ),
                AndroidUsbEndpointDescriptor(
                    address = 0x01,
                    direction = UsbConstants.USB_DIR_OUT,
                    type = UsbConstants.USB_ENDPOINT_XFER_BULK,
                    maxPacketSize = 64,
                ),
                AndroidUsbEndpointDescriptor(
                    address = 0x02,
                    direction = UsbConstants.USB_DIR_OUT,
                    type = UsbConstants.USB_ENDPOINT_XFER_BULK,
                    maxPacketSize = 512,
                ),
            ),
        )

        assertEquals(0x81, pair?.inputAddress)
        assertEquals(0x01, pair?.outputAddress)
    }
}
