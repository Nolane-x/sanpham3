package dev.nolane.sanpham3.androidhost

import android.Manifest
import android.app.PendingIntent
import android.content.Context
import android.content.pm.PackageManager
import android.os.Build
import android.telephony.SmsManager
import android.telephony.SubscriptionManager

const val SP3_SMS_DATA_BUDGET_BYTES: Int = 120

data class AndroidSmsSendRequest(
    val destinationAddress: String,
    val subscriptionId: Int,
    val destinationPort: Int,
    val segmentBytes: ByteArray,
    /**
     * Caller assertion that the user explicitly approved this potentially
     * cost-bearing SMS send.
     *
     * The adapter cannot prove UI consent itself. Product UI must set this
     * only after an explicit confirmation step.
     */
    val callerConfirmedCost: Boolean,
)

enum class AndroidSmsBlockReason {
    USER_CONSENT_REQUIRED,
    TELEPHONY_MESSAGING_UNAVAILABLE,
    SEND_SMS_PERMISSION_MISSING,
    INVALID_SUBSCRIPTION_ID,
    INVALID_DESTINATION,
    INVALID_DESTINATION_PORT,
    EMPTY_SEGMENT,
    SEGMENT_EXCEEDS_PROJECT_BUDGET,
    SMS_SERVICE_UNAVAILABLE,
}

sealed interface AndroidSmsSendResult {
    data class Submitted(
        val subscriptionId: Int,
        val destinationPort: Int,
        val bytes: Int,
    ) : AndroidSmsSendResult

    data class Blocked(
        val reason: AndroidSmsBlockReason,
    ) : AndroidSmsSendResult

    data class PlatformFailure(
        val exceptionType: String,
    ) : AndroidSmsSendResult
}

internal object AndroidSmsInputPolicy {
    fun validate(
        request: AndroidSmsSendRequest,
        subscriptionValid: Boolean,
    ): AndroidSmsBlockReason? {
        if (!request.callerConfirmedCost) {
            return AndroidSmsBlockReason.USER_CONSENT_REQUIRED
        }
        if (!subscriptionValid) {
            return AndroidSmsBlockReason.INVALID_SUBSCRIPTION_ID
        }
        if (request.destinationAddress.isBlank()) {
            return AndroidSmsBlockReason.INVALID_DESTINATION
        }
        if (request.destinationPort !in 1..0xffff) {
            return AndroidSmsBlockReason.INVALID_DESTINATION_PORT
        }
        if (request.segmentBytes.isEmpty()) {
            return AndroidSmsBlockReason.EMPTY_SEGMENT
        }
        if (request.segmentBytes.size > SP3_SMS_DATA_BUDGET_BYTES) {
            return AndroidSmsBlockReason.SEGMENT_EXCEEDS_PROJECT_BUDGET
        }
        return null
    }
}

class AndroidSmsDataSender(
    context: Context,
) {
    private val appContext = context.applicationContext

    fun preflight(
        request: AndroidSmsSendRequest,
    ): AndroidSmsBlockReason? {
        val packageManager = appContext.packageManager
        if (!packageManager.hasSystemFeature(
                PackageManager.FEATURE_TELEPHONY_MESSAGING,
            )
        ) {
            return AndroidSmsBlockReason.TELEPHONY_MESSAGING_UNAVAILABLE
        }

        if (appContext.checkSelfPermission(Manifest.permission.SEND_SMS) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            return AndroidSmsBlockReason.SEND_SMS_PERMISSION_MISSING
        }

        return AndroidSmsInputPolicy.validate(
            request = request,
            subscriptionValid =
                request.subscriptionId !=
                    SubscriptionManager.INVALID_SUBSCRIPTION_ID,
        )
    }

    /**
     * Submits exactly one already-segmented SP3 SMS/data-message payload.
     *
     * The caller owns PendingIntent receivers and must use them to record real
     * send/delivery outcomes. A Submitted result means SmsManager accepted the
     * submission call; it is not delivery evidence.
     */
    fun sendDataSegment(
        request: AndroidSmsSendRequest,
        sentIntent: PendingIntent? = null,
        deliveryIntent: PendingIntent? = null,
    ): AndroidSmsSendResult {
        val blocked = preflight(request)
        if (blocked != null) {
            return AndroidSmsSendResult.Blocked(blocked)
        }

        val baseManager = appContext.getSystemService(SmsManager::class.java)
            ?: return AndroidSmsSendResult.Blocked(
                AndroidSmsBlockReason.SMS_SERVICE_UNAVAILABLE,
            )

        return try {
            val manager = managerForSubscription(
                baseManager,
                request.subscriptionId,
            )
            manager.sendDataMessage(
                request.destinationAddress,
                null,
                request.destinationPort.toShort(),
                request.segmentBytes,
                sentIntent,
                deliveryIntent,
            )
            AndroidSmsSendResult.Submitted(
                subscriptionId = request.subscriptionId,
                destinationPort = request.destinationPort,
                bytes = request.segmentBytes.size,
            )
        } catch (error: SecurityException) {
            AndroidSmsSendResult.PlatformFailure(
                exceptionType = error.javaClass.simpleName,
            )
        } catch (error: IllegalArgumentException) {
            AndroidSmsSendResult.PlatformFailure(
                exceptionType = error.javaClass.simpleName,
            )
        } catch (error: UnsupportedOperationException) {
            AndroidSmsSendResult.PlatformFailure(
                exceptionType = error.javaClass.simpleName,
            )
        }
    }

    @Suppress("DEPRECATION")
    private fun managerForSubscription(
        baseManager: SmsManager,
        subscriptionId: Int,
    ): SmsManager =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            baseManager.createForSubscriptionId(subscriptionId)
        } else {
            SmsManager.getSmsManagerForSubscriptionId(subscriptionId)
        }
}
