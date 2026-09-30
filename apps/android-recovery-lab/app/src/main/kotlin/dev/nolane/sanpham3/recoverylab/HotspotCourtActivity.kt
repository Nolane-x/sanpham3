package dev.nolane.sanpham3.recoverylab

import android.Manifest
import android.app.Activity
import android.content.ContentValues
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
import dev.nolane.sanpham3.androidhost.AndroidG8PairCourt
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotBootstrap
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotClientDataPath
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotClientEvent
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotServerDataPath
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotServerEvent
import dev.nolane.sanpham3.androidhost.AndroidPeerSession
import dev.nolane.sanpham3.androidhost.acceptPeerSession
import dev.nolane.sanpham3.androidhost.connectPeerSession
import java.io.Closeable
import java.io.File
import java.security.SecureRandom
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class HotspotCourtActivity : Activity() {
    companion object {
        private const val permissionRequestCode = 7101
        private const val defaultPort = 45125
        private const val timeoutMillis = 120_000
        private val evidenceStamp = DateTimeFormatter
            .ofPattern("yyyyMMdd'T'HHmmss'Z'")
            .withZone(ZoneOffset.UTC)
    }

    private data class LabConfig(
        val nodeId: Long,
        val peerKey: ByteArray,
    )

    private val worker = Executors.newSingleThreadExecutor()
    private val transcript = StringBuilder()
    private val clientConnecting = AtomicBoolean(false)
    private val stateLock = Any()

    private lateinit var nodeIdInput: EditText
    private lateinit var pskInput: EditText
    private lateinit var bootstrapInput: EditText
    private lateinit var logView: TextView
    private lateinit var logScroll: ScrollView
    private var pskVisible = false

    @Volatile
    private var hotspotServer: AndroidLocalHotspotServerDataPath? = null

    @Volatile
    private var hotspotClient: AndroidLocalHotspotClientDataPath? = null

    @Volatile
    private var peerSession: AndroidPeerSession? = null

    @Volatile
    private var pendingPeerKey: ByteArray? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildUi())

        appendLog("HOTSPOT_LAB_START git=${BuildConfig.GIT_SHA}")
        appendLog(
            "LOCAL_NETWORK_PERMISSION state=" +
                RecoveryLabPermissions.localNetworkPermissionState(
                    this,
                    Build.VERSION.SDK_INT,
                ),
        )

        appendLog(
            "DEVICE manufacturer=${Build.MANUFACTURER} " +
                "model=${Build.MODEL} sdk=${Build.VERSION.SDK_INT}",
        )
        appendLog(
            "Bootstrap credentials are displayed only in the bootstrap field; " +
                "logs/evidence store its SHA-256, never plaintext credentials.",
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
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == permissionRequestCode) {
            val granted = grantResults.isNotEmpty() &&
                grantResults.all { it == PackageManager.PERMISSION_GRANTED }
            appendLog("PEER_LAN_PERMISSION granted=$granted")
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
            text = "SP3 Local-Only Hotspot G8 Court"
            textSize = 22f
        })
        root.addView(TextView(this).apply {
            text =
                "Physical path: Local-Only Hotspot → exact Android Network → " +
                    "TCP → Rust peer-session → encrypted G8 challenge/ACK"
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
                InputType.TYPE_CLASS_TEXT or InputType.TYPE_TEXT_VARIATION_PASSWORD
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
        root.addView(button("Grant peer-LAN permissions") {
            requestPeerLanPermissions()
        })

        bootstrapInput = EditText(this).apply {
            hint = "Hotspot bootstrap capsule hex — server generates, client pastes"
            inputType = InputType.TYPE_CLASS_TEXT
            isSingleLine = false
            minLines = 2
            maxLines = 5
            setTextIsSelectable(true)
        }
        root.addView(bootstrapInput)

        root.addView(button("Start hotspot G8 server") {
            startHotspotServer()
        })
        root.addView(button("Start hotspot G8 client") {
            startHotspotClient()
        })
        root.addView(button("Clear bootstrap field") {
            bootstrapInput.text.clear()
            appendLog("BOOTSTRAP_FIELD cleared=true")
        })
        root.addView(button("Stop active court") {
            stopActive("user_stop")
        })
        root.addView(button("Back to BLE lab") {
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

    private fun requiredPeerLanPermissions(): List<String> =
        RecoveryLabPermissions.peerLanPermissions(
            Build.VERSION.SDK_INT,
        )

    private fun peerLanPermissionsReady(): Boolean =
        requiredPeerLanPermissions().all { permission ->
            checkSelfPermission(permission) ==
                PackageManager.PERMISSION_GRANTED
        }

    private fun requestPeerLanPermissions() {
        val missing = requiredPeerLanPermissions().filter { permission ->
            checkSelfPermission(permission) !=
                PackageManager.PERMISSION_GRANTED
        }
        if (missing.isEmpty()) {
            appendLog("PEER_LAN_PERMISSION already_granted=true")
            return
        }
        appendLog(
            "PEER_LAN_PERMISSION requesting=${missing.joinToString()}",
        )
        requestPermissions(
            missing.toTypedArray(),
            permissionRequestCode,
        )
    }

    private fun readConfig(): LabConfig? {
        if (!peerLanPermissionsReady()) {
            appendLog("ERROR peer-LAN permissions are not ready")
            requestPeerLanPermissions()
            return null
        }

        return try {
            val nodeId = nodeIdInput.text.toString().trim().toLong()
            require(nodeId >= 0) {
                "node ID must be non-negative"
            }
            val peerKey = LabCodec.parsePeerKey(pskInput.text.toString())
            LabConfig(nodeId, peerKey)
        } catch (error: Exception) {
            appendLog("ERROR config=${error.message}")
            null
        }
    }

    private fun startHotspotServer() {
        val config = readConfig() ?: return
        stopActive("replace_with_hotspot_server")
        pendingPeerKey = config.peerKey

        val server = AndroidLocalHotspotServerDataPath(this)
        hotspotServer = server

        val startAt = System.nanoTime()
        try {
            server.start(defaultPort) { event ->
                when (event) {
                    is AndroidLocalHotspotServerEvent.Ready -> {
                        val capsule = try {
                            AndroidLocalHotspotBootstrap.encode(event.endpoint)
                        } catch (error: Throwable) {
                            appendLog(
                                "HOTSPOT_FAIL role=server stage=bootstrap " +
                                    "error=${error.javaClass.simpleName}",
                            )
                            stopActive("bootstrap_failure")
                            return@start
                        }

                        val capsuleHash = LabCodec.sha256Hex(capsule)
                        val startupMs = (System.nanoTime() - startAt) / 1_000_000
                        runOnUiThread {
                            bootstrapInput.setText(LabCodec.hex(capsule))
                            bootstrapInput.setSelection(bootstrapInput.text.length)
                        }

                        appendLog(
                            "HOTSPOT_READY role=server security=${event.endpoint.security} " +
                                "port=${event.endpoint.port} capsule_bytes=${capsule.size} " +
                                "capsule_sha256=$capsuleHash startup_ms=$startupMs",
                        )

                        worker.execute {
                            runHotspotServerCourt(
                                config = config,
                                server = server,
                                capsuleHash = capsuleHash,
                                startupMs = startupMs,
                            )
                        }
                    }
                    is AndroidLocalHotspotServerEvent.Failed -> {
                        clearPendingKey(config.peerKey)
                        appendLog(
                            "HOTSPOT_FAIL role=server stage=start " +
                                "reason=${event.reasonCode} detail=${event.detail}",
                        )
                        saveEvidence(
                            "g8-hotspot-server-start-fail",
                            config.nodeId,
                            listOf(
                                "role=server",
                                "carrier=local_only_hotspot",
                                "result=FAIL",
                                "stage=start",
                                "reason_code=${event.reasonCode}",
                            ),
                        )
                    }
                }
            }
        } catch (error: Throwable) {
            clearPendingKey(config.peerKey)
            server.close()
            hotspotServer = null
            appendLog(
                "HOTSPOT_FAIL role=server stage=start " +
                    "error=${error.javaClass.simpleName}:${error.message}",
            )
            saveEvidence(
                "g8-hotspot-server-start-fail",
                config.nodeId,
                listOf(
                    "role=server",
                    "carrier=local_only_hotspot",
                    "result=FAIL",
                    "stage=start",
                    "error=${error.javaClass.simpleName}",
                ),
            )
        }
    }

    private fun runHotspotServerCourt(
        config: LabConfig,
        server: AndroidLocalHotspotServerDataPath,
        capsuleHash: String,
        startupMs: Long,
    ) {
        var session: AndroidPeerSession? = null
        val acceptAt = System.nanoTime()
        try {
            session = server.acceptPeerSession(
                timeoutMillis = timeoutMillis,
                nodeId = config.nodeId,
                peerKey = config.peerKey,
            )
            clearPendingKey(config.peerKey)
            peerSession = session

            require(session.peerNodeId != config.nodeId) {
                "peer node ID must differ from local node ID"
            }

            val evidence = AndroidG8PairCourt.serveOnce(session)
            val g8Ms = (System.nanoTime() - acceptAt) / 1_000_000
            val challenge = LabCodec.hex(evidence.challenge)

            appendLog(
                "G8_PASS role=server carrier=local_only_hotspot " +
                    "local_node=${config.nodeId} peer_node=${evidence.peerNodeId} " +
                    "challenge=$challenge g8_wait_ms=$g8Ms",
            )
            saveEvidence(
                "g8-hotspot-server-pass",
                config.nodeId,
                listOf(
                    "role=server",
                    "carrier=local_only_hotspot",
                    "bootstrap_sha256=$capsuleHash",
                    "hotspot_startup_ms=$startupMs",
                    "authenticated_peer_node=${evidence.peerNodeId}",
                    "challenge_hex=$challenge",
                    "g8_wait_ms=$g8Ms",
                    "result=PASS",
                ),
            )
        } catch (error: Throwable) {
            clearPendingKey(config.peerKey)
            appendLog(
                "G8_FAIL role=server carrier=local_only_hotspot " +
                    "error=${error.javaClass.simpleName}:${error.message}",
            )
            saveEvidence(
                "g8-hotspot-server-fail",
                config.nodeId,
                listOf(
                    "role=server",
                    "carrier=local_only_hotspot",
                    "bootstrap_sha256=$capsuleHash",
                    "hotspot_startup_ms=$startupMs",
                    "result=FAIL",
                    "error=${error.javaClass.simpleName}",
                ),
            )
        } finally {
            session?.close()
            server.close()
            synchronized(stateLock) {
                if (peerSession === session) peerSession = null
                if (hotspotServer === server) hotspotServer = null
            }
        }
    }

    private fun startHotspotClient() {
        val config = readConfig() ?: return
        val capsule = try {
            LabCodec.parseHex(bootstrapInput.text.toString())
        } catch (error: Throwable) {
            config.peerKey.fill(0)
            appendLog("ERROR bootstrap_hex=${error.message}")
            return
        }

        val endpoint = try {
            AndroidLocalHotspotBootstrap.decode(capsule)
        } catch (error: Throwable) {
            config.peerKey.fill(0)
            capsule.fill(0)
            appendLog("ERROR bootstrap_decode=${error.message}")
            return
        }

        val capsuleHash = LabCodec.sha256Hex(capsule)
        capsule.fill(0)

        stopActive("replace_with_hotspot_client")
        pendingPeerKey = config.peerKey
        clientConnecting.set(false)

        val client = AndroidLocalHotspotClientDataPath(this)
        hotspotClient = client
        val requestAt = System.nanoTime()

        appendLog(
            "HOTSPOT_CLIENT_REQUEST security=${endpoint.security} " +
                "port=${endpoint.port} bootstrap_sha256=$capsuleHash",
        )

        try {
            client.start(endpoint) { event ->
                when (event) {
                    is AndroidLocalHotspotClientEvent.NetworkAvailable -> {
                        val joinMs = (System.nanoTime() - requestAt) / 1_000_000
                        appendLog(
                            "HOTSPOT_NETWORK_AVAILABLE role=client " +
                                "server=${event.route.serverAddress.hostAddress} " +
                                "port=${event.route.port} join_ms=$joinMs",
                        )

                        if (clientConnecting.compareAndSet(false, true)) {
                            worker.execute {
                                runHotspotClientCourt(
                                    config = config,
                                    client = client,
                                    capsuleHash = capsuleHash,
                                    joinMs = joinMs,
                                )
                            }
                        }
                    }
                    AndroidLocalHotspotClientEvent.Lost -> {
                        appendLog("HOTSPOT_NETWORK_LOST role=client")
                    }
                    is AndroidLocalHotspotClientEvent.Unavailable -> {
                        clearPendingKey(config.peerKey)
                        appendLog(
                            "HOTSPOT_FAIL role=client stage=join " +
                                "detail=${event.detail}",
                        )
                        saveEvidence(
                            "g8-hotspot-client-join-fail",
                            config.nodeId,
                            listOf(
                                "role=client",
                                "carrier=local_only_hotspot",
                                "bootstrap_sha256=$capsuleHash",
                                "result=FAIL",
                                "stage=join",
                            ),
                        )
                    }
                }
            }
        } catch (error: Throwable) {
            clearPendingKey(config.peerKey)
            client.close()
            hotspotClient = null
            appendLog(
                "HOTSPOT_FAIL role=client stage=request " +
                    "error=${error.javaClass.simpleName}:${error.message}",
            )
            saveEvidence(
                "g8-hotspot-client-request-fail",
                config.nodeId,
                listOf(
                    "role=client",
                    "carrier=local_only_hotspot",
                    "bootstrap_sha256=$capsuleHash",
                    "result=FAIL",
                    "stage=request",
                    "error=${error.javaClass.simpleName}",
                ),
            )
        }
    }

    private fun runHotspotClientCourt(
        config: LabConfig,
        client: AndroidLocalHotspotClientDataPath,
        capsuleHash: String,
        joinMs: Long,
    ) {
        var session: AndroidPeerSession? = null
        val connectAt = System.nanoTime()
        try {
            session = client.connectPeerSession(
                timeoutMillis = timeoutMillis,
                nodeId = config.nodeId,
                peerKey = config.peerKey,
            )
            clearPendingKey(config.peerKey)
            peerSession = session

            require(session.peerNodeId != config.nodeId) {
                "peer node ID must differ from local node ID"
            }

            val evidence = AndroidG8PairCourt.runClient(session)
            val g8Ms = (System.nanoTime() - connectAt) / 1_000_000
            val challenge = LabCodec.hex(evidence.challenge)

            appendLog(
                "G8_PASS role=client carrier=local_only_hotspot " +
                    "local_node=${config.nodeId} peer_node=${evidence.peerNodeId} " +
                    "challenge=$challenge join_ms=$joinMs g8_ms=$g8Ms",
            )
            saveEvidence(
                "g8-hotspot-client-pass",
                config.nodeId,
                listOf(
                    "role=client",
                    "carrier=local_only_hotspot",
                    "bootstrap_sha256=$capsuleHash",
                    "network_join_ms=$joinMs",
                    "authenticated_peer_node=${evidence.peerNodeId}",
                    "challenge_hex=$challenge",
                    "g8_ms=$g8Ms",
                    "result=PASS",
                ),
            )
        } catch (error: Throwable) {
            clearPendingKey(config.peerKey)
            appendLog(
                "G8_FAIL role=client carrier=local_only_hotspot " +
                    "error=${error.javaClass.simpleName}:${error.message}",
            )
            saveEvidence(
                "g8-hotspot-client-fail",
                config.nodeId,
                listOf(
                    "role=client",
                    "carrier=local_only_hotspot",
                    "bootstrap_sha256=$capsuleHash",
                    "network_join_ms=$joinMs",
                    "result=FAIL",
                    "error=${error.javaClass.simpleName}",
                ),
            )
        } finally {
            session?.close()
            client.close()
            clientConnecting.set(false)
            synchronized(stateLock) {
                if (peerSession === session) peerSession = null
                if (hotspotClient === client) hotspotClient = null
            }
        }
    }

    private fun stopActive(reason: String) {
        var oldSession: AndroidPeerSession? = null
        var oldServer: Closeable? = null
        var oldClient: Closeable? = null
        var oldKey: ByteArray? = null

        synchronized(stateLock) {
            oldSession = peerSession
            oldServer = hotspotServer
            oldClient = hotspotClient
            oldKey = pendingPeerKey

            peerSession = null
            hotspotServer = null
            hotspotClient = null
            pendingPeerKey = null
            clientConnecting.set(false)
        }

        oldKey?.fill(0)
        runCatching { oldSession?.close() }
        runCatching { oldServer?.close() }
        runCatching { oldClient?.close() }
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
                appendLine("device=${Build.MANUFACTURER} ${Build.MODEL}")
                appendLine("android_sdk=${Build.VERSION.SDK_INT}")
                appendLine("local_node_id=$nodeId")
                for (field in fields) appendLine(field)
                appendLine()
                appendLine("--- transcript ---")
                append(logSnapshot)
            }

            file.writeText(evidenceText)
            val downloadUri = publishEvidenceToDownloads(file.name, evidenceText)
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
