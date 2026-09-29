package dev.nolane.sanpham3.recoverylab

import android.Manifest
import android.app.Activity
import android.content.ClipData
import android.content.ClipboardManager
import android.content.Context
import android.os.Build
import android.os.Bundle
import android.text.InputType
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.nolane.sanpham3.androidhost.AndroidBleDiscovery
import dev.nolane.sanpham3.androidhost.AndroidBleEvent
import dev.nolane.sanpham3.androidhost.AndroidBleL2capClientDataPath
import dev.nolane.sanpham3.androidhost.AndroidBleL2capServerDataPath
import dev.nolane.sanpham3.androidhost.AndroidBlePeer
import dev.nolane.sanpham3.androidhost.AndroidFeatureScanner
import dev.nolane.sanpham3.androidhost.AndroidG8PairCourt
import dev.nolane.sanpham3.androidhost.AndroidPeerSession
import dev.nolane.sanpham3.androidhost.acceptPeerSession
import dev.nolane.sanpham3.androidhost.connectPeerSession
import java.net.InetSocketAddress
import java.net.ServerSocket
import java.net.Socket
import java.security.SecureRandom
import java.util.concurrent.Executors
import java.util.concurrent.LinkedBlockingQueue
import java.util.concurrent.TimeUnit
import java.util.concurrent.atomic.AtomicBoolean

class MainActivity : Activity() {
    companion object {
        private const val PERMISSION_REQUEST = 1001
        private const val BLE_ACCEPT_TIMEOUT_MS = 60_000
        private const val BLE_SCAN_TIMEOUT_SECONDS = 60L
        private const val TCP_CONNECT_TIMEOUT_MS = 10_000
    }

    private val executor = Executors.newCachedThreadPool()
    private val operationBusy = AtomicBoolean(false)

    private lateinit var nodeIdInput: EditText
    private lateinit var pskInput: EditText
    private lateinit var tcpEndpointInput: EditText
    private lateinit var logView: TextView

    @Volatile
    private var discovery: AndroidBleDiscovery? = null

    @Volatile
    private var l2capServer: AndroidBleL2capServerDataPath? = null

    @Volatile
    private var peerSession: AndroidPeerSession? = null

    @Volatile
    private var tcpServer: ServerSocket? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        title = "sanpham3 Recovery Lab"
        setContentView(buildUi())

        log("Recovery Lab ready.")
        log("This app is for physical G8/G9 evidence, not end-user UX.")
    }

    override fun onDestroy() {
        stopCurrentOperation()
        executor.shutdownNow()
        super.onDestroy()
    }

    private fun buildUi(): ScrollView {
        val scroll = ScrollView(this)
        val column = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(24, 24, 24, 24)
        }
        scroll.addView(
            column,
            ViewGroup.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.WRAP_CONTENT,
            ),
        )

        column.addView(TextView(this).apply {
            text = "sanpham3 Android Recovery Lab"
            textSize = 22f
        })

        column.addView(TextView(this).apply {
            text = "Physical court utility. Use two real Android devices."
        })

        nodeIdInput = EditText(this).apply {
            hint = "Project node ID"
            inputType = InputType.TYPE_CLASS_NUMBER
            setText("100")
        }
        column.addView(nodeIdInput)

        pskInput = EditText(this).apply {
            hint = "64-hex lab PSK"
            inputType =
                InputType.TYPE_CLASS_TEXT or
                    InputType.TYPE_TEXT_VARIATION_PASSWORD
            isSingleLine = true
        }
        column.addView(pskInput)

        tcpEndpointInput = EditText(this).apply {
            hint = "TCP bind/peer endpoint (host:port)"
            setText("0.0.0.0:39080")
            isSingleLine = true
        }
        column.addView(tcpEndpointInput)

        column.addView(button("Generate lab PSK") {
            val bytes = ByteArray(32)
            SecureRandom().nextBytes(bytes)
            pskInput.setText(bytes.toHex())
            bytes.fill(0)
            log("Generated a new lab PSK. Copy the same value to the other device.")
        })

        column.addView(button("Copy PSK") {
            val value = pskInput.text.toString().trim()
            if (value.length != 64) {
                log("PSK is not 64 hex characters.")
                return@button
            }

            val clipboard =
                getSystemService(Context.CLIPBOARD_SERVICE) as ClipboardManager
            clipboard.setPrimaryClip(
                ClipData.newPlainText("sanpham3 lab PSK", value),
            )
            log("Copied lab PSK to clipboard.")
        })

        column.addView(button("Request BLE permissions") {
            requestBlePermissions()
        })

        column.addView(button("Show feature report") {
            try {
                val report = AndroidFeatureScanner(this).scan()
                log("FEATURE_REPORT $report")
            } catch (error: Throwable) {
                logError("feature scan", error)
            }
        })

        column.addView(button("BLE G8 Server") {
            startBleServer()
        })

        column.addView(button("BLE G8 Client") {
            startBleClient()
        })

        column.addView(button("TCP G8 Server") {
            startTcpServer()
        })

        column.addView(button("TCP G8 Client") {
            startTcpClient()
        })

        column.addView(button("Stop current operation") {
            stopCurrentOperation()
            log("Stop requested.")
        })

        column.addView(TextView(this).apply {
            text = "Evidence log"
            textSize = 18f
        })

        logView = TextView(this).apply {
            setTextIsSelectable(true)
            textSize = 13f
        }
        column.addView(logView)

        return scroll
    }

    private fun button(
        label: String,
        action: () -> Unit,
    ): Button = Button(this).apply {
        text = label
        setOnClickListener { action() }
    }

    private fun requestBlePermissions() {
        val permissions = if (Build.VERSION.SDK_INT >= 31) {
            arrayOf(
                Manifest.permission.BLUETOOTH_SCAN,
                Manifest.permission.BLUETOOTH_ADVERTISE,
                Manifest.permission.BLUETOOTH_CONNECT,
            )
        } else {
            arrayOf(Manifest.permission.ACCESS_FINE_LOCATION)
        }

        requestPermissions(permissions, PERMISSION_REQUEST)
    }

    private fun startBleServer() {
        val config = readConfig() ?: return

        runOperation("BLE G8 server") {
            val (nodeId, key) = config
            try {
                val server = AndroidBleL2capServerDataPath(this)
                l2capServer = server

                val endpoint = server.start()
                val discovery = AndroidBleDiscovery(this)
                this.discovery = discovery
                discovery.startAdvertiseOnly(server.discoveryInfo()) { event ->
                    logBleEvent("server-advertise", event)
                }

                log(
                    "BLE_SERVER_LISTEN node=$nodeId psm=${endpoint.psm} " +
                        "timeout_ms=$BLE_ACCEPT_TIMEOUT_MS",
                )

                val session = server.acceptPeerSession(
                    timeoutMillis = BLE_ACCEPT_TIMEOUT_MS,
                    nodeId = nodeId,
                    peerKey = key,
                )
                peerSession = session

                log(
                    "BLE_SERVER_AUTH local_node=$nodeId " +
                        "peer_node=${session.peerNodeId}",
                )

                val evidence = AndroidG8PairCourt.serveOnce(session)
                log(
                    "G8_BLE_PASS role=server local_node=$nodeId " +
                        "peer_node=${evidence.peerNodeId} " +
                        "challenge=${evidence.challenge.toHex()}",
                )
            } finally {
                key.fill(0)
                closeOperationResources()
            }
        }
    }

    private fun startBleClient() {
        val config = readConfig() ?: return

        runOperation("BLE G8 client") {
            val (nodeId, key) = config
            val peers = LinkedBlockingQueue<AndroidBlePeer>()

            try {
                val discovery = AndroidBleDiscovery(this)
                this.discovery = discovery
                discovery.startScanOnly { event ->
                    logBleEvent("client-scan", event)
                    if (event is AndroidBleEvent.PeerDiscovered) {
                        peers.offer(event.peer)
                    }
                }

                val client = AndroidBleL2capClientDataPath(this)
                val deadline = System.nanoTime() +
                    TimeUnit.SECONDS.toNanos(BLE_SCAN_TIMEOUT_SECONDS)

                while (System.nanoTime() < deadline) {
                    val remaining = deadline - System.nanoTime()
                    val peer = peers.poll(
                        remaining.coerceAtLeast(1),
                        TimeUnit.NANOSECONDS,
                    ) ?: break

                    log(
                        "BLE_CLIENT_CANDIDATE rssi=${peer.rssi} " +
                            "address_hint=${peer.addressHint.ifBlank { "-" }} " +
                            "service_bytes=${peer.serviceData.size}",
                    )

                    try {
                        val session = client.connectPeerSession(
                            peer = peer,
                            nodeId = nodeId,
                            peerKey = key,
                        )
                        peerSession = session

                        log(
                            "BLE_CLIENT_AUTH local_node=$nodeId " +
                                "peer_node=${session.peerNodeId}",
                        )

                        val evidence = AndroidG8PairCourt.runClient(session)
                        log(
                            "G8_BLE_PASS role=client local_node=$nodeId " +
                                "peer_node=${evidence.peerNodeId} " +
                                "challenge=${evidence.challenge.toHex()} " +
                                "rssi=${peer.rssi}",
                        )
                        return@runOperation
                    } catch (error: Throwable) {
                        log(
                            "BLE_CLIENT_REJECT candidate=${peer.addressHint.ifBlank { "-" }} " +
                                "detail=${error.message ?: error.javaClass.simpleName}",
                        )
                    }
                }

                error(
                    "No authenticated BLE L2CAP peer completed G8 court " +
                        "within ${BLE_SCAN_TIMEOUT_SECONDS}s",
                )
            } finally {
                key.fill(0)
                closeOperationResources()
            }
        }
    }

    private fun startTcpServer() {
        val config = readConfig() ?: return
        val endpoint = readTcpEndpoint() ?: return

        runOperation("TCP G8 server") {
            val (nodeId, key) = config

            try {
                val server = ServerSocket()
                tcpServer = server
                server.reuseAddress = true
                server.bind(endpoint)

                log(
                    "TCP_SERVER_LISTEN node=$nodeId " +
                        "addr=${server.localSocketAddress}",
                )

                val socket = server.accept()
                log(
                    "TCP_SERVER_ACCEPT remote=${socket.remoteSocketAddress}",
                )

                val session = AndroidPeerSession.server(
                    socket = socket,
                    nodeId = nodeId,
                    peerKey = key,
                )
                peerSession = session

                log(
                    "TCP_SERVER_AUTH local_node=$nodeId " +
                        "peer_node=${session.peerNodeId}",
                )

                val evidence = AndroidG8PairCourt.serveOnce(session)
                log(
                    "G8_TCP_PASS role=server local_node=$nodeId " +
                        "peer_node=${evidence.peerNodeId} " +
                        "challenge=${evidence.challenge.toHex()}",
                )
            } finally {
                key.fill(0)
                closeOperationResources()
            }
        }
    }

    private fun startTcpClient() {
        val config = readConfig() ?: return
        val endpoint = readTcpEndpoint() ?: return

        runOperation("TCP G8 client") {
            val (nodeId, key) = config

            try {
                val socket = Socket()
                socket.connect(endpoint, TCP_CONNECT_TIMEOUT_MS)
                socket.tcpNoDelay = true

                log(
                    "TCP_CLIENT_CONNECTED local_node=$nodeId " +
                        "remote=${socket.remoteSocketAddress}",
                )

                val session = AndroidPeerSession.client(
                    socket = socket,
                    nodeId = nodeId,
                    peerKey = key,
                )
                peerSession = session

                log(
                    "TCP_CLIENT_AUTH local_node=$nodeId " +
                        "peer_node=${session.peerNodeId}",
                )

                val evidence = AndroidG8PairCourt.runClient(session)
                log(
                    "G8_TCP_PASS role=client local_node=$nodeId " +
                        "peer_node=${evidence.peerNodeId} " +
                        "challenge=${evidence.challenge.toHex()}",
                )
            } finally {
                key.fill(0)
                closeOperationResources()
            }
        }
    }

    private fun readTcpEndpoint(): InetSocketAddress? {
        val text = tcpEndpointInput.text.toString().trim()
        val split = text.lastIndexOf(':')

        if (split <= 0 || split == text.lastIndex) {
            log("TCP endpoint must be host:port.")
            return null
        }

        val host = text.substring(0, split).trim()
        val port = text.substring(split + 1).toIntOrNull()

        if (host.isEmpty() || port == null || port !in 1..65535) {
            log("TCP endpoint must contain a valid host and port.")
            return null
        }

        return InetSocketAddress(host, port)
    }

    private fun runOperation(
        name: String,
        operation: () -> Unit,
    ) {
        if (!operationBusy.compareAndSet(false, true)) {
            log("Another operation is already active.")
            return
        }

        log("START $name")

        executor.execute {
            try {
                operation()
                log("DONE $name")
            } catch (error: Throwable) {
                logError(name, error)
            } finally {
                operationBusy.set(false)
            }
        }
    }

    private fun readConfig(): Pair<Long, ByteArray>? {
        val nodeId = nodeIdInput.text
            .toString()
            .trim()
            .toLongOrNull()

        if (nodeId == null || nodeId < 0) {
            log("Node ID must be a non-negative integer.")
            return null
        }

        val key = try {
            decodeHexKey(pskInput.text.toString().trim())
        } catch (error: IllegalArgumentException) {
            log(error.message ?: "Invalid PSK.")
            return null
        }

        return nodeId to key
    }

    private fun stopCurrentOperation() {
        closeOperationResources()
        operationBusy.set(false)
    }

    private fun closeOperationResources() {
        val session = peerSession
        peerSession = null
        try {
            session?.close()
        } catch (_: Throwable) {
        }

        val discovery = discovery
        this.discovery = null
        try {
            discovery?.close()
        } catch (_: Throwable) {
        }

        val server = l2capServer
        l2capServer = null
        try {
            server?.close()
        } catch (_: Throwable) {
        }

        val tcp = tcpServer
        tcpServer = null
        try {
            tcp?.close()
        } catch (_: Throwable) {
        }
    }

    private fun logBleEvent(
        source: String,
        event: AndroidBleEvent,
    ) {
        when (event) {
            AndroidBleEvent.Started ->
                log("BLE_EVENT source=$source started")

            is AndroidBleEvent.PeerDiscovered ->
                log(
                    "BLE_EVENT source=$source peer " +
                        "rssi=${event.peer.rssi} " +
                        "service_bytes=${event.peer.serviceData.size}",
                )

            is AndroidBleEvent.Failed ->
                log(
                    "BLE_EVENT source=$source failed " +
                        "code=${event.code} detail=${event.detail}",
                )
        }
    }

    private fun logError(
        scope: String,
        error: Throwable,
    ) {
        log(
            "ERROR scope=$scope type=${error.javaClass.simpleName} " +
                "detail=${error.message ?: "-"}",
        )
    }

    private fun log(message: String) {
        runOnUiThread {
            val timestamp = System.currentTimeMillis()
            logView.append("$timestamp $message\n")
        }
    }

    private fun decodeHexKey(value: String): ByteArray {
        require(value.length == 64) {
            "PSK must be exactly 64 hexadecimal characters."
        }

        val out = ByteArray(32)
        for (index in out.indices) {
            val high = value[index * 2].digitToIntOrNull(16)
                ?: throw IllegalArgumentException(
                    "PSK contains a non-hex character.",
                )
            val low = value[index * 2 + 1].digitToIntOrNull(16)
                ?: throw IllegalArgumentException(
                    "PSK contains a non-hex character.",
                )
            out[index] = ((high shl 4) or low).toByte()
        }

        return out
    }

    private fun ByteArray.toHex(): String {
        val digits = "0123456789abcdef"
        val out = StringBuilder(size * 2)

        for (byte in this) {
            val value = byte.toInt() and 0xff
            out.append(digits[value ushr 4])
            out.append(digits[value and 0x0f])
        }

        return out.toString()
    }
}
