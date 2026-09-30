# F6 Android Data-SMS Send Adapter

This adapter connects the SP3M/S3MG software transport to Android's
`SmsManager.sendDataMessage()` send primitive.

It is deliberately send-only at this stage.

## Permission and feature boundary

The adapter requires the host application to declare and obtain:

```text
android.permission.SEND_SMS
```

The android-host library does **not** add that dangerous permission to its own
manifest automatically.

This keeps SMS an explicit host/product opt-in instead of silently expanding
the permission surface of every application that embeds android-host.

Preflight also requires:

- `PackageManager.FEATURE_TELEPHONY_MESSAGING`;
- a non-invalid subscription ID;
- a non-empty destination address;
- destination port in 1..65535;
- one non-empty S3MG segment no larger than the project 120-byte budget;
- `callerConfirmedCost=true`.

`callerConfirmedCost` is a caller assertion, not proof that a UI was shown.
Product UI must set it only after an explicit confirmation step.

## Subscription binding

The sender never relies on ambiguous default-SIM behavior.

For API 31+ it uses:

```kotlin
context.getSystemService(SmsManager::class.java)
    .createForSubscriptionId(subscriptionId)
```

For API 26-30 it uses the older subscription-specific
`SmsManager.getSmsManagerForSubscriptionId(subscriptionId)` API.

The caller is responsible for choosing the subscription ID.

The adapter does not request `READ_PHONE_STATE` or enumerate SIMs.

## Send API

```kotlin
val sender = AndroidSmsDataSender(context)

val request = AndroidSmsSendRequest(
    destinationAddress = "+15551234567",
    subscriptionId = chosenSubscriptionId,
    destinationPort = 0x5350,
    segmentBytes = encodedS3mgSegment,
    callerConfirmedCost = true,
)

val result = sender.sendDataSegment(
    request = request,
    sentIntent = sentPendingIntent,
    deliveryIntent = deliveryPendingIntent,
)
```

A result of `Submitted` means only that the platform call returned without a
synchronous exception.

It is **not** delivery evidence.

Real send/delivery evidence must come from the caller-owned PendingIntents and
be included in a physical court.

## Feature report

`AndroidFeatureReport` now exposes:

- `telephonyMessagingHardware`;
- `sendSmsPermission`.

This makes SMS eligibility visible to the recovery UI/engine without attempting
a send.

## Typed failure behavior

Preflight can fail closed with:

- `USER_CONSENT_REQUIRED`;
- `TELEPHONY_MESSAGING_UNAVAILABLE`;
- `SEND_SMS_PERMISSION_MISSING`;
- `INVALID_SUBSCRIPTION_ID`;
- `INVALID_DESTINATION`;
- `INVALID_DESTINATION_PORT`;
- `EMPTY_SEGMENT`;
- `SEGMENT_EXCEEDS_PROJECT_BUDGET`;
- `SMS_SERVICE_UNAVAILABLE`.

Synchronous Android exceptions are returned as `PlatformFailure` and are not
reported as delivery failures/successes.

## Evidence boundary

This closes the Android **send-adapter compile/policy baseline** only.

It does not close:

- host-app runtime permission UX;
- subscription picker UX;
- incoming data-SMS receive integration;
- actual carrier delivery;
- operator segment-size behavior;
- sent/delivery PendingIntent evidence;
- monetary cost;
- roaming behavior;
- Play distribution/policy eligibility;
- gateway interoperability.

Those require separate product and physical courts before F7 promotion.
