package dev.nolane.sanpham3.recoverylab

import android.Manifest
import android.app.Activity
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
import dev.nolane.sanpham3.androidhost.AndroidBleDiscovery
import dev.nolane.sanpham3.androidhost.AndroidBleEvent
import dev.nolane.sanpham3.androidhost.AndroidBleGattEvidence
import dev.nolane.sanpham3.androidhost.AndroidBleGattG8Client
import dev.nolane.sanpham3.androidhost.AndroidBleGattServer
import dev.nolane.sanpham3.androidhost.AndroidBleGattServerEvent
import dev.nolane.sanpham3.androidhost.AndroidBlePeer
import java.io.File
import java.security.SecureRandom
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.concurrent.ConcurrentHashMap
import java.util.concurrent.Executors

class GattCourtActivity : Activity() {
    companion object {
        private const val permissionRequestCode = 7501
        private const val clientTimeoutMillis = 20_000L
        private const val benchmarkRounds = 32
        private val evidenceStamp = DateTimeFormatter
            .ofPattern("yyyyMMdd'T'HHmmss'Z'")
            .withZone(ZoneOffset.UTC)
    }

    private data class LabConfig(
        val nodeId: Long,
        val peerKey: ByteArray,
    )

    private val worker = Executors.newSingleThreadExecutor()
    private val peers = ConcurrentHashMap<String, AndroidBlePeer>()
    private val transcript = StringBuilder()
    private val stateLock = Any()

    private lateinit var nodeIdInput: EditText
    private lateinit var pskInput: EditText
    private lateinit var targetAddressInput: EditText
    private lateinit var logView: TextView
    private lateinit var logScroll: ScrollView

    @Volatile
    private var discovery: AndroidBleDiscovery? = null

    @Volatile
    private var server: AndroidBleGattServer? = null

    @Volatile
    private var latestServerPair: AndroidBleGattEvidence? = null

    private var pskVisible = false

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildUi())

        appendLog("GATT_LAB_START git=${BuildConfig.GIT_SHA}")
        appendLog(
            "DEVICE manufacturer=${Build.MANUFACTURER} " +
                "model=${Build.MODEL} sdk=${Build.VERSION.SDK_INT}",
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
                "GATT_PERMISSION granted=" +
                    (
                        grantResults.isNotEmpty() &&
                            grantResults.all {
                                it == PackageManager.PERMISSION_GRANTED
                            }
                    ),
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
            text = "SP3 BLE GATT Physical G8 Court"
            textSize = 22f
        })
        root.addView(TextView(this).apply {
            text =
                "BLE marker -> GATT MTU/service -> Rust peer-session -> " +
                    "encrypted G8 -> encrypted RTT/goodput benchmark"
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
            hint = "Target BLE address from scan"
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
        root.addView(button("Grant BLE permissions") {
            requestBlePermissions()
        })
        root.addView(button("Start GATT server + advertise") {
            startGattServer()
        })
        root.addView(button("Scan GATT peers") {
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

    private fun requestBlePermissions() {
        val missing = requiredPermissions().filter {
            checkSelfPermission(it) !=
                PackageManager.PERMISSION_GRANTED
        }
        if (missing.isEmpty()) {
            appendLog("GATT_PERMISSION already_granted=true")
            return
        }
        requestPermissions(
            missing.toTypedArray(),
            permissionRequestCode,
        )
    }

    private fun readConfig(): LabConfig? {
        if (!permissionsReady()) {
            appendLog("ERROR GATT permissions are not ready")
            requestBlePermissions()
            return null
        }

        return try {
            val nodeId = nodeIdInput.text.toString().trim().toLong()
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

    private fun startGattServer() {
        val config = readConfig() ?: return
        stopActive("replace_with_gatt_server")

        val localServer = AndroidBleGattServer(this)
        val localDiscovery = AndroidBleDiscovery(this)
        synchronized(stateLock) {
            server = localServer
            discovery = localDiscovery
            latestServerPair = null
        }

        try {
            localServer.start(
                nodeId = config.nodeId,
                peerKey = config.peerKey,
            ) { event ->
                when (event) {
                    AndroidBleGattServerEvent.Started -> {
                        appendLog(
                            "GATT_SERVER_STARTED node=${config.nodeId}",
                        )
                        startServerDiscovery(
                            localServer,
                            localDiscovery,
                        )
                    }

                    is AndroidBleGattServerEvent.PeerAuthenticated -> {
                        appendLog(
                            "GATT_PEER_AUTHENTICATED peer_node=" +
                                "${event.peerNodeId}",
                        )
                    }

                    is AndroidBleGattServerEvent.PairPassed -> {
                        latestServerPair = event.evidence
                        appendLog(
                            "GATT_G8_PASS role=server " +
                                "peer_node=${event.evidence.authenticatedPeerNodeId} " +
                                "mtu=${event.evidence.mtu}",
                        )
                    }

                    is AndroidBleGattServerEvent.BenchmarkProbe -> {
                        if (event.sequence == 0L ||
                            event.sequence == benchmarkRounds - 1L
                        ) {
                            appendLog(
                                "GATT_BENCHMARK_PROBE peer_node=" +
                                    "${event.peerNodeId} " +
                                    "sequence=${event.sequence} " +
                                    "payload_bytes=${event.payloadBytes}",
                            )
                        }

                        if (event.sequence == benchmarkRounds - 1L) {
                            val pair = latestServerPair
                            val challenge = pair?.challenge
                                ?.let(LabCodec::hex)
                                ?: "-"
                            saveEvidence(
                                "g8-gatt-server-pass",
                                config.nodeId,
                                listOf(
                                    "role=server",
                                    "carrier=ble_gatt",
                                    "authenticated_peer_node=${event.peerNodeId}",
                                    "challenge_hex=$challenge",
                                    "negotiated_mtu=${pair?.mtu ?: -1}",
                                    "benchmark_rounds=$benchmarkRounds",
                                    "benchmark_payload_bytes=${event.payloadBytes}",
                                    "benchmark_last_sequence=${event.sequence}",
                                    "result=PASS",
                                ),
                            )
                        }
                    }

                    is AndroidBleGattServerEvent.Failed -> {
                        appendLog(
                            "GATT_FAIL role=server detail=" +
                                sanitize(event.detail),
                        )
                        saveEvidence(
                            "g8-gatt-server-fail",
                            config.nodeId,
                            listOf(
                                "role=server",
                                "carrier=ble_gatt",
                                "result=FAIL",
                                "detail=${sanitize(event.detail)}",
                            ),
                        )
                    }
                }
            }
            config.peerKey.fill(0)
        } catch (error: Throwable) {
            config.peerKey.fill(0)
            localDiscovery.close()
            localServer.close()
            synchronized(stateLock) {
                if (server === localServer) server = null
                if (discovery === localDiscovery) discovery = null
            }
            appendLog(
                "GATT_FAIL role=server stage=start error=" +
                    "${error.javaClass.simpleName}:${sanitize(error.message)}",
            )
        }
    }

    private fun startServerDiscovery(
        localServer: AndroidBleGattServer,
        localDiscovery: AndroidBleDiscovery,
    ) {
        runCatching {
            localDiscovery.start(
                localServer.discoveryInfo(),
            ) { event ->
                handleDiscoveryEvent(event, "server")
            }
        }.onFailure { error ->
            appendLog(
                "GATT_DISCOVERY_FAIL role=server error=" +
                    "${error.javaClass.simpleName}:${sanitize(error.message)}",
            )
        }
    }

    private fun scanPeers() {
        if (!permissionsReady()) {
            requestBlePermissions()
            return
        }

        discovery?.close()
        peers.clear()

        val scanner = AndroidBleDiscovery(this)
        discovery = scanner
        runCatching {
            scanner.start(
                AndroidBleGattServer.discoveryMarker(),
            ) { event ->
                handleDiscoveryEvent(event, "client")
            }
        }.onFailure { error ->
            scanner.close()
            discovery = null
            appendLog(
                "GATT_DISCOVERY_FAIL role=client error=" +
                    "${error.javaClass.simpleName}:${sanitize(error.message)}",
            )
        }
    }

    private fun handleDiscoveryEvent(
        event: AndroidBleEvent,
        role: String,
    ) {
        when (event) {
            AndroidBleEvent.Started -> {
                appendLog("GATT_DISCOVERY_STARTED role=$role")
            }

            is AndroidBleEvent.PeerDiscovered -> {
                val peer = event.peer
                if (peer.addressHint.isBlank()) return
                peers[peer.addressHint.uppercase()] = peer
                appendLog(
                    "GATT_PEER role=$role " +
                        "address=${peer.addressHint} " +
                        "rssi=${peer.rssi}",
                )
            }

            is AndroidBleEvent.Failed -> {
                appendLog(
                    "GATT_DISCOVERY_FAIL role=$role " +
                        "code=${event.code ?: -1} " +
                        "detail=${sanitize(event.detail)}",
                )
            }
        }
    }

    private fun connectTarget() {
        val config = readConfig() ?: return
        val target =
            targetAddressInput.text.toString().trim().uppercase()
        if (target.isBlank()) {
            config.peerKey.fill(0)
            appendLog("ERROR target BLE address is empty")
            return
        }

        val peer = peers[target]
        if (peer == null) {
            config.peerKey.fill(0)
            appendLog(
                "GATT_FAIL role=client stage=select_peer " +
                    "target=${sanitize(target)}",
            )
            return
        }

        worker.execute {
            val started = System.nanoTime()
            try {
                val evidence = AndroidBleGattG8Client(this).run(
                    peer = peer,
                    nodeId = config.nodeId,
                    peerKey = config.peerKey,
                    timeoutMillis = clientTimeoutMillis,
                )
                val totalMs =
                    (System.nanoTime() - started) / 1_000_000
                config.peerKey.fill(0)

                appendLog(
                    "GATT_G8_PASS role=client " +
                        "local_node=${config.nodeId} " +
                        "peer_node=${evidence.authenticatedPeerNodeId} " +
                        "mtu=${evidence.mtu} total_ms=$totalMs " +
                        "rtt_p95_ns=${evidence.benchmarkP95RttNanos} " +
                        "one_way_useful_bps=" +
                        "${evidence.benchmarkOneWayUsefulBitsPerSecond}",
                )
                saveEvidence(
                    "g8-gatt-client-pass",
                    config.nodeId,
                    listOf(
                        "role=client",
                        "carrier=ble_gatt",
                        "target_address_hint=${peer.addressHint}",
                        "rssi=${peer.rssi}",
                        "authenticated_peer_node=${evidence.authenticatedPeerNodeId}",
                        "challenge_hex=${LabCodec.hex(evidence.challenge)}",
                        "negotiated_mtu=${evidence.mtu}",
                        "g8_plus_benchmark_total_ms=$totalMs",
                        "benchmark_rounds=${evidence.benchmarkRounds}",
                        "benchmark_payload_bytes=${evidence.benchmarkPayloadBytes}",
                        "benchmark_elapsed_ns=${evidence.benchmarkElapsedNanos}",
                        "benchmark_rtt_min_ns=${evidence.benchmarkMinRttNanos}",
                        "benchmark_rtt_median_ns=${evidence.benchmarkMedianRttNanos}",
                        "benchmark_rtt_p95_ns=${evidence.benchmarkP95RttNanos}",
                        "benchmark_rtt_max_ns=${evidence.benchmarkMaxRttNanos}",
                        "benchmark_one_way_useful_bps=" +
                            "${evidence.benchmarkOneWayUsefulBitsPerSecond}",
                        "benchmark_round_trip_useful_bps=" +
                            "${evidence.benchmarkRoundTripUsefulBitsPerSecond}",
                        "result=PASS",
                    ),
                )
            } catch (error: Throwable) {
                config.peerKey.fill(0)
                appendLog(
                    "GATT_G8_FAIL role=client " +
                        "error=${error.javaClass.simpleName}:" +
                        sanitize(error.message),
                )
                saveEvidence(
                    "g8-gatt-client-fail",
                    config.nodeId,
                    listOf(
                        "role=client",
                        "carrier=ble_gatt",
                        "target_address_hint=${peer.addressHint}",
                        "result=FAIL",
                        "error=${error.javaClass.simpleName}",
                    ),
                )
            }
        }
    }

    private fun stopActive(reason: String) {
        val oldServer: AndroidBleGattServer?
        val oldDiscovery: AndroidBleDiscovery?

        synchronized(stateLock) {
            oldServer = server
            oldDiscovery = discovery
            server = null
            discovery = null
            latestServerPair = null
        }

        runCatching { oldDiscovery?.close() }
        runCatching { oldServer?.close() }
        appendLog("GATT_STOP reason=$reason")
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
            appendLog("EVIDENCE_SAVED path=${file.absolutePath}")
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
