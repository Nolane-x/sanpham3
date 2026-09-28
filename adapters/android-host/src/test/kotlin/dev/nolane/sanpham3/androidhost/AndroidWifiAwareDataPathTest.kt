package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertThrows
import org.junit.Test

class AndroidWifiAwareDataPathTest {
    @Test
    fun acceptsPrintableAsciiPassphraseInAllowedRange() {
        validatePassphrase("12345678")
        validatePassphrase("A-secure-lab-passphrase-42")
    }

    @Test
    fun rejectsTooShortPassphrase() {
        assertThrows(IllegalArgumentException::class.java) {
            validatePassphrase("short")
        }
    }

    @Test
    fun rejectsNonPrintableAscii() {
        assertThrows(IllegalArgumentException::class.java) {
            validatePassphrase("1234567\n")
        }
    }
}
