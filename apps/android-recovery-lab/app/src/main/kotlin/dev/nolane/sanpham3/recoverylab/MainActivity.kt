package dev.nolane.sanpham3.recoverylab

import android.Manifest
import android.app.Activity
import android.content.ContentValues
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.provider.MediaStore
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
import dev.nolane.sanpham3.androidhost.AndroidBleL2capClientDataPath
import dev.nolane.sanpham3.androidhost.AndroidBleL2capEvent
import dev.nolane.sanpham3.androidhost.AndroidBleL2capServerDataPath
import dev.nolane.sanpham3.androidhost.AndroidFeatureScanner
import dev.nolane.sanpham3.androidhost.AndroidG8PairCourt
import dev.nolane.sanpham3.androidhost.AndroidPeerSession
import dev.nolane.sanpham3.androidhost.acceptPeerSession
import dev.nolane.sanpham3.androidhost.connectPeerSession
import java.io.File
import java.security.SecureRandom
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class MainActivity : Activity() {
    companion object {
        private const val permissionRequestCode = 7001
        private const val acceptTimeoutMillis = 120_000
        private val evidenceStamp = DateTimeFormatter
            .ofPattern("yyyyMMdd'T'HHmmss'Z'")
            .withZone(ZoneOffset.UTC)
    }

    private data class LabConfig(
        val nodeId: Long,
        val peerKey: ByteArray,
    )

    private data class ActiveResources(
        val session: AndroidPeerSession?,
        val discovery: AndroidBleDiscovery?,
        val server: AndroidBleL2capServerDataPath?,
        val key: ByteArray?,
    )

    private val worker = Executors.newSingleThreadExecutor()
    private val clientChosen = AtomicBoolean(false)
    private val stateLock = Any()
    private val transcript = StringBuilder()

    private lateinit var nodeIdInput: EditText
    private lateinit var pskInput: EditText
    private lateinit var logView: TextView
    private lateinit var logScroll: ScrollView
    private var pskVisible = false

    @Volatile
    private var discovery: AndroidBleDiscovery? = null

    @Volatile
    private var bleServer: AndroidBleL2capServerDataPath? = null

    @Volatile
    private var peerSession: AndroidPeerSession? = null

    @Volatile
    private var pendingPeerKey: ByteArray? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildUi())

        appendLog("LAB_START git=${BuildConfig.GIT_SHA}")
        appendLog(
            "DEVICE manufacturer=${Build.MANUFACTURER} " +
                "model=${Build.MODEL} sdk=${Build.VERSION.SDK_INT}",
        )
        appendLog("No PSK is written to evidence logs.")
        appendLog(
            "LOCAL_NETWORK_PERMISSION state=" +
                RecoveryLabPermissions.localNetworkPermissionState(
                    this,
                    Build.VERSION.SDK_INT,
                ),
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
            val granted = grantResults.isNotEmpty() &&
                grantResults.all {
                    it == PackageManager.PERMISSION_GRANTED
                }
            appendLog("PERMISSIONS granted=$granted")
            showFeatureReport()
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
            text = "SP3 Android Recovery Lab"
            textSize = 22f
        })

        root.addView(TextView(this).apply {
            text =
                "Physical G8 tool: BLE discovery → L2CAP → Rust peer-session → encrypted challenge/ACK"
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
            transformationMethod = PasswordTransformationMethod.getInstance()
            isSingleLine = true
        }
        root.addView(pskInput)

        root.addView(button("Generate laboratory PSK") {
            generateLabPsk()
        })
        root.addView(button("Show / hide PSK") {
            togglePskVisibility()
        })
        root.addView(button("Grant lab permissions") {
            requestLabPermissions()
        })
        root.addView(button("Scan capabilities") {
            showFeatureReport()
        })
        root.addView(button("Start BLE G8 server") {
            startBleServer()
        })
        root.addView(button("Start BLE G8 client") {
            startBleClient()
        })
        root.addView(button("Open physical signal emitter") {
            startActivity(
                Intent(
                    this,
                    PhysicalSignalEmitterActivity::class.java,
                ),
            )
        })
        root.addView(button("Open RFCOMM physical G8 court") {
            startActivity(Intent(this, RfcommCourtActivity::class.java))
        })
        root.addView(button("Open BLE GATT physical G8 court") {
            startActivity(Intent(this, GattCourtActivity::class.java))
        })
        root.addView(button("Open NFC HCE / Reader physical G8 court") {
            startActivity(Intent(this, NfcCourtActivity::class.java))
        })
        root.addView(button("Open Local-Only Hotspot G8 court") {
            startActivity(Intent(this, HotspotCourtActivity::class.java))
        })
        root.addView(button("Open G9 peer-egress recovery court") {
            startActivity(Intent(this, G9PeerEgressActivity::class.java))
        })
        root.addView(button("Stop active court") {
            stopActive("user_stop")
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

    private fun requestLabPermissions() {
        val missing = requiredPermissions().filter { permission ->
            checkSelfPermission(permission) !=
                PackageManager.PERMISSION_GRANTED
        }

        if (missing.isEmpty()) {
            appendLog("PERMISSIONS already_granted=true")
            showFeatureReport()
            return
        }

        appendLog("PERMISSIONS requesting=${missing.joinToString()}")
        requestPermissions(
            missing.toTypedArray(),
            permissionRequestCode,
        )
    }

    private fun requiredPermissions(): List<String> =
        RecoveryLabPermissions.allLabPermissions(
            Build.VERSION.SDK_INT,
        )

    private fun permissionsReady(): Boolean =
        requiredPermissions().all { permission ->
            checkSelfPermission(permission) ==
                PackageManager.PERMISSION_GRANTED
        }

    private fun showFeatureReport() {
        val report = AndroidFeatureScanner(this).scan()
        appendLog(
            "FEATURE bluetoothLeHardware=${report.bluetoothLeHardware} " +
                "bluetoothLeL2capCocApiSupported=" +
                "${report.bluetoothLeL2capCocApiSupported} " +
                "scanPermission=${report.bluetoothScanPermission} " +
                "advertisePermission=${report.bluetoothAdvertisePermission} " +
                "connectPermission=${report.bluetoothConnectPermission} " +
                "accessLocalNetworkPermission=" +
                "${report.accessLocalNetworkPermission}",
        )
    }

    private fun readConfig(): LabConfig? {
        if (!permissionsReady()) {
            appendLog("ERROR BLE permissions are not ready")
            requestLabPermissions()
            return null
        }

        return try {
            val nodeId = nodeIdInput.text.toString().trim().toLong()
            require(nodeId >= 0) {
                "node ID must be non-negative"
            }
            val key = LabCodec.parsePeerKey(
                pskInput.text.toString(),
            )
            LabConfig(nodeId, key)
        } catch (error: Exception) {
            appendLog("ERROR config=${error.message}")
            null
        }
    }

    private fun startBleServer() {
        val config = readConfig() ?: return
        stopActive("replace_with_server")
        pendingPeerKey = config.peerKey

        val server = AndroidBleL2capServerDataPath(this)
        val endpoint = try {
            server.start { event ->
                when (event) {
                    is AndroidBleL2capEvent.Listening -> {
                        appendLog(
                            "BLE_L2CAP listening psm=${event.endpoint.psm} " +
                                "discovery=${LabCodec.hex(event.discoveryInfo)}",
                        )
                    }
                    is AndroidBleL2capEvent.Failed -> {
                        appendLog("ERROR l2cap_server=${event.detail}")
                    }
                }
            }
        } catch (error: Exception) {
            config.peerKey.fill(0)
            pendingPeerKey = null
            appendLog("ERROR l2cap_server_start=${error.message}")
            saveEvidence(
                "g8-ble-server-start-fail",
                config.nodeId,
                listOf("error=${error.javaClass.simpleName}"),
            )
            return
        }

        val scanner = AndroidBleDiscovery(this)
        synchronized(stateLock) {
            bleServer = server
            discovery = scanner
        }

        scanner.start(server.discoveryInfo()) { event ->
            when (event) {
                AndroidBleEvent.Started -> {
                    appendLog("BLE_DISCOVERY advertising=true scanning=true")
                }
                is AndroidBleEvent.PeerDiscovered -> {
                    appendLog(
                        "BLE_SEEN rssi=${event.peer.rssi} " +
                            "hint=${event.peer.addressHint}",
                    )
                }
                is AndroidBleEvent.Failed -> {
                    appendLog("ERROR ble_discovery=${event.detail}")
                }
            }
        }

        appendLog(
            "G8_SERVER_WAIT node=${config.nodeId} psm=${endpoint.psm}",
        )

        worker.execute {
            var session: AndroidPeerSession? = null
            try {
                session = server.acceptPeerSession(
                    timeoutMillis = acceptTimeoutMillis,
                    nodeId = config.nodeId,
                    peerKey = config.peerKey,
                )
                clearPendingKey(config.peerKey)
                peerSession = session
                require(session.peerNodeId != config.nodeId) {
                    "peer node ID must differ from local node ID"
                }

                scanner.close()
                val evidence = AndroidG8PairCourt.serveOnce(session)
                val challenge = LabCodec.hex(evidence.challenge)
                appendLog(
                    "G8_PASS role=server local_node=${config.nodeId} " +
                        "peer_node=${evidence.peerNodeId} " +
                        "challenge=$challenge carrier=ble_l2cap",
                )
                saveEvidence(
                    "g8-ble-server-pass",
                    config.nodeId,
                    listOf(
                        "role=server",
                        "carrier=ble_l2cap",
                        "psm=${endpoint.psm}",
                        "authenticated_peer_node=${evidence.peerNodeId}",
                        "challenge_hex=$challenge",
                        "result=PASS",
                    ),
                )
            } catch (error: Throwable) {
                clearPendingKey(config.peerKey)
                appendLog(
                    "G8_FAIL role=server error=${error.javaClass.simpleName}:${error.message}",
                )
                saveEvidence(
                    "g8-ble-server-fail",
                    config.nodeId,
                    listOf(
                        "role=server",
                        "carrier=ble_l2cap",
                        "psm=${endpoint.psm}",
                        "result=FAIL",
                        "error=${error.javaClass.simpleName}",
                    ),
                )
            } finally {
                session?.close()
                scanner.close()
                server.close()
                clearActiveReferences(session, scanner, server)
            }
        }
    }

    private fun startBleClient() {
        val config = readConfig() ?: return
        stopActive("replace_with_client")
        clientChosen.set(false)
        pendingPeerKey = config.peerKey

        val scanner = AndroidBleDiscovery(this)
        discovery = scanner

        appendLog("G8_CLIENT_SCAN node=${config.nodeId}")
        scanner.start(byteArrayOf(0)) peerScan@{ event ->
            when (event) {
                AndroidBleEvent.Started -> {
                    appendLog("BLE_DISCOVERY client_scan_started=true")
                }
                is AndroidBleEvent.PeerDiscovered -> {
                    val psm = LabCodec.decodeL2capPsm(
                        event.peer.serviceData,
                    ) ?: return@peerScan

                    if (!clientChosen.compareAndSet(false, true)) {
                        return@peerScan
                    }

                    val peer = event.peer
                    appendLog(
                        "BLE_L2CAP_CANDIDATE rssi=${peer.rssi} " +
                            "psm=$psm hint=${peer.addressHint} " +
                            "service_data=${LabCodec.hex(peer.serviceData)}",
                    )
                    scanner.close()

                    worker.execute {
                        var session: AndroidPeerSession? = null
                        try {
                            val path = AndroidBleL2capClientDataPath(this)
                            session = path.connectPeerSession(
                                peer = peer,
                                nodeId = config.nodeId,
                                peerKey = config.peerKey,
                            )
                            clearPendingKey(config.peerKey)
                            peerSession = session
                            require(session.peerNodeId != config.nodeId) {
                                "peer node ID must differ from local node ID"
                            }

                            val evidence =
                                AndroidG8PairCourt.runClient(session)
                            val challenge =
                                LabCodec.hex(evidence.challenge)
                            appendLog(
                                "G8_PASS role=client local_node=${config.nodeId} " +
                                    "peer_node=${evidence.peerNodeId} " +
                                    "challenge=$challenge carrier=ble_l2cap " +
                                    "rssi=${peer.rssi} psm=$psm",
                            )
                            saveEvidence(
                                "g8-ble-client-pass",
                                config.nodeId,
                                listOf(
                                    "role=client",
                                    "carrier=ble_l2cap",
                                    "rssi=${peer.rssi}",
                                    "psm=$psm",
                                    "service_data_hex=${LabCodec.hex(peer.serviceData)}",
                                    "authenticated_peer_node=${evidence.peerNodeId}",
                                    "challenge_hex=$challenge",
                                    "result=PASS",
                                ),
                            )
                        } catch (error: Throwable) {
                            clearPendingKey(config.peerKey)
                            appendLog(
                                "G8_FAIL role=client error=${error.javaClass.simpleName}:${error.message}",
                            )
                            saveEvidence(
                                "g8-ble-client-fail",
                                config.nodeId,
                                listOf(
                                    "role=client",
                                    "carrier=ble_l2cap",
                                    "rssi=${peer.rssi}",
                                    "psm=$psm",
                                    "result=FAIL",
                                    "error=${error.javaClass.simpleName}",
                                ),
                            )
                        } finally {
                            session?.close()
                            scanner.close()
                            clearActiveReferences(session, scanner, null)
                        }
                    }
                }
                is AndroidBleEvent.Failed -> {
                    appendLog("ERROR ble_discovery=${event.detail}")
                }
            }
        }
    }

    private fun stopActive(reason: String) {
        val old = synchronized(stateLock) {
            ActiveResources(
                session = peerSession,
                discovery = discovery,
                server = bleServer,
                key = pendingPeerKey,
            ).also {
                peerSession = null
                discovery = null
                bleServer = null
                pendingPeerKey = null
                clientChosen.set(false)
            }
        }

        old.key?.fill(0)
        runCatching { old.session?.close() }
        runCatching { old.discovery?.close() }
        runCatching { old.server?.close() }
        appendLog("STOP reason=$reason")
    }

    private fun clearPendingKey(key: ByteArray) {
        key.fill(0)
        synchronized(stateLock) {
            if (pendingPeerKey === key) {
                pendingPeerKey = null
            }
        }
    }

    private fun clearActiveReferences(
        session: AndroidPeerSession?,
        scanner: AndroidBleDiscovery?,
        server: AndroidBleL2capServerDataPath?,
    ) {
        synchronized(stateLock) {
            if (peerSession === session) peerSession = null
            if (discovery === scanner) discovery = null
            if (bleServer === server) bleServer = null
        }
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

    private fun publishEvidenceToDownloads(
        fileName: String,
        content: String,
    ): String {
        val values = ContentValues().apply {
            put(MediaStore.Downloads.DISPLAY_NAME, fileName)
            put(MediaStore.Downloads.MIME_TYPE, "text/plain")
            put(
                MediaStore.Downloads.RELATIVE_PATH,
                "Download/SP3-Recovery-Lab",
            )
        }

        val uri = checkNotNull(
            contentResolver.insert(
                MediaStore.Downloads.EXTERNAL_CONTENT_URI,
                values,
            ),
        ) {
            "failed to create Downloads evidence entry"
        }

        contentResolver.openOutputStream(uri, "w").use { output ->
            checkNotNull(output) {
                "failed to open Downloads evidence entry"
            }
            output.bufferedWriter().use { writer ->
                writer.write(content)
            }
        }

        return uri.toString()
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
            val logSnapshot = synchronized(transcript) {
                transcript.toString()
            }

            val evidenceText = buildString {
                appendLine("timestamp_utc=${Instant.now()}")
                appendLine("git_commit=${BuildConfig.GIT_SHA}")
                appendLine(
                    "device=${Build.MANUFACTURER} ${Build.MODEL}",
                )
                appendLine("android_sdk=${Build.VERSION.SDK_INT}")
                appendLine("local_node_id=$nodeId")
                for (field in fields) appendLine(field)
                appendLine()
                appendLine("--- transcript ---")
                append(logSnapshot)
            }

            file.writeText(evidenceText)
            val downloadUri = publishEvidenceToDownloads(
                file.name,
                evidenceText,
            )
            appendLog(
                "EVIDENCE saved=${file.absolutePath} downloads=$downloadUri",
            )
        }.onFailure { error ->
            appendLog(
                "ERROR evidence_save=${error.javaClass.simpleName}:${error.message}",
            )
        }
    }
}