package dev.nolane.sanpham3.androidhost

import android.content.Context
import android.net.wifi.aware.AttachCallback
import android.net.wifi.aware.DiscoverySessionCallback
import android.net.wifi.aware.PeerHandle
import android.net.wifi.aware.PublishConfig
import android.net.wifi.aware.PublishDiscoverySession
import android.net.wifi.aware.SubscribeConfig
import android.net.wifi.aware.SubscribeDiscoverySession
import android.net.wifi.aware.WifiAwareManager
import android.net.wifi.aware.WifiAwareSession
import android.os.Build
import android.os.Handler
import java.io.Closeable
import java.util.concurrent.atomic.AtomicBoolean

data class AndroidWifiAwarePeer(
    val peerHandle: PeerHandle,
    val serviceSpecificInfo: ByteArray,
)

sealed interface AndroidWifiAwareEvent {
    data object Attached : AndroidWifiAwareEvent
    data object Advertising : AndroidWifiAwareEvent
    data object Discovering : AndroidWifiAwareEvent

    data class PeerDiscovered(
        val peer: AndroidWifiAwarePeer,
    ) : AndroidWifiAwareEvent

    data class Failed(
        val detail: String,
    ) : AndroidWifiAwareEvent
}

/**
 * Publishes and subscribes to a tiny project-local Wi-Fi Aware service.
 *
 * The service-specific info is an untrusted discovery hint only. Actual relay
 * authorization still belongs to the authenticated peer-session protocol.
 */
class AndroidWifiAwareDiscovery(
    context: Context,
) : Closeable {
    companion object {
        private const val SERVICE_NAME = "sp3-connectivity-v0"
        private const val MAX_DISCOVERY_INFO_BYTES = 64
    }

    private val appContext = context.applicationContext
    private val manager = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
        appContext.getSystemService(WifiAwareManager::class.java)
    } else {
        null
    }
    private val handler = Handler(appContext.mainLooper)
    private val started = AtomicBoolean(false)

    private var awareSession: WifiAwareSession? = null
    private var publishSession: PublishDiscoverySession? = null
    private var subscribeSession: SubscribeDiscoverySession? = null
    private var listener: ((AndroidWifiAwareEvent) -> Unit)? = null

    fun start(
        discoveryInfo: ByteArray,
        onEvent: (AndroidWifiAwareEvent) -> Unit,
    ) {
        require(discoveryInfo.size <= MAX_DISCOVERY_INFO_BYTES) {
            "discoveryInfo must be <= $MAX_DISCOVERY_INFO_BYTES bytes"
        }

        if (!started.compareAndSet(false, true)) {
            return
        }

        listener = onEvent

        val wifiAware = manager
        if (wifiAware == null) {
            failAndClose("Wi-Fi Aware unavailable on this Android version")
            return
        }
        if (!wifiAware.isAvailable) {
            failAndClose("Wi-Fi Aware hardware/service currently unavailable")
            return
        }

        try {
            wifiAware.attach(
                object : AttachCallback() {
                    override fun onAttached(session: WifiAwareSession) {
                        awareSession = session
                        listener?.invoke(AndroidWifiAwareEvent.Attached)
                        startPublish(session, discoveryInfo)
                        startSubscribe(session)
                    }

                    override fun onAttachFailed() {
                        failAndClose("Wi-Fi Aware attach failed")
                    }
                },
                handler,
            )
        } catch (error: SecurityException) {
            failAndClose(
                error.message ?: "Wi-Fi Aware permission denied",
            )
        } catch (error: RuntimeException) {
            failAndClose(error.message ?: error.javaClass.simpleName)
        }
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) {
            return
        }

        publishSession?.close()
        publishSession = null
        subscribeSession?.close()
        subscribeSession = null
        awareSession?.close()
        awareSession = null
        listener = null
    }

    private fun startPublish(
        session: WifiAwareSession,
        discoveryInfo: ByteArray,
    ) {
        val config = PublishConfig.Builder()
            .setServiceName(SERVICE_NAME)
            .setServiceSpecificInfo(discoveryInfo)
            .build()

        session.publish(
            config,
            object : DiscoverySessionCallback() {
                override fun onPublishStarted(
                    session: PublishDiscoverySession,
                ) {
                    publishSession = session
                    listener?.invoke(AndroidWifiAwareEvent.Advertising)
                }

                override fun onSessionConfigFailed() {
                    listener?.invoke(
                        AndroidWifiAwareEvent.Failed(
                            "Wi-Fi Aware publish configuration failed",
                        ),
                    )
                }

                override fun onSessionTerminated() {
                    publishSession = null
                }
            },
            handler,
        )
    }

    private fun startSubscribe(session: WifiAwareSession) {
        val config = SubscribeConfig.Builder()
            .setServiceName(SERVICE_NAME)
            .build()

        session.subscribe(
            config,
            object : DiscoverySessionCallback() {
                override fun onSubscribeStarted(
                    session: SubscribeDiscoverySession,
                ) {
                    subscribeSession = session
                    listener?.invoke(AndroidWifiAwareEvent.Discovering)
                }

                override fun onServiceDiscovered(
                    peerHandle: PeerHandle,
                    serviceSpecificInfo: ByteArray,
                    matchFilter: List<ByteArray>,
                ) {
                    listener?.invoke(
                        AndroidWifiAwareEvent.PeerDiscovered(
                            AndroidWifiAwarePeer(
                                peerHandle = peerHandle,
                                serviceSpecificInfo =
                                    serviceSpecificInfo.copyOf(),
                            ),
                        ),
                    )
                }

                override fun onSessionConfigFailed() {
                    listener?.invoke(
                        AndroidWifiAwareEvent.Failed(
                            "Wi-Fi Aware subscribe configuration failed",
                        ),
                    )
                }

                override fun onSessionTerminated() {
                    subscribeSession = null
                }
            },
            handler,
        )
    }

    private fun failAndClose(detail: String) {
        val callback = listener
        close()
        callback?.invoke(AndroidWifiAwareEvent.Failed(detail))
    }
}
