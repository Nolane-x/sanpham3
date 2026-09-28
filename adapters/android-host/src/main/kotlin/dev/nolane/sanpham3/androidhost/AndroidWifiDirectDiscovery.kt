package dev.nolane.sanpham3.androidhost

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.wifi.p2p.WifiP2pDevice
import android.net.wifi.p2p.WifiP2pManager
import android.os.Build
import java.io.Closeable
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidWifiDirectPeer(
    val deviceName: String,
    val deviceAddressHint: String,
    val status: Int,
)

sealed interface AndroidWifiDirectEvent {
    data object DiscoveryStarted : AndroidWifiDirectEvent

    data class Peers(
        val peers: List<AndroidWifiDirectPeer>,
    ) : AndroidWifiDirectEvent

    data class State(
        val enabled: Boolean,
    ) : AndroidWifiDirectEvent

    data class Failed(
        val reasonCode: Int?,
        val detail: String,
    ) : AndroidWifiDirectEvent
}

/**
 * Discovers nearby Wi-Fi Direct peers.
 *
 * Device addresses are discovery hints only. They are not trusted project
 * identities and must never replace the authenticated peer-session handshake.
 */
class AndroidWifiDirectDiscovery(
    context: Context,
) : Closeable {
    private val appContext = context.applicationContext
    private val manager = appContext.getSystemService(
        Context.WIFI_P2P_SERVICE,
    ) as? WifiP2pManager

    private val started = AtomicBoolean(false)
    private var listener: ((AndroidWifiDirectEvent) -> Unit)? = null

    private val channel by lazy {
        requireNotNull(manager) {
            "WifiP2pManager unavailable"
        }.initialize(appContext, appContext.mainLooper, null)
    }

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            when (intent?.action) {
                WifiP2pManager.WIFI_P2P_STATE_CHANGED_ACTION -> {
                    val state = intent.getIntExtra(
                        WifiP2pManager.EXTRA_WIFI_STATE,
                        WifiP2pManager.WIFI_P2P_STATE_DISABLED,
                    )
                    listener?.invoke(
                        AndroidWifiDirectEvent.State(
                            enabled =
                                state == WifiP2pManager.WIFI_P2P_STATE_ENABLED,
                        ),
                    )
                }

                WifiP2pManager.WIFI_P2P_PEERS_CHANGED_ACTION -> {
                    requestPeers()
                }
            }
        }
    }

    fun start(
        onEvent: (AndroidWifiDirectEvent) -> Unit,
    ) {
        if (!started.compareAndSet(false, true)) {
            return
        }

        listener = onEvent

        val wifiP2p = manager
        if (wifiP2p == null) {
            started.set(false)
            listener = null
            onEvent(
                AndroidWifiDirectEvent.Failed(
                    reasonCode = null,
                    detail = "WifiP2pManager unavailable",
                ),
            )
            return
        }

        val filter = IntentFilter().apply {
            addAction(WifiP2pManager.WIFI_P2P_STATE_CHANGED_ACTION)
            addAction(WifiP2pManager.WIFI_P2P_PEERS_CHANGED_ACTION)
        }

        try {
            if (Build.VERSION.SDK_INT >= 33) {
                appContext.registerReceiver(
                    receiver,
                    filter,
                    Context.RECEIVER_NOT_EXPORTED,
                )
            } else {
                @Suppress("DEPRECATION")
                appContext.registerReceiver(receiver, filter)
            }

            wifiP2p.discoverPeers(
                channel,
                object : WifiP2pManager.ActionListener {
                    override fun onSuccess() {
                        listener?.invoke(
                            AndroidWifiDirectEvent.DiscoveryStarted,
                        )
                    }

                    override fun onFailure(reason: Int) {
                        listener?.invoke(
                            AndroidWifiDirectEvent.Failed(
                                reasonCode = reason,
                                detail = "discoverPeers failed",
                            ),
                        )
                    }
                },
            )
        } catch (error: SecurityException) {
            close()
            onEvent(
                AndroidWifiDirectEvent.Failed(
                    reasonCode = null,
                    detail = error.message ?: "Wi-Fi Direct permission denied",
                ),
            )
        } catch (error: RuntimeException) {
            close()
            onEvent(
                AndroidWifiDirectEvent.Failed(
                    reasonCode = null,
                    detail = error.message ?: error.javaClass.simpleName,
                ),
            )
        }
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) {
            return
        }

        try {
            appContext.unregisterReceiver(receiver)
        } catch (_: IllegalArgumentException) {
            // Receiver was not registered or was already removed.
        }

        listener = null
    }

    private fun requestPeers() {
        val wifiP2p = manager ?: return

        try {
            wifiP2p.requestPeers(channel) { peerList ->
                val peers = peerList.deviceList
                    .map(WifiP2pDevice::toDiscoveryPeer)
                    .sortedWith(
                        compareBy(
                            AndroidWifiDirectPeer::deviceName,
                            AndroidWifiDirectPeer::deviceAddressHint,
                        ),
                    )

                listener?.invoke(AndroidWifiDirectEvent.Peers(peers))
            }
        } catch (error: SecurityException) {
            listener?.invoke(
                AndroidWifiDirectEvent.Failed(
                    reasonCode = null,
                    detail = error.message ?: "requestPeers permission denied",
                ),
            )
        }
    }

    private fun WifiP2pDevice.toDiscoveryPeer(): AndroidWifiDirectPeer =
        AndroidWifiDirectPeer(
            deviceName = deviceName.orEmpty(),
            deviceAddressHint = deviceAddress.orEmpty(),
            status = status,
        )
}
