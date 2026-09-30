package dev.nolane.sanpham3.recoverylab

import android.Manifest
import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.text.InputType
import android.text.method.PasswordTransformationMethod
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.nolane.sanpham3.androidhost.AndroidBluetoothClassicDiscovery
import dev.nolane.sanpham3.androidhost.AndroidBluetoothClassicEvent
import dev.nolane.sanpham3.androidhost.AndroidBluetoothClassicPeer
import dev.nolane.sanpham3.androidhost.AndroidBluetoothRfcommClientDataPath
import dev.nolane.sanpham3.androidhost.AndroidBluetoothRfcommProtocol
import dev.nolane.sanpham3.androidhost.AndroidBluetoothRfcommServerDataPath
import dev.nolane.sanpham3.androidhost.AndroidG8PairCourt
import dev.nolane.sanpham3.androidhost.AndroidPeerBenchmarkConfig
import dev.nolane.sanpham3.androidhost.AndroidPeerSession
import dev.nolane.sanpham3.androidhost.AndroidPeerSessionBenchmark
import dev.nolane.sanpham3.androidhost.acceptPeerSession
import dev.nolane.sanpham3.androidhost.connectPeerSession
import java.io.File
import java.security.SecureRandom
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors

class RfcommCourtActivity : Activity() {
    companion object {
        private const val permissionRequestCode = 7401
        private const val acceptTimeoutMillis = 120_000
        private val benchmarkConfig = AndroidPeerBenchmarkConfig(
            rounds = 32,
            payloadBytes = 1024,
        )
        private val evidenceStamp = DateTimeFormatter
            .ofPattern("yyyyMMdd'T'HHmmss'Z'")
            .withZone(ZoneOffset.UTC)
    }

    private data class LabConfig(
        val nodeId: Long,
        val peerKey: ByteArray,
    )

    private val worker = Executors.newSingleThreadExecutor()
    private val peers = ConcurrentHashMap<String, AndroidBluetoothClassicPeer>()
    private val transcript = StringBuilder()
    private val stateLock = Any()

    private lateinit var nodeIdInput: EditText
    private lateinit var pskInput: EditText
    private lateinit var targetAddressInput: EditText
    private lateinit var logView: TextView
    private lateinit var logScroll: ScrollView
    private var pskVisible = false

    @Volatile
    private var discovery: AndroidBluetoothClassicDiscovery? = null

    @Volatile
    private var server: AndroidBluetoothRfcommServerDataPath? = null

    @Volatile
    private var peerSession: AndroidPeerSession? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildUi())

        appendLog("RFCOMM_LAB_START git=${BuildConfig.GIT_SHA}")
        appendLog(
            "DEVICE manufacturer=${Build.MANUFACTURER} " +
                "model=${Build.MODEL} sdk=${Build.VERSION.SDK_INT}",
        )
        appendLog(
            "service_uuid=${AndroidBluetoothRfcommProtocol.SERVICE_UUID}",
        )
    }

    override fun onDestroy() {
        stopActive("activity_destroyed")
        worker.shutdownNow()
        super.onDestroy()
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray,
    ) {
        super.onRequestPermissionsResult(
            requestCode,
            permissions,
            grantResults,
        )
        if (requestCode == permissionRequestCode) {
            appendLog(
                "RFCOMM_PERMISSION granted=" +
                    (grantResults.isNotEmpty() &&
                        grantResults.all {
                            it == PackageManager.PERMISSION_GRANTED
                        }),
            )
        }
    }

    private fun buildUi(): LinearLayout {
        val root = LinearLayout(this).apply {
            orientation = LinearLayout.VERTICAL
            setPadding(24, 24, 24, 24)
            layoutParams = LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                ViewGroup.LayoutParams.MATCH_PARENT,
            )
        }

        root.addView(TextView(this).apply {
            text = "SP3 RFCOMM Physical G8 Court"
            textSize = 22f
        })
        root.addView(TextView(this).apply {
            text =
                "Bluetooth Classic discovery/bond → RFCOMM → " +
                    "Rust peer-session → encrypted G8 → RTT/goodput benchmark"
        })

        nodeIdInput = EditText(this).apply {
            hint = "Project node ID"
            setText("100")
            inputType = InputType.TYPE_CLASS_NUMBER
            isSingleLine = true
        }
        root.addView(nodeIdInput)

        pskInput = EditText(this).apply {
            hint = "64-hex laboratory peer PSK"
            inputType =
                InputType.TYPE_CLASS_TEXT or
                    InputType.TYPE_TEXT_VARIATION_PASSWORD
            transformationMethod =
                PasswordTransformationMethod.getInstance()
            isSingleLine = true
        }
        root.addView(pskInput)

        targetAddressInput = EditText(this).apply {
            hint = "Target Bluetooth address from scan/bonded list"
            inputType = InputType.TYPE_CLASS_TEXT
            isSingleLine = true
        }
        root.addView(targetAddressInput)

        root.addView(button("Generate laboratory PSK") {
            generateLabPsk()
        })
        root.addView(button("Show / hide PSK") {
            togglePskVisibility()
        })
        root.addView(button("Grant RFCOMM permissions") {
            requestRfcommPermissions()
        })
        root.addView(button("Request 120s discoverable") {
            requestDiscoverable()
        })
        root.addView(button("Start RFCOMM G8 server") {
            startRfcommServer()
        })
        root.addView(button("Scan Classic peers") {
            scanPeers()
        })
        root.addView(button("Connect target + run G8 benchmark") {
            connectTarget()
        })
        root.addView(button("Stop active court") {
            stopActive("user_stop")
        })
        root.addView(button("Back") {
            finish()
        })

        logView = TextView(this).apply {
            textSize = 12f
            setTextIsSelectable(true)
        }
        logScroll = ScrollView(this).apply {
            addView(
                logView,
                ViewGroup.LayoutParams(
                    ViewGroup.LayoutParams.MATCH_PARENT,
                    ViewGroup.LayoutParams.WRAP_CONTENT,
                ),
            )
        }
        root.addView(
            logScroll,
            LinearLayout.LayoutParams(
                ViewGroup.LayoutParams.MATCH_PARENT,
                0,
                1f,
            ),
        )
        return root
    }

    private fun button(
        label: String,
        action: () -> Unit,
    ): Button =
        Button(this).apply {
            text = label
            setOnClickListener { action() }
        }

    private fun requiredPermissions(): List<String> =
        if (Build.VERSION.SDK_INT >= 31) {
            listOf(
                Manifest.permission.BLUETOOTH_SCAN,
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.BLUETOOTH_ADVERTISE,
            )
        } else {
            listOf(Manifest.permission.ACCESS_FINE_LOCATION)
        }

    private fun permissionsReady(): Boolean =
        requiredPermissions().all {
            checkSelfPermission(it) ==
                PackageManager.PERMISSION_GRANTED
        }

    private fun requestRfcommPermissions() {
        val missing = requiredPermissions().filter {
            checkSelfPermission(it) !=
                PackageManager.PERMISSION_GRANTED
        }
        if (missing.isEmpty()) {
            appendLog("RFCOMM_PERMISSION already_granted=true")
            return
        }
        requestPermissions(
            missing.toTypedArray(),
            permissionRequestCode,
        )
    }

    private fun requestDiscoverable() {
        if (!permissionsReady()) {
            requestRfcommPermissions()
            return
        }

        runCatching {
            val intent =
                AndroidBluetoothClassicDiscovery
                    .requestDiscoverableIntent(120)
            startActivity(intent)
            appendLog("RFCOMM_DISCOVERABLE_REQUEST seconds=120")
        }.onFailure { error ->
            appendLog(
                "RFCOMM_DISCOVERABLE_FAIL " +
                    "error=${error.javaClass.simpleName}:${sanitize(error.message)}",
            )
        }
    }

    private fun readConfig(): LabConfig? {
        if (!permissionsReady()) {
            appendLog("ERROR RFCOMM permissions are not ready")
            requestRfcommPermissions()
            return null
        }

        return try {
            val nodeId =
                nodeIdInput.text.toString().trim().toLong()
            require(nodeId >= 0) {
                "node ID must be non-negative"
            }
            val key =
                LabCodec.parsePeerKey(pskInput.text.toString())
            LabConfig(nodeId, key)
        } catch (error: Exception) {
            appendLog("ERROR config=${sanitize(error.message)}")
            null
        }
    }

    private fun generateLabPsk() {
        val bytes = ByteArray(32)
        SecureRandom().nextBytes(bytes)
        val encoded = LabCodec.hex(bytes)
        bytes.fill(0)
        pskInput.setText(encoded)
        pskInput.setSelection(encoded.length)
        appendLog("PSK_GENERATED bytes=32 stored_in_evidence=false")
    }

    private fun togglePskVisibility() {
        pskVisible = !pskVisible
        pskInput.transformationMethod = if (pskVisible) {
            null
        } else {
            PasswordTransformationMethod.getInstance()
        }
        pskInput.setSelection(pskInput.text.length)
        appendLog("PSK_VISIBILITY visible=$pskVisible")
    }

    private fun startRfcommServer() {
        val config = readConfig() ?: return
        stopActive("replace_with_rfcomm_server")

        val localServer =
            AndroidBluetoothRfcommServerDataPath(this)
        try {
            localServer.start()
        } catch (error: Throwable) {
            config.peerKey.fill(0)
            appendLog(
                "RFCOMM_FAIL role=server stage=start " +
                    "error=${error.javaClass.simpleName}:${sanitize(error.message)}",
            )
            saveEvidence(
                "g8-rfcomm-server-start-fail",
                config.nodeId,
                listOf(
                    "role=server",
                    "carrier=rfcomm",
                    "result=FAIL",
                    "stage=start",
                    "error=${error.javaClass.simpleName}",
                ),
            )
            return
        }

        synchronized(stateLock) {
            server = localServer
        }
        appendLog(
            "RFCOMM_SERVER_LISTEN node=${config.nodeId} " +
                "uuid=${AndroidBluetoothRfcommProtocol.SERVICE_UUID}",
        )

        worker.execute {
            var session: AndroidPeerSession? = null
            val acceptAt = System.nanoTime()
            try {
                session = localServer.acceptPeerSession(
                    timeoutMillis = acceptTimeoutMillis,
                    nodeId = config.nodeId,
                    peerKey = config.peerKey,
                )
                config.peerKey.fill(0)
                peerSession = session
                require(session.peerNodeId != config.nodeId) {
                    "peer node ID must differ from local node ID"
                }

                val g8 = AndroidG8PairCourt.serveOnce(session)
                val g8WaitMs =
                    (System.nanoTime() - acceptAt) / 1_000_000
                val benchmarkAt = System.nanoTime()
                AndroidPeerSessionBenchmark.serve(
                    session,
                    benchmarkConfig,
                )
                val benchmarkServeMs =
                    (System.nanoTime() - benchmarkAt) / 1_000_000
                val challenge = LabCodec.hex(g8.challenge)

                appendLog(
                    "RFCOMM_G8_PASS role=server " +
                        "local_node=${config.nodeId} " +
                        "peer_node=${g8.peerNodeId} " +
                        "g8_wait_ms=$g8WaitMs " +
                        "benchmark_serve_ms=$benchmarkServeMs",
                )
                saveEvidence(
                    "g8-rfcomm-server-pass",
                    config.nodeId,
                    listOf(
                        "role=server",
                        "carrier=rfcomm",
                        "service_uuid=${AndroidBluetoothRfcommProtocol.SERVICE_UUID}",
                        "authenticated_peer_node=${g8.peerNodeId}",
                        "challenge_hex=$challenge",
                        "g8_wait_ms=$g8WaitMs",
                        "benchmark_rounds=${benchmarkConfig.rounds}",
                        "benchmark_payload_bytes=${benchmarkConfig.payloadBytes}",
                        "benchmark_serve_ms=$benchmarkServeMs",
                        "result=PASS",
                    ),
                )
            } catch (error: Throwable) {
                config.peerKey.fill(0)
                appendLog(
                    "RFCOMM_G8_FAIL role=server " +
                        "error=${error.javaClass.simpleName}:${sanitize(error.message)}",
                )
                saveEvidence(
                    "g8-rfcomm-server-fail",
                    config.nodeId,
                    listOf(
                        "role=server",
                        "carrier=rfcomm",
                        "result=FAIL",
                        "error=${error.javaClass.simpleName}",
                    ),
                )
            } finally {
                session?.close()
                localServer.close()
                synchronized(stateLock) {
                    if (peerSession === session) {
                        peerSession = null
                    }
                    if (server === localServer) {
                        server = null
                    }
                }
            }
        }
    }

    private fun scanPeers() {
        if (!permissionsReady()) {
            requestRfcommPermissions()
            return
        }

        discovery?.close()
        peers.clear()

        val scanner = AndroidBluetoothClassicDiscovery(this)
        discovery = scanner

        runCatching {
            val bonded = scanner.bondedPeers()
            for (peer in bonded) {
                rememberPeer(peer, "bonded")
            }

            scanner.start { event ->
                when (event) {
                    AndroidBluetoothClassicEvent.Started -> {
                        appendLog(
                            "RFCOMM_DISCOVERY started=true " +
                                "bonded_count=${bonded.size}",
                        )
                    }

                    is AndroidBluetoothClassicEvent.PeerDiscovered -> {
                        rememberPeer(event.peer, "scan")
                    }

                    AndroidBluetoothClassicEvent.Finished -> {
                        appendLog(
                            "RFCOMM_DISCOVERY finished=true " +
                                "peer_count=${peers.size}",
                        )
                    }

                    is AndroidBluetoothClassicEvent.Failed -> {
                        appendLog(
                            "RFCOMM_DISCOVERY_FAIL " +
                                "detail=${sanitize(event.detail)}",
                        )
                    }
                }
            }
        }.onFailure { error ->
            scanner.close()
            discovery = null
            appendLog(
                "RFCOMM_DISCOVERY_FAIL " +
                    "error=${error.javaClass.simpleName}:${sanitize(error.message)}",
            )
        }
    }

    private fun rememberPeer(
        peer: AndroidBluetoothClassicPeer,
        source: String,
    ) {
        if (peer.addressHint.isBlank()) {
            return
        }
        peers[peer.addressHint.uppercase()] = peer
        appendLog(
            "RFCOMM_PEER source=$source " +
                "address=${peer.addressHint} " +
                "name=${sanitize(peer.nameHint)} " +
                "rssi=${peer.rssi ?: -1}",
        )
    }

    private fun connectTarget() {
        val config = readConfig() ?: return
        val target =
            targetAddressInput.text.toString().trim().uppercase()
        if (target.isBlank()) {
            config.peerKey.fill(0)
            appendLog("ERROR target Bluetooth address is empty")
            return
        }

        val scanner =
            discovery ?: AndroidBluetoothClassicDiscovery(this)
        val peer = peers[target]
            ?: runCatching {
                scanner.bondedPeers().firstOrNull {
                    it.addressHint.equals(target, ignoreCase = true)
                }
            }.getOrNull()

        if (peer == null) {
            config.peerKey.fill(0)
            appendLog(
                "RFCOMM_FAIL role=client stage=select_peer " +
                    "target=${sanitize(target)}",
            )
            return
        }

        discovery?.close()
        discovery = null

        worker.execute {
            var session: AndroidPeerSession? = null
            val connectAt = System.nanoTime()
            try {
                val client =
                    AndroidBluetoothRfcommClientDataPath(this)
                session = client.connectPeerSession(
                    peer = peer,
                    nodeId = config.nodeId,
                    peerKey = config.peerKey,
                )
                config.peerKey.fill(0)
                peerSession = session
                require(session.peerNodeId != config.nodeId) {
                    "peer node ID must differ from local node ID"
                }

                val g8 = AndroidG8PairCourt.runClient(session)
                val g8Ms =
                    (System.nanoTime() - connectAt) / 1_000_000
                val benchmark =
                    AndroidPeerSessionBenchmark.runClient(
                        session,
                        benchmarkConfig,
                    )
                val challenge = LabCodec.hex(g8.challenge)

                appendLog(
                    "RFCOMM_G8_PASS role=client " +
                        "local_node=${config.nodeId} " +
                        "peer_node=${g8.peerNodeId} " +
                        "g8_ms=$g8Ms " +
                        "rtt_p95_ns=${benchmark.p95RttNanos} " +
                        "one_way_useful_bps=${benchmark.oneWayUsefulBitsPerSecond}",
                )
                saveEvidence(
                    "g8-rfcomm-client-pass",
                    config.nodeId,
                    listOf(
                        "role=client",
                        "carrier=rfcomm",
                        "target_address_hint=${peer.addressHint}",
                        "target_name_hint=${sanitize(peer.nameHint)}",
                        "rssi=${peer.rssi ?: -1}",
                        "authenticated_peer_node=${g8.peerNodeId}",
                        "challenge_hex=$challenge",
                        "g8_ms=$g8Ms",
                        "benchmark_rounds=${benchmark.rounds}",
                        "benchmark_payload_bytes=${benchmark.payloadBytes}",
                        "benchmark_elapsed_ns=${benchmark.elapsedNanos}",
                        "benchmark_rtt_min_ns=${benchmark.minRttNanos}",
                        "benchmark_rtt_median_ns=${benchmark.medianRttNanos}",
                        "benchmark_rtt_p95_ns=${benchmark.p95RttNanos}",
                        "benchmark_rtt_max_ns=${benchmark.maxRttNanos}",
                        "benchmark_one_way_useful_bps=${benchmark.oneWayUsefulBitsPerSecond}",
                        "benchmark_round_trip_useful_bps=${benchmark.roundTripUsefulBitsPerSecond}",
                        "result=PASS",
                    ),
                )
            } catch (error: Throwable) {
                config.peerKey.fill(0)
                appendLog(
                    "RFCOMM_G8_FAIL role=client " +
                        "error=${error.javaClass.simpleName}:${sanitize(error.message)}",
                )
                saveEvidence(
                    "g8-rfcomm-client-fail",
                    config.nodeId,
                    listOf(
                        "role=client",
                        "carrier=rfcomm",
                        "target_address_hint=${peer.addressHint}",
                        "result=FAIL",
                        "error=${error.javaClass.simpleName}",
                    ),
                )
            } finally {
                session?.close()
                synchronized(stateLock) {
                    if (peerSession === session) {
                        peerSession = null
                    }
                }
            }
        }
    }

    private fun stopActive(reason: String) {
        val oldSession: AndroidPeerSession?
        val oldServer: AndroidBluetoothRfcommServerDataPath?
        val oldDiscovery: AndroidBluetoothClassicDiscovery?

        synchronized(stateLock) {
            oldSession = peerSession
            oldServer = server
            oldDiscovery = discovery
            peerSession = null
            server = null
            discovery = null
        }

        runCatching { oldSession?.close() }
        runCatching { oldServer?.close() }
        runCatching { oldDiscovery?.close() }
        appendLog("RFCOMM_STOP reason=$reason")
    }

    private fun appendLog(message: String) {
        val line = "${Instant.now()} $message"
        val snapshot = synchronized(transcript) {
            transcript.append(line).append('\n')
            transcript.toString()
        }
        runOnUiThread {
            logView.text = snapshot
            logScroll.post {
                logScroll.fullScroll(ScrollView.FOCUS_DOWN)
            }
        }
    }

    private fun saveEvidence(
        label: String,
        nodeId: Long,
        fields: List<String>,
    ) {
        runCatching {
            val base = getExternalFilesDir(null) ?: filesDir
            val directory = File(base, "evidence")
            check(directory.exists() || directory.mkdirs())

            val file = File(
                directory,
                "${evidenceStamp.format(Instant.now())}-$label.txt",
            )
            val snapshot = synchronized(transcript) {
                transcript.toString()
            }
            file.writeText(
                buildString {
                    appendLine("timestamp_utc=${Instant.now()}")
                    appendLine("git_commit=${BuildConfig.GIT_SHA}")
                    appendLine(
                        "device=${Build.MANUFACTURER} ${Build.MODEL}",
                    )
                    appendLine("android_sdk=${Build.VERSION.SDK_INT}")
                    appendLine("local_node_id=$nodeId")
                    for (field in fields) {
                        appendLine(field)
                    }
                    appendLine()
                    appendLine("--- transcript ---")
                    append(snapshot)
                },
            )
            appendLog(
                "EVIDENCE_SAVED path=${file.absolutePath}",
            )
        }.onFailure { error ->
            appendLog(
                "EVIDENCE_FAIL error=${error.javaClass.simpleName}:" +
                    sanitize(error.message),
            )
        }
    }

    private fun sanitize(value: String?): String =
        value
            ?.replace('\n', ' ')
            ?.replace('\r', ' ')
            ?.replace(' ', '_')
            ?.take(200)
            ?: "-"
}
