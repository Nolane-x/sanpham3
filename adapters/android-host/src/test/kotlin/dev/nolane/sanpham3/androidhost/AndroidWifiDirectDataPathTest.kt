package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidWifiDirectDataPathTest {
    @Test
    fun acceptsUnprivilegedTcpPorts() {
        validateWifiDirectPort(1024)
        validateWifiDirectPort(45123)
        validateWifiDirectPort(65535)
    }

    @Test
    fun rejectsPrivilegedOrInvalidPorts() {
        for (port in listOf(-1, 0, 80, 1023, 65536)) {
            assertThrows(IllegalArgumentException::class.java) {
                validateWifiDirectPort(port)
            }
        }
    }
}
