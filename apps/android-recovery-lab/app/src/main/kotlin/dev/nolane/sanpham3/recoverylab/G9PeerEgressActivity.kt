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
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotBootstrap
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotClientDataPath
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotClientEvent
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotServerDataPath
import dev.nolane.sanpham3.androidhost.AndroidLocalHotspotServerEvent
import dev.nolane.sanpham3.androidhost.AndroidPeerEgress
import dev.nolane.sanpham3.androidhost.AndroidPeerSession
import dev.nolane.sanpham3.androidhost.acceptPeerSession
import dev.nolane.sanpham3.androidhost.connectPeerSession
import java.io.Closeable
import java.io.File
import java.net.URL
import java.security.SecureRandom
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean
import javax.net.ssl.HttpsURLConnection

class G9PeerEgressActivity : Activity() {
    companion object {
        private const val permissionRequestCode = 7201
        private const val defaultPort = 45126
        private const val timeoutMillis = 120_000
        private const val probeTimeoutMillis = 5_000
        private val evidenceStamp = DateTimeFormatter
            .ofPattern("yyyyMMdd'T'HHmmss'Z'")
            .withZone(ZoneOffset.UTC)
    }

    private data class LabConfig(
        val nodeId: Long,
        val peerKey: ByteArray,
    )

    private data class DirectProbe(
        val url: String,
        val startedAt: Instant,
        val completedAt: Instant,
        val reachable: Boolean,
        val responseCode: Int?,
        val errorClass: String?,
    )

    private val worker = Executors.newSingleThreadExecutor()
    private val transcript = StringBuilder()
    private val clientConnecting = AtomicBoolean(false)
    private val stateLock = Any()

    private lateinit var nodeIdInput: EditText
    private lateinit var pskInput: EditText
    private lateinit var bootstrapInput: EditText
    private lateinit var hostnameInput: EditText
    private lateinit var directUrlInput: EditText
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

        appendLog("G9_LAB_START git=${BuildConfig.GIT_SHA}")
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
            "PASS requires failed direct HTTPS before local hotspot join, " +
                "then a live public DNS result through the authenticated peer.",
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
            text = "SP3 G9 Peer-Egress Recovery Court"
            textSize = 22f
        })
        root.addView(TextView(this).apply {
            text =
                "Device A direct HTTPS fails → Local-Only Hotspot → " +
                    "Rust-authenticated peer B → live constrained DNS result"
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

        hostnameInput = EditText(this).apply {
            hint = "Public hostname"
            setText("example.com")
            inputType = InputType.TYPE_CLASS_TEXT or
                InputType.TYPE_TEXT_VARIATION_URI
            isSingleLine = true
        }
        root.addView(hostnameInput)

        directUrlInput = EditText(this).apply {
            hint = "Direct HTTPS probe URL — host must match hostname"
            setText("https://example.com/")
            inputType = InputType.TYPE_CLASS_TEXT or
                InputType.TYPE_TEXT_VARIATION_URI
            isSingleLine = true
        }
        root.addView(directUrlInput)

        bootstrapInput = EditText(this).apply {
            hint = "Hotspot bootstrap capsule hex — server generates, client pastes"
            inputType = InputType.TYPE_CLASS_TEXT
            isSingleLine = false
            minLines = 2
            maxLines = 5
            setTextIsSelectable(true)
        }
        root.addView(bootstrapInput)

        root.addView(button("Start G9 egress server") {
            startG9Server()
        })
        root.addView(button("Start G9 recovery client") {
            startG9Client()
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

    private fun readRemoteTarget(): Pair<String, String>? =
        try {
            val hostname = hostnameInput.text.toString().trim().trimEnd('.')
            require(AndroidPeerEgress.isValidPublicHostname(hostname)) {
                "hostname is not allowed by constrained peer-egress policy"
            }

            val directUrl = directUrlInput.text.toString().trim()
            val parsed = URL(directUrl)
            require(parsed.protocol.equals("https", ignoreCase = true)) {
                "direct probe must use HTTPS"
            }
            require(
                parsed.host.trimEnd('.').equals(
                    hostname,
                    ignoreCase = true,
                ),
            ) {
                "direct HTTPS URL host must match peer-egress hostname"
            }
            hostname to directUrl
        } catch (error: Throwable) {
            appendLog("ERROR remote_target=${error.message}")
            null
        }

    private fun startG9Server() {
        val config = readConfig() ?: return
        readRemoteTarget() ?: run {
            config.peerKey.fill(0)
            return
        }

        stopActive("replace_with_g9_server")
        pendingPeerKey = config.peerKey

        val server = AndroidLocalHotspotServerDataPath(this)
        hotspotServer = server
        val startAt = System.nanoTime()

        try {
            server.start(defaultPort) { event ->
                when (event) {
                    is AndroidLocalHotspotServerEvent.Ready -> {
                        val capsule = AndroidLocalHotspotBootstrap.encode(
                            event.endpoint,
                        )
                        val capsuleHash = LabCodec.sha256Hex(capsule)
                        val startupMs =
                            (System.nanoTime() - startAt) / 1_000_000

                        runOnUiThread {
                            bootstrapInput.setText(LabCodec.hex(capsule))
                            bootstrapInput.setSelection(
                                bootstrapInput.text.length,
                            )
                        }

                        appendLog(
                            "G9_SERVER_READY carrier=local_only_hotspot " +
                                "security=${event.endpoint.security} " +
                                "bootstrap_sha256=$capsuleHash startup_ms=$startupMs",
                        )

                        worker.execute {
                            runG9Server(
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
                            "G9_SERVER_FAIL stage=hotspot_start " +
                                "reason=${event.reasonCode}",
                        )
                        saveEvidence(
                            "g9-server-hotspot-fail",
                            config.nodeId,
                            listOf(
                                "role=server",
                                "carrier=local_only_hotspot",
                                "stage=hotspot_start",
                                "result=FAIL",
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
                "G9_SERVER_FAIL stage=hotspot_start " +
                    "error=${error.javaClass.simpleName}:${error.message}",
            )
        }
    }

    private fun runG9Server(
        config: LabConfig,
        server: AndroidLocalHotspotServerDataPath,
        capsuleHash: String,
        startupMs: Long,
    ) {
        var session: AndroidPeerSession? = null
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

            val observedAt = Instant.now()
            val evidence = AndroidPeerEgress.serveOne(session)
            val addresses = evidence.addresses
                .mapNotNull { it.hostAddress }
                .joinToString(",")

            appendLog(
                "G9_SERVER_RESULT peer=${session.peerNodeId} " +
                    "hostname=${evidence.hostname} status=${evidence.status} " +
                    "addresses=$addresses observed_at=$observedAt",
            )

            saveEvidence(
                "g9-server-result",
                config.nodeId,
                listOf(
                    "role=server",
                    "carrier=local_only_hotspot",
                    "bootstrap_sha256=$capsuleHash",
                    "hotspot_startup_ms=$startupMs",
                    "authenticated_peer_node=${session.peerNodeId}",
                    "hostname=${evidence.hostname}",
                    "resolver_observed_at_utc=$observedAt",
                    "resolve_status=${evidence.status}",
                    "returned_addresses=$addresses",
                    "result=${if (evidence.status == AndroidPeerEgress.ResolveStatus.OK) "PASS" else "FAIL"}",
                ),
            )
        } catch (error: Throwable) {
            clearPendingKey(config.peerKey)
            appendLog(
                "G9_SERVER_FAIL stage=peer_egress " +
                    "error=${error.javaClass.simpleName}:${error.message}",
            )
            saveEvidence(
                "g9-server-fail",
                config.nodeId,
                listOf(
                    "role=server",
                    "carrier=local_only_hotspot",
                    "bootstrap_sha256=$capsuleHash",
                    "result=FAIL",
                    "stage=peer_egress",
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

    private fun startG9Client() {
        val config = readConfig() ?: return
        val target = readRemoteTarget() ?: run {
            config.peerKey.fill(0)
            return
        }
        val hostname = target.first
        val directUrl = target.second

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

        stopActive("replace_with_g9_client")
        pendingPeerKey = config.peerKey
        clientConnecting.set(false)

        appendLog(
            "G9_DIRECT_PROBE_BEGIN url=$directUrl " +
                "before_hotspot_join=true",
        )

        worker.execute {
            val direct = probeDirectHttps(directUrl)
            appendLog(
                "G9_DIRECT_PROBE reachable=${direct.reachable} " +
                    "code=${direct.responseCode} error=${direct.errorClass} " +
                    "started=${direct.startedAt} completed=${direct.completedAt}",
            )

            if (direct.reachable) {
                clearPendingKey(config.peerKey)
                appendLog(
                    "G9_REFUSED direct_default_path_still_works=true",
                )
                saveEvidence(
                    "g9-client-refused-direct-live",
                    config.nodeId,
                    listOf(
                        "role=client",
                        "default_path_probe=https",
                        "default_path_result=WORKING",
                        "direct_url=$directUrl",
                        "direct_response_code=${direct.responseCode}",
                        "result=REFUSED",
                        "reason=direct_default_path_still_works",
                    ),
                )
                return@execute
            }

            val client = AndroidLocalHotspotClientDataPath(this)
            hotspotClient = client
            val requestAt = System.nanoTime()

            try {
                client.start(endpoint) { event ->
                    when (event) {
                        is AndroidLocalHotspotClientEvent.NetworkAvailable -> {
                            val joinMs =
                                (System.nanoTime() - requestAt) / 1_000_000
                            appendLog(
                                "G9_RESCUE_NETWORK_AVAILABLE " +
                                    "server=${event.route.serverAddress.hostAddress} " +
                                    "join_ms=$joinMs",
                            )

                            if (clientConnecting.compareAndSet(false, true)) {
                                worker.execute {
                                    runG9Client(
                                        config = config,
                                        client = client,
                                        hostname = hostname,
                                        direct = direct,
                                        capsuleHash = capsuleHash,
                                        joinMs = joinMs,
                                    )
                                }
                            }
                        }

                        AndroidLocalHotspotClientEvent.Lost -> {
                            appendLog("G9_RESCUE_NETWORK_LOST")
                        }

                        is AndroidLocalHotspotClientEvent.Unavailable -> {
                            clearPendingKey(config.peerKey)
                            appendLog(
                                "G9_CLIENT_FAIL stage=hotspot_join " +
                                    "detail=${event.detail}",
                            )
                            saveEvidence(
                                "g9-client-hotspot-join-fail",
                                config.nodeId,
                                listOf(
                                    "role=client",
                                    "default_path_result=FAILED",
                                    "carrier=local_only_hotspot",
                                    "bootstrap_sha256=$capsuleHash",
                                    "result=FAIL",
                                    "stage=hotspot_join",
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
                    "G9_CLIENT_FAIL stage=hotspot_request " +
                        "error=${error.javaClass.simpleName}:${error.message}",
                )
            }
        }
    }

    private fun runG9Client(
        config: LabConfig,
        client: AndroidLocalHotspotClientDataPath,
        hostname: String,
        direct: DirectProbe,
        capsuleHash: String,
        joinMs: Long,
    ) {
        var session: AndroidPeerSession? = null
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

            val requestId = System.currentTimeMillis() and 0xffff_ffffL
            val rescueObservedAt = Instant.now()
            val addresses = AndroidPeerEgress.resolveViaPeer(
                session = session,
                requestId = requestId,
                hostname = hostname,
            )
            val addressText = addresses
                .mapNotNull { it.hostAddress }
                .joinToString(",")

            appendLog(
                "G9_PHYSICAL_PASS default_failed=true " +
                    "carrier=local_only_hotspot peer=${session.peerNodeId} " +
                    "hostname=$hostname addresses=$addressText " +
                    "observed_at=$rescueObservedAt",
            )

            saveEvidence(
                "g9-client-pass-candidate",
                config.nodeId,
                listOf(
                    "role=client",
                    "default_path_probe=https",
                    "default_path_result=FAILED",
                    "direct_url=${direct.url}",
                    "direct_probe_started_at=${direct.startedAt}",
                    "direct_probe_completed_at=${direct.completedAt}",
                    "direct_probe_error=${direct.errorClass}",
                    "carrier=local_only_hotspot",
                    "bootstrap_sha256=$capsuleHash",
                    "network_join_ms=$joinMs",
                    "authenticated_peer_node=${session.peerNodeId}",
                    "selected_path_kind=PeerEgress",
                    "hostname=$hostname",
                    "request_id=$requestId",
                    "peer_result_observed_at_utc=$rescueObservedAt",
                    "returned_addresses=$addressText",
                    "result=PASS_CANDIDATE",
                ),
            )
        } catch (error: Throwable) {
            clearPendingKey(config.peerKey)
            appendLog(
                "G9_CLIENT_FAIL stage=peer_rescue " +
                    "error=${error.javaClass.simpleName}:${error.message}",
            )
            saveEvidence(
                "g9-client-peer-rescue-fail",
                config.nodeId,
                listOf(
                    "role=client",
                    "default_path_result=FAILED",
                    "carrier=local_only_hotspot",
                    "bootstrap_sha256=$capsuleHash",
                    "result=FAIL",
                    "stage=peer_rescue",
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

    private fun probeDirectHttps(urlText: String): DirectProbe {
        val started = Instant.now()
        var connection: HttpsURLConnection? = null
        return try {
            connection = URL(urlText).openConnection() as HttpsURLConnection
            connection.connectTimeout = probeTimeoutMillis
            connection.readTimeout = probeTimeoutMillis
            connection.useCaches = false
            connection.instanceFollowRedirects = false
            connection.requestMethod = "HEAD"
            connection.setRequestProperty(
                "User-Agent",
                "sanpham3-g9-physical-court/1",
            )

            val code = connection.responseCode
            DirectProbe(
                url = urlText,
                startedAt = started,
                completedAt = Instant.now(),
                reachable = code in 100..599,
                responseCode = code,
                errorClass = null,
            )
        } catch (error: Throwable) {
            DirectProbe(
                url = urlText,
                startedAt = started,
                completedAt = Instant.now(),
                reachable = false,
                responseCode = null,
                errorClass = error.javaClass.simpleName,
            )
        } finally {
            connection?.disconnect()
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
            val downloadUri =
                publishEvidenceToDownloads(file.name, evidenceText)
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
