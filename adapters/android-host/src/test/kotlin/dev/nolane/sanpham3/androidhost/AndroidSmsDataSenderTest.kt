package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertEquals
import org.junit.Assert.assertNull
import org.junit.Test

class AndroidSmsDataSenderTest {
    private fun request(
        confirmed: Boolean = true,
        destination: String = "+15551234567",
        subscriptionId: Int = 1,
        port: Int = 0x5350,
        bytes: ByteArray = byteArrayOf(1, 2, 3),
    ): AndroidSmsSendRequest =
        AndroidSmsSendRequest(
            destinationAddress = destination,
            subscriptionId = subscriptionId,
            destinationPort = port,
            segmentBytes = bytes,
            callerConfirmedCost = confirmed,
        )

    @Test
    fun policyRequiresCallerConfirmedCost() {
        assertEquals(
            AndroidSmsBlockReason.USER_CONSENT_REQUIRED,
            AndroidSmsInputPolicy.validate(
                request(confirmed = false),
                subscriptionValid = true,
            ),
        )
    }

    @Test
    fun policyRejectsInvalidSubscription() {
        assertEquals(
            AndroidSmsBlockReason.INVALID_SUBSCRIPTION_ID,
            AndroidSmsInputPolicy.validate(
                request(),
                subscriptionValid = false,
            ),
        )
    }

    @Test
    fun policyRejectsEmptyDestinationAndInvalidPort() {
        assertEquals(
            AndroidSmsBlockReason.INVALID_DESTINATION,
            AndroidSmsInputPolicy.validate(
                request(destination = "   "),
                subscriptionValid = true,
            ),
        )
        assertEquals(
            AndroidSmsBlockReason.INVALID_DESTINATION_PORT,
            AndroidSmsInputPolicy.validate(
                request(port = 0),
                subscriptionValid = true,
            ),
        )
        assertEquals(
            AndroidSmsBlockReason.INVALID_DESTINATION_PORT,
            AndroidSmsInputPolicy.validate(
                request(port = 0x1_0000),
                subscriptionValid = true,
            ),
        )
    }

    @Test
    fun policyEnforcesProjectSegmentBudget() {
        assertEquals(
            AndroidSmsBlockReason.EMPTY_SEGMENT,
            AndroidSmsInputPolicy.validate(
                request(bytes = byteArrayOf()),
                subscriptionValid = true,
            ),
        )
        assertEquals(
            AndroidSmsBlockReason.SEGMENT_EXCEEDS_PROJECT_BUDGET,
            AndroidSmsInputPolicy.validate(
                request(
                    bytes = ByteArray(
                        SP3_SMS_DATA_BUDGET_BYTES + 1,
                    ),
                ),
                subscriptionValid = true,
            ),
        )
        assertNull(
            AndroidSmsInputPolicy.validate(
                request(
                    bytes = ByteArray(
                        SP3_SMS_DATA_BUDGET_BYTES,
                    ),
                ),
                subscriptionValid = true,
            ),
        )
    }
}
