package dev.nolane.sanpham3.androidhost

import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.net.MacAddress
import android.net.NetworkInfo
import android.net.wifi.p2p.WifiP2pConfig
import android.net.wifi.p2p.WifiP2pInfo
import android.net.wifi.p2p.WifiP2pManager
import android.os.Build
import java.io.Closeable
import java.net.InetAddress
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidWifiDirectEndpoint(
    val groupOwner: Boolean,
    val groupOwnerAddress: InetAddress?,
    val port: Int,
)

sealed interface AndroidWifiDirectDataPathEvent {
    data object Connecting : AndroidWifiDirectDataPathEvent

    data class GroupFormed(
        val endpoint: AndroidWifiDirectEndpoint,
    ) : AndroidWifiDirectDataPathEvent

    data object Disconnected : AndroidWifiDirectDataPathEvent

    data class Failed(
        val reasonCode: Int?,
        val detail: String,
    ) : AndroidWifiDirectDataPathEvent
}

/**
 * Turns a discovered Wi-Fi Direct peer hint into a socket-capable P2P path.
 *
 * The Wi-Fi Direct device address is never treated as project identity.
 * After a socket is established, the project peer-session protocol must still
 * authenticate and encrypt the application traffic.
 *
 * V0 uses a caller-selected TCP port shared by both peers. A later service
 * advertisement layer can negotiate dynamic ports.
 */
class AndroidWifiDirectDataPath(
    context: Context,
) : Closeable {
    private val appContext = context.applicationContext
    private val manager = appContext.getSystemService(
        Context.WIFI_P2P_SERVICE,
    ) as? WifiP2pManager

    private val channel by lazy {
        requireNotNull(manager) {
            "WifiP2pManager unavailable"
        }.initialize(appContext, appContext.mainLooper, null)
    }

    private val started = AtomicBoolean(false)
    private var listener: ((AndroidWifiDirectDataPathEvent) -> Unit)? = null
    private var projectPort: Int = 0

    @Volatile
    private var latestInfo: WifiP2pInfo? = null

    private var serverSocket: ServerSocket? = null

    private val receiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action !=
                WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION
            ) {
                return
            }

            val networkInfo = if (Build.VERSION.SDK_INT >= 33) {
                intent.getParcelableExtra(
                    WifiP2pManager.EXTRA_NETWORK_INFO,
                    NetworkInfo::class.java,
                )
            } else {
                @Suppress("DEPRECATION")
                intent.getParcelableExtra<NetworkInfo>(
                    WifiP2pManager.EXTRA_NETWORK_INFO,
                )
            }

            if (networkInfo?.isConnected != true) {
                latestInfo = null
                closeServerSocket()
                listener?.invoke(AndroidWifiDirectDataPathEvent.Disconnected)
                return
            }

            requestConnectionInfo()
        }
    }

    fun start(
        peer: AndroidWifiDirectPeer,
        port: Int,
        onEvent: (AndroidWifiDirectDataPathEvent) -> Unit,
    ) {
        validateWifiDirectPort(port)
        require(peer.deviceAddressHint.isNotBlank()) {
            "Wi-Fi Direct peer address hint is empty"
        }

        if (!started.compareAndSet(false, true)) {
            return
        }

        listener = onEvent
        projectPort = port

        val wifiP2p = manager
        if (wifiP2p == null) {
            failAndClose(
                null,
                "WifiP2pManager unavailable",
            )
            return
        }

        val filter = IntentFilter(
            WifiP2pManager.WIFI_P2P_CONNECTION_CHANGED_ACTION,
        )

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

            val config = buildConfig(peer.deviceAddressHint)

            wifiP2p.connect(
                channel,
                config,
                object : WifiP2pManager.ActionListener {
                    override fun onSuccess() {
                        listener?.invoke(
                            AndroidWifiDirectDataPathEvent.Connecting,
                        )
                    }

                    override fun onFailure(reason: Int) {
                        listener?.invoke(
                            AndroidWifiDirectDataPathEvent.Failed(
                                reasonCode = reason,
                                detail = "WifiP2pManager.connect failed",
                            ),
                        )
                    }
                },
            )
        } catch (error: SecurityException) {
            failAndClose(
                null,
                error.message ?: "Wi-Fi Direct permission denied",
            )
        } catch (error: IllegalArgumentException) {
            failAndClose(
                null,
                error.message ?: "invalid Wi-Fi Direct peer address",
            )
        } catch (error: RuntimeException) {
            failAndClose(
                null,
                error.message ?: error.javaClass.simpleName,
            )
        }
    }

    /**
     * Called by the Wi-Fi Direct group owner after GroupFormed.
     */
    fun accept(timeoutMillis: Int): Socket {
        require(timeoutMillis > 0)

        val info = checkNotNull(latestInfo) {
            "Wi-Fi Direct group is not formed"
        }
        check(info.groupFormed && info.isGroupOwner) {
            "this device is not the Wi-Fi Direct group owner"
        }

        val socket = serverSocket ?: ServerSocket(projectPort).also {
            serverSocket = it
        }
        socket.soTimeout = timeoutMillis
        return socket.accept()
    }

    /**
     * Called by a Wi-Fi Direct group client after GroupFormed.
     */
    fun connect(timeoutMillis: Int): Socket {
        require(timeoutMillis > 0)

        val info = checkNotNull(latestInfo) {
            "Wi-Fi Direct group is not formed"
        }
        check(info.groupFormed && !info.isGroupOwner) {
            "this device is the Wi-Fi Direct group owner"
        }

        val owner = checkNotNull(info.groupOwnerAddress) {
            "Wi-Fi Direct group owner address unavailable"
        }

        val socket = Socket()
        try {
            socket.connect(
                InetSocketAddress(owner, projectPort),
                timeoutMillis,
            )
            return socket
        } catch (error: Exception) {
            socket.close()
            throw error
        }
    }

    fun currentEndpoint(): AndroidWifiDirectEndpoint? =
        latestInfo
            ?.takeIf(WifiP2pInfo::groupFormed)
            ?.let { info ->
                AndroidWifiDirectEndpoint(
                    groupOwner = info.isGroupOwner,
                    groupOwnerAddress = info.groupOwnerAddress,
                    port = projectPort,
                )
            }

    override fun close() {
        if (!started.compareAndSet(true, false)) {
            return
        }

        closeServerSocket()
        latestInfo = null

        try {
            appContext.unregisterReceiver(receiver)
        } catch (_: IllegalArgumentException) {
            // Receiver was not registered or was already removed.
        }

        listener = null
        projectPort = 0
    }

    /**
     * Explicitly remove a P2P group created/used by this recovery session.
     *
     * close() intentionally does not call this automatically because a group
     * may be shared with another app/user flow.
     */
    fun removeGroup(
        onResult: (Boolean, Int?) -> Unit = { _, _ -> },
    ) {
        val wifiP2p = manager
        if (wifiP2p == null) {
            onResult(false, null)
            return
        }

        try {
            wifiP2p.removeGroup(
                channel,
                object : WifiP2pManager.ActionListener {
                    override fun onSuccess() {
                        onResult(true, null)
                    }

                    override fun onFailure(reason: Int) {
                        onResult(false, reason)
                    }
                },
            )
        } catch (_: SecurityException) {
            onResult(false, null)
        }
    }

    private fun requestConnectionInfo() {
        val wifiP2p = manager ?: return

        try {
            wifiP2p.requestConnectionInfo(channel) { info ->
                latestInfo = info

                if (!info.groupFormed) {
                    return@requestConnectionInfo
                }

                if (info.isGroupOwner && serverSocket == null) {
                    try {
                        serverSocket = ServerSocket(projectPort)
                    } catch (error: Exception) {
                        listener?.invoke(
                            AndroidWifiDirectDataPathEvent.Failed(
                                reasonCode = null,
                                detail = error.message
                                    ?: "failed to bind Wi-Fi Direct server port",
                            ),
                        )
                        return@requestConnectionInfo
                    }
                }

                listener?.invoke(
                    AndroidWifiDirectDataPathEvent.GroupFormed(
                        AndroidWifiDirectEndpoint(
                            groupOwner = info.isGroupOwner,
                            groupOwnerAddress = info.groupOwnerAddress,
                            port = projectPort,
                        ),
                    ),
                )
            }
        } catch (error: SecurityException) {
            listener?.invoke(
                AndroidWifiDirectDataPathEvent.Failed(
                    reasonCode = null,
                    detail = error.message
                        ?: "requestConnectionInfo permission denied",
                ),
            )
        }
    }

    private fun buildConfig(deviceAddress: String): WifiP2pConfig =
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q) {
            WifiP2pConfig.Builder()
                .setDeviceAddress(MacAddress.fromString(deviceAddress))
                .build()
        } else {
            @Suppress("DEPRECATION")
            WifiP2pConfig().apply {
                this.deviceAddress = deviceAddress
            }
        }

    private fun closeServerSocket() {
        try {
            serverSocket?.close()
        } finally {
            serverSocket = null
        }
    }

    private fun failAndClose(
        reasonCode: Int?,
        detail: String,
    ) {
        val callback = listener
        close()
        callback?.invoke(
            AndroidWifiDirectDataPathEvent.Failed(
                reasonCode = reasonCode,
                detail = detail,
            ),
        )
    }
}

internal fun validateWifiDirectPort(port: Int) {
    require(port in 1024..65535) {
        "Wi-Fi Direct project port must be in 1024..65535"
    }
}
