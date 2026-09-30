package dev.nolane.sanpham3.recoverylab

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager

object RecoveryLabPermissions {
    fun blePermissions(apiLevel: Int): List<String> =
        if (apiLevel >= 31) {
            listOf(
                Manifest.permission.BLUETOOTH_SCAN,
                Manifest.permission.BLUETOOTH_ADVERTISE,
                Manifest.permission.BLUETOOTH_CONNECT,
            )
        } else {
            listOf(Manifest.permission.ACCESS_FINE_LOCATION)
        }

    fun peerLanPermissions(apiLevel: Int): List<String> =
        buildList {
            add(
                if (apiLevel >= 33) {
                    Manifest.permission.NEARBY_WIFI_DEVICES
                } else {
                    Manifest.permission.ACCESS_FINE_LOCATION
                },
            )
            if (apiLevel >= 37) {
                add(Manifest.permission.ACCESS_LOCAL_NETWORK)
            }
        }

    fun allLabPermissions(apiLevel: Int): List<String> =
        (blePermissions(apiLevel) + peerLanPermissions(apiLevel))
            .distinct()

    fun localNetworkPermissionState(
        context: Context,
        apiLevel: Int,
    ): String =
        if (apiLevel < 37) {
            "not_applicable"
        } else if (
            context.checkSelfPermission(
                Manifest.permission.ACCESS_LOCAL_NETWORK,
            ) == PackageManager.PERMISSION_GRANTED
        ) {
            "granted"
        } else {
            "denied"
        }
}
