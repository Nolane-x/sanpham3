package dev.nolane.sanpham3.androidhost

import android.Manifest
import android.content.Context
import android.content.pm.PackageManager
import android.net.wifi.aware.WifiAwareManager
import android.os.Build

data class AndroidFeatureReport(
    val wifiDirectHardware: Boolean,
    val wifiAwareHardware: Boolean,
    val wifiAwareAvailableNow: Boolean,
    val bluetoothLeHardware: Boolean,
    val bluetoothLeL2capCocAvailable: Boolean,
    val nearbyWifiPermission: Boolean,
    val bluetoothScanPermission: Boolean,
    val bluetoothAdvertisePermission: Boolean,
    val bluetoothConnectPermission: Boolean,
)

class AndroidFeatureScanner(
    private val context: Context,
) {
    fun scan(): AndroidFeatureReport {
        val packageManager = context.packageManager

        val wifiDirect = packageManager.hasSystemFeature(
            PackageManager.FEATURE_WIFI_DIRECT,
        )
        val wifiAware = Build.VERSION.SDK_INT >= Build.VERSION_CODES.O &&
            packageManager.hasSystemFeature(PackageManager.FEATURE_WIFI_AWARE)
        val wifiAwareAvailable = if (wifiAware) {
            context.getSystemService(WifiAwareManager::class.java)
                ?.isAvailable == true
        } else {
            false
        }

        val bluetoothLe = packageManager.hasSystemFeature(
            PackageManager.FEATURE_BLUETOOTH_LE,
        )

        val nearbyWifiGranted = if (Build.VERSION.SDK_INT >= 33) {
            granted(Manifest.permission.NEARBY_WIFI_DEVICES)
        } else {
            granted(Manifest.permission.ACCESS_FINE_LOCATION)
        }

        val bluetoothScanGranted = if (Build.VERSION.SDK_INT >= 31) {
            granted(Manifest.permission.BLUETOOTH_SCAN)
        } else {
            granted(Manifest.permission.BLUETOOTH)
        }
        val bluetoothAdvertiseGranted = if (Build.VERSION.SDK_INT >= 31) {
            granted(Manifest.permission.BLUETOOTH_ADVERTISE)
        } else {
            granted(Manifest.permission.BLUETOOTH_ADMIN)
        }
        val bluetoothConnectGranted = if (Build.VERSION.SDK_INT >= 31) {
            granted(Manifest.permission.BLUETOOTH_CONNECT)
        } else {
            granted(Manifest.permission.BLUETOOTH)
        }

        return AndroidFeatureReport(
            wifiDirectHardware = wifiDirect,
            wifiAwareHardware = wifiAware,
            wifiAwareAvailableNow = wifiAwareAvailable,
            bluetoothLeHardware = bluetoothLe,
            bluetoothLeL2capCocAvailable =
                bluetoothLe && Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q,
            nearbyWifiPermission = nearbyWifiGranted,
            bluetoothScanPermission = bluetoothScanGranted,
            bluetoothAdvertisePermission = bluetoothAdvertiseGranted,
            bluetoothConnectPermission = bluetoothConnectGranted,
        )
    }

    private fun granted(permission: String): Boolean =
        context.checkSelfPermission(permission) ==
            PackageManager.PERMISSION_GRANTED
}
