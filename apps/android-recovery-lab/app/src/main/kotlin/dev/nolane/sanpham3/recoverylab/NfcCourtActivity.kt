package dev.nolane.sanpham3.recoverylab

import android.app.Activity
import android.content.Context
import android.content.pm.PackageManager
import android.nfc.NfcManager
import android.os.Bundle
import android.text.InputType
import android.text.method.PasswordTransformationMethod
import android.view.ViewGroup
import android.widget.Button
import android.widget.EditText
import android.widget.LinearLayout
import android.widget.ScrollView
import android.widget.TextView
import dev.nolane.sanpham3.androidhost.AndroidNfcG8Reader
import dev.nolane.sanpham3.androidhost.AndroidNfcHceCourt
import dev.nolane.sanpham3.androidhost.AndroidNfcReaderEvent
import java.io.File
import java.security.SecureRandom
import java.time.Instant
import java.time.ZoneOffset
import java.time.format.DateTimeFormatter
import java.util.concurrent.Executors
import java.util.concurrent.atomic.AtomicBoolean

class NfcCourtActivity : Activity() {
    companion object {
        private val evidenceStamp = DateTimeFormatter
            .ofPattern("yyyyMMdd'T'HHmmss'Z'")
            .withZone(ZoneOffset.UTC)
        private const val HCE_TIMEOUT_MS = 120_000L
    }

    private data class CourtConfig(
        val nodeId: Long,
        val peerKey: ByteArray,
    )

    private val worker = Executors.newSingleThreadExecutor()
    private val hcePolling = AtomicBoolean(false)
    private val transcript = StringBuilder()

    private lateinit var nodeIdInput: EditText
    private lateinit var pskInput: EditText
    private lateinit var logView: TextView
    private lateinit var logScroll: ScrollView

    @Volatile
    private var reader: AndroidNfcG8Reader? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(buildUi())

        appendLog("NFC_COURT_START git=${BuildConfig.GIT_SHA}")
        appendLog(
            "DEVICE manufacturer=${android.os.Build.MANUFACTURER} " +
                "model=${android.os.Build.MODEL} " +
                "sdk=${android.os.Build.VERSION.SDK_INT}",
        )
        appendLog("No peer key is written to evidence.")
        showCapability()
    }

    override fun onDestroy() {
        stopCourt("activity_destroyed")
        worker.shutdownNow()
        super.onDestroy()
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
            text = "SP3 NFC HCE / Reader G8 Court"
            textSize = 20f
        })

        root.addView(TextView(this).apply {
            text =
                "Two physical Android devices. Same PSK. " +
                    "One HCE role, one Reader role."
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
            generatePsk()
        })
        root.addView(button("Show capability") {
            showCapability()
        })
        root.addView(button("Configure HCE role") {
            startHceRole()
        })
        root.addView(button("Start Reader role") {
            startReaderRole()
        })
        root.addView(button("Stop NFC court") {
            stopCourt("user_stop")
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

    private fun generatePsk() {
        val bytes = ByteArray(32)
        SecureRandom().nextBytes(bytes)
        val encoded = LabCodec.hex(bytes)
        bytes.fill(0)
        pskInput.setText(encoded)
        pskInput.setSelection(encoded.length)
        appendLog("NFC_PSK_GENERATED bytes=32")
    }

    private fun readConfig(): CourtConfig? =
        try {
            val nodeId = nodeIdInput.text.toString().trim().toLong()
            require(nodeId >= 0) {
                "node ID must be non-negative"
            }
            val key = LabCodec.parsePeerKey(
                pskInput.text.toString(),
            )
            CourtConfig(nodeId, key)
        } catch (error: Exception) {
            appendLog("NFC_CONFIG_FAIL detail=${sanitize(error.message)}")
            null
        }

    private fun showCapability() {
        val manager = getSystemService(Context.NFC_SERVICE) as? NfcManager
        val adapter = manager?.defaultAdapter
        val nfc = packageManager.hasSystemFeature(
            PackageManager.FEATURE_NFC,
        )
        val hce = packageManager.hasSystemFeature(
            PackageManager.FEATURE_NFC_HOST_CARD_EMULATION,
        )
        appendLog(
            "NFC_CAPABILITY hardware=$nfc hce=$hce " +
                "adapter_present=${adapter != null} " +
                "enabled=${adapter?.isEnabled == true}",
        )
    }

    private fun requireNfc(
        requireHce: Boolean,
    ) {
        require(
            packageManager.hasSystemFeature(
                PackageManager.FEATURE_NFC,
            ),
        ) {
            "NFC hardware feature missing"
        }
        if (requireHce) {
            require(
                packageManager.hasSystemFeature(
                    PackageManager.FEATURE_NFC_HOST_CARD_EMULATION,
                ),
            ) {
                "NFC HCE feature missing"
            }
        }
        val adapter =
            (getSystemService(Context.NFC_SERVICE) as NfcManager)
                .defaultAdapter
        require(adapter.isEnabled) {
            "NFC adapter is disabled"
        }
    }

    private fun startHceRole() {
        val config = readConfig() ?: return
        stopCourt("replace_with_hce")

        runCatching {
            requireNfc(requireHce = true)
            AndroidNfcHceCourt.configure(
                nodeId = config.nodeId,
                peerKey = config.peerKey,
            )
            config.peerKey.fill(0)
            appendLog(
                "NFC_HCE_READY local_node=${config.nodeId} " +
                    "benchmark_rounds=${AndroidNfcG8Reader.BENCHMARK_ROUNDS}",
            )
            pollHceEvidence(config.nodeId)
        }.onFailure { error ->
            config.peerKey.fill(0)
            appendLog(
                "NFC_HCE_FAIL detail=${sanitize(error.message)}",
            )
            saveEvidence(
                label = "nfc-hce-fail",
                nodeId = config.nodeId,
                fields = listOf(
                    "role=hce",
                    "carrier=nfc_hce",
                    "result=FAIL",
                    "error=${error.javaClass.simpleName}",
                ),
            )
        }
    }

    private fun pollHceEvidence(
        nodeId: Long,
    ) {
        if (!hcePolling.compareAndSet(false, true)) {
            return
        }

        worker.execute {
            val deadline = System.nanoTime() +
                HCE_TIMEOUT_MS * 1_000_000L
            try {
                while (
                    hcePolling.get() &&
                    System.nanoTime() < deadline
                ) {
                    val evidence = AndroidNfcHceCourt.evidence()
                    if (
                        evidence != null &&
                        evidence.lastChallenge != null &&
                        evidence.benchmarkFramesCompleted >=
                            AndroidNfcG8Reader.BENCHMARK_ROUNDS
                    ) {
                        val challenge =
                            LabCodec.hex(evidence.lastChallenge)
                        appendLog(
                            "NFC_HCE_PASS local_node=$nodeId " +
                                "peer_node=${evidence.authenticatedPeerNodeId} " +
                                "benchmark_frames=" +
                                "${evidence.benchmarkFramesCompleted} " +
                                "challenge=$challenge",
                        )
                        saveEvidence(
                            label = "nfc-hce-pass",
                            nodeId = nodeId,
                            fields = listOf(
                                "role=hce",
                                "carrier=nfc_hce",
                                "authenticated_peer_node=" +
                                    evidence.authenticatedPeerNodeId,
                                "challenge_hex=$challenge",
                                "benchmark_frames_completed=" +
                                    evidence.benchmarkFramesCompleted,
                                "result=PASS",
                            ),
                        )
                        return@execute
                    }
                    Thread.sleep(100)
                }

                appendLog("NFC_HCE_FAIL detail=timeout")
                saveEvidence(
                    label = "nfc-hce-timeout",
                    nodeId = nodeId,
                    fields = listOf(
                        "role=hce",
                        "carrier=nfc_hce",
                        "result=FAIL",
                        "error=timeout",
                    ),
                )
            } finally {
                hcePolling.set(false)
            }
        }
    }

    private fun startReaderRole() {
        val config = readConfig() ?: return
        stopCourt("replace_with_reader")

        runCatching {
            requireNfc(requireHce = false)
            val court = AndroidNfcG8Reader(
                activity = this,
                nodeId = config.nodeId,
                peerKey = config.peerKey,
            )
            config.peerKey.fill(0)
            reader = court

            court.start { event ->
                when (event) {
                    AndroidNfcReaderEvent.Started -> {
                        appendLog(
                            "NFC_READER_READY local_node=${config.nodeId} " +
                                "tap_hce_device=true",
                        )
                    }

                    is AndroidNfcReaderEvent.Passed -> {
                        val evidence = event.evidence
                        val benchmark = evidence.benchmark
                        val challenge =
                            LabCodec.hex(evidence.challenge)

                        appendLog(
                            "NFC_READER_PASS local_node=${config.nodeId} " +
                                "peer_node=" +
                                "${evidence.authenticatedPeerNodeId} " +
                                "rounds=${benchmark.rounds} " +
                                "payload_bytes=${benchmark.payloadBytes} " +
                                "median_rtt_ms=" +
                                "${nanosToMillis(benchmark.medianRttNanos)} " +
                                "p95_rtt_ms=" +
                                "${nanosToMillis(benchmark.p95RttNanos)} " +
                                "useful_bps=" +
                                "${formatRate(benchmark.roundTripUsefulBitsPerSecond)}",
                        )

                        saveEvidence(
                            label = "nfc-reader-pass",
                            nodeId = config.nodeId,
                            fields = listOf(
                                "role=reader",
                                "carrier=nfc_hce",
                                "authenticated_peer_node=" +
                                    evidence.authenticatedPeerNodeId,
                                "challenge_hex=$challenge",
                                "max_transceive_length=" +
                                    evidence.maxTransceiveLength,
                                "benchmark_rounds=${benchmark.rounds}",
                                "benchmark_payload_bytes=" +
                                    benchmark.payloadBytes,
                                "benchmark_elapsed_ns=" +
                                    benchmark.elapsedNanos,
                                "min_rtt_ns=${benchmark.minRttNanos}",
                                "median_rtt_ns=" +
                                    benchmark.medianRttNanos,
                                "p95_rtt_ns=${benchmark.p95RttNanos}",
                                "max_rtt_ns=${benchmark.maxRttNanos}",
                                "one_way_useful_bytes=" +
                                    benchmark.oneWayUsefulBytes,
                                "round_trip_useful_bytes=" +
                                    benchmark.roundTripUsefulBytes,
                                "one_way_useful_bps=" +
                                    benchmark.oneWayUsefulBitsPerSecond,
                                "round_trip_useful_bps=" +
                                    benchmark.roundTripUsefulBitsPerSecond,
                                "result=PASS",
                            ),
                        )
                    }

                    is AndroidNfcReaderEvent.Failed -> {
                        appendLog(
                            "NFC_READER_FAIL detail=" +
                                sanitize(event.detail),
                        )
                        saveEvidence(
                            label = "nfc-reader-fail",
                            nodeId = config.nodeId,
                            fields = listOf(
                                "role=reader",
                                "carrier=nfc_hce",
                                "result=FAIL",
                                "detail=${sanitize(event.detail)}",
                            ),
                        )
                    }
                }
            }
        }.onFailure { error ->
            config.peerKey.fill(0)
            appendLog(
                "NFC_READER_FAIL detail=${sanitize(error.message)}",
            )
            saveEvidence(
                label = "nfc-reader-start-fail",
                nodeId = config.nodeId,
                fields = listOf(
                    "role=reader",
                    "carrier=nfc_hce",
                    "result=FAIL",
                    "error=${error.javaClass.simpleName}",
                ),
            )
        }
    }

    private fun stopCourt(reason: String) {
        hcePolling.set(false)
        reader?.destroy()
        reader = null
        AndroidNfcHceCourt.clear()
        appendLog("NFC_STOP reason=$reason")
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
            val directory = File(base, "nfc-court")
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
                        "device=${android.os.Build.MANUFACTURER} " +
                            android.os.Build.MODEL,
                    )
                    appendLine(
                        "android_sdk=${android.os.Build.VERSION.SDK_INT}",
                    )
                    appendLine("local_node_id=$nodeId")
                    for (field in fields) appendLine(field)
                    appendLine()
                    appendLine("--- transcript ---")
                    append(snapshot)
                },
            )
            appendLog("NFC_EVIDENCE saved=${file.absolutePath}")
        }.onFailure { error ->
            appendLog(
                "NFC_EVIDENCE_FAIL detail=" +
                    sanitize(error.message),
            )
        }
    }

    private fun sanitize(value: String?): String =
        value
            ?.replace('\n', ' ')
            ?.replace('\r', ' ')
            ?.replace(' ', '_')
            ?.take(240)
            ?: "-"

    private fun nanosToMillis(value: Long): String =
        "%.3f".format(value / 1_000_000.0)

    private fun formatRate(value: Double): String =
        "%.1f".format(value)
}
