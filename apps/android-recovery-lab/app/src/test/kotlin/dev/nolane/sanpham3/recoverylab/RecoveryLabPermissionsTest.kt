package dev.nolane.sanpham3.recoverylab

import android.Manifest
import org.junit.Assert.assertEquals
import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class RecoveryLabPermissionsTest {
    @Test
    fun api30UsesLocationCompatibilityPermissions() {
        assertEquals(
            listOf(Manifest.permission.ACCESS_FINE_LOCATION),
            RecoveryLabPermissions.blePermissions(30),
        )
        assertEquals(
            listOf(Manifest.permission.ACCESS_FINE_LOCATION),
            RecoveryLabPermissions.peerLanPermissions(30),
        )
    }

    @Test
    fun api33UsesNearbyWifiWithoutAndroid17LanPermission() {
        val permissions =
            RecoveryLabPermissions.peerLanPermissions(33)

        assertTrue(
            permissions.contains(
                Manifest.permission.NEARBY_WIFI_DEVICES,
            ),
        )
        assertFalse(
            permissions.contains(
                Manifest.permission.ACCESS_LOCAL_NETWORK,
            ),
        )
    }

    @Test
    fun api37RequiresNearbyWifiAndAccessLocalNetworkForPeerLan() {
        val permissions =
            RecoveryLabPermissions.peerLanPermissions(37)

        assertEquals(
            listOf(
                Manifest.permission.NEARBY_WIFI_DEVICES,
                Manifest.permission.ACCESS_LOCAL_NETWORK,
            ),
            permissions,
        )
    }

    @Test
    fun fullLabPermissionsAreDistinct() {
        val permissions =
            RecoveryLabPermissions.allLabPermissions(37)

        assertEquals(5, permissions.size)
        assertTrue(
            permissions.contains(
                Manifest.permission.BLUETOOTH_SCAN,
            ),
        )
        assertTrue(
            permissions.contains(
                Manifest.permission.ACCESS_LOCAL_NETWORK,
            ),
        )
    }
}
