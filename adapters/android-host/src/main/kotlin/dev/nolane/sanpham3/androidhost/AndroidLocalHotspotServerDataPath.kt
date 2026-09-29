package dev.nolane.sanpham3.androidhost

import android.content.Context
import android.net.wifi.SoftApConfiguration
import android.net.wifi.WifiManager
import android.os.Build
import java.io.Closeable
import java.net.ServerSocket
import java.net.Socket
import java.util.concurrent.atomic.AtomicBoolean

enum class AndroidLocalHotspotSecurity {
    OPEN,
    WPA2_PSK,
    WPA3_SAE,
}

data class AndroidLocalHotspotEndpoint(
    val ssid: String,
    val passphrase: String?,
    val security: AndroidLocalHotspotSecurity,
    val port: Int,
)

sealed interface AndroidLocalHotspotServerEvent {
    data class Ready(
        val endpoint: AndroidLocalHotspotEndpoint,
    ) : AndroidLocalHotspotServerEvent

    data class Failed(
        val reasonCode: Int?,
        val detail: String,
    ) : AndroidLocalHotspotServerEvent
}

class AndroidLocalHotspotServerDataPath(
    context: Context,
) : Closeable {
    private val wifiManager =
        context.applicationContext.getSystemService(WifiManager::class.java)
    private val started = AtomicBoolean(false)
    private var reservation: WifiManager.LocalOnlyHotspotReservation? = null
    private var serverSocket: ServerSocket? = null

    @Volatile
    private var endpoint: AndroidLocalHotspotEndpoint? = null

    fun start(
        port: Int,
        onEvent: (AndroidLocalHotspotServerEvent) -> Unit,
    ) {
        validateLocalHotspotPort(port)
        require(Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            "Local-Only Hotspot requires Android 8.0+"
        }
        if (!started.compareAndSet(false, true)) return

        val listenerSocket = try {
            ServerSocket(port)
        } catch (error: Throwable) {
            started.set(false)
            throw error
        }
        serverSocket = listenerSocket

        val callback = object : WifiManager.LocalOnlyHotspotCallback() {
            override fun onStarted(
                value: WifiManager.LocalOnlyHotspotReservation,
            ) {
                reservation = value
                try {
                    val ready = value.toEndpoint(port)
                    endpoint = ready
                    onEvent(AndroidLocalHotspotServerEvent.Ready(ready))
                } catch (error: Throwable) {
                    failAndClose(
                        null,
                        error.message ?: error.javaClass.simpleName,
                        onEvent,
                    )
                }
            }

            override fun onStopped() {
                releaseAfterSystemStop()
            }

            override fun onFailed(reason: Int) {
                failAndClose(
                    reason,
                    "Local-Only Hotspot start failed",
                    onEvent,
                )
            }
        }

        try {
            wifiManager.startLocalOnlyHotspot(callback, null)
        } catch (error: Throwable) {
            failAndClose(
                null,
                error.message ?: error.javaClass.simpleName,
                onEvent,
            )
            throw error
        }
    }

    fun currentEndpoint(): AndroidLocalHotspotEndpoint? = endpoint

    fun accept(timeoutMillis: Int): Socket {
        require(timeoutMillis > 0)
        checkNotNull(endpoint) {
            "Local-Only Hotspot is not ready"
        }
        val socket = checkNotNull(serverSocket) {
            "Local-Only Hotspot server socket unavailable"
        }
        socket.soTimeout = timeoutMillis
        return socket.accept()
    }

    override fun close() {
        if (!started.compareAndSet(true, false)) return
        endpoint = null
        try {
            reservation?.close()
        } finally {
            reservation = null
            try {
                serverSocket?.close()
            } finally {
                serverSocket = null
            }
        }
    }

    private fun releaseAfterSystemStop() {
        if (!started.compareAndSet(true, false)) return
        endpoint = null
        reservation = null
        try {
            serverSocket?.close()
        } finally {
            serverSocket = null
        }
    }

    private fun failAndClose(
        reasonCode: Int?,
        detail: String,
        onEvent: (AndroidLocalHotspotServerEvent) -> Unit,
    ) {
        close()
        onEvent(
            AndroidLocalHotspotServerEvent.Failed(
                reasonCode = reasonCode,
                detail = detail,
            ),
        )
    }
}

private fun WifiManager.LocalOnlyHotspotReservation.toEndpoint(
    port: Int,
): AndroidLocalHotspotEndpoint =
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.R) {
        val config = softApConfiguration
        @Suppress("DEPRECATION")
        val ssid = requireNotNull(config.ssid) {
            "Local-Only Hotspot SSID unavailable"
        }
        val passphrase = config.passphrase
        val security = when (config.securityType) {
            SoftApConfiguration.SECURITY_TYPE_OPEN ->
                AndroidLocalHotspotSecurity.OPEN

            SoftApConfiguration.SECURITY_TYPE_WPA3_OWE,
            SoftApConfiguration.SECURITY_TYPE_WPA3_OWE_TRANSITION ->
                error(
                    "OWE Local-Only Hotspot bootstrap is not implemented yet",
                )

            SoftApConfiguration.SECURITY_TYPE_WPA2_PSK,
            SoftApConfiguration.SECURITY_TYPE_WPA3_SAE_TRANSITION ->
                AndroidLocalHotspotSecurity.WPA2_PSK

            SoftApConfiguration.SECURITY_TYPE_WPA3_SAE ->
                AndroidLocalHotspotSecurity.WPA3_SAE

            else -> error(
                "unsupported Local-Only Hotspot security type ${config.securityType}",
            )
        }

        AndroidLocalHotspotEndpoint(
            ssid = ssid,
            passphrase = passphrase,
            security = security,
            port = port,
        )
    } else {
        @Suppress("DEPRECATION")
        val config = requireNotNull(wifiConfiguration) {
            "legacy Local-Only Hotspot configuration unavailable"
        }
        val ssid = stripWifiQuotes(config.SSID)
        val passphrase = config.preSharedKey
            ?.let(::stripWifiQuotes)
            ?.takeIf(String::isNotBlank)

        AndroidLocalHotspotEndpoint(
            ssid = ssid,
            passphrase = passphrase,
            security = if (passphrase == null) {
                AndroidLocalHotspotSecurity.OPEN
            } else {
                AndroidLocalHotspotSecurity.WPA2_PSK
            },
            port = port,
        )
    }

internal fun stripWifiQuotes(value: String?): String {
    val source = requireNotNull(value) {
        "Wi-Fi value unavailable"
    }
    return if (source.length >= 2 &&
        source.first() == '"' && source.last() == '"'
    ) {
        source.substring(1, source.lastIndex)
    } else {
        source
    }
}

internal fun validateLocalHotspotPort(port: Int) {
    require(port in 1024..65535) {
        "Local-Only Hotspot project port must be in 1024..65535"
    }
}