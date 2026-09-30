package dev.nolane.sanpham3.recoverylab

import android.app.Activity
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.util.Log
import dev.nolane.sanpham3.androidhost.AndroidG8PairCourt
import dev.nolane.sanpham3.androidhost.AndroidWifiDirectDataPath
import dev.nolane.sanpham3.androidhost.AndroidWifiDirectDataPathEvent
import dev.nolane.sanpham3.androidhost.AndroidWifiDirectDiscovery
import dev.nolane.sanpham3.androidhost.AndroidWifiDirectEvent
import dev.nolane.sanpham3.androidhost.AndroidWifiDirectPeer
import dev.nolane.sanpham3.androidhost.acceptPeerSession
import dev.nolane.sanpham3.androidhost.connectPeerSession
import java.io.File
import java.time.Instant
import java.util.concurrent.atomic.AtomicBoolean

class WifiDirectPairCourtActivity : Activity() {
    companion object {
        const val LOG_TAG = "SP3WifiDirect"
        private const val EXTRA_ROLE =
            "dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_ROLE"
        private const val EXTRA_NODE_ID =
            "dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_NODE_ID"
        private const val EXTRA_PSK =
            "dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_PSK"
        private const val EXTRA_PORT =
            "dev.nolane.sanpham3.recoverylab.WIFI_DIRECT_PORT"
        private const val DEFAULT_PORT = 47_117
        private const val SESSION_TIMEOUT_MS = 30_000
        private const val COURT_TIMEOUT_MS = 75_000L
    }

    private var discovery: AndroidWifiDirectDiscovery? = null
    private var dataPath: AndroidWifiDirectDataPath? = null
    private val completed = AtomicBoolean(false)
    private val groupFormedSeen = AtomicBoolean(false)
    private val peerSessionStarted = AtomicBoolean(false)

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Thread {
            Thread.sleep(COURT_TIMEOUT_MS)
            if (completed.compareAndSet(false, true)) {
                record("WIFI_DIRECT_PAIR_FAIL reason=court_timeout")
                runOnUiThread { finish() }
            }
        }.start()

        runCatching {
            val featureFlag = packageManager.hasSystemFeature(
                "android.hardware.wifi.direct",
            )
            record(
                "WIFI_DIRECT_CAPABILITY feature_flag=$featureFlag " +
                    "api=${Build.VERSION.SDK_INT}",
            )

            val missing = RecoveryLabPermissions
                .peerLanPermissions(Build.VERSION.SDK_INT)
                .filter {
                    checkSelfPermission(it) !=
                        PackageManager.PERMISSION_GRANTED
                }
            require(missing.isEmpty()) {
                "missing permissions: ${missing.joinToString(",")}"
            }

            val role = intent
                ?.getStringExtra(EXTRA_ROLE)
                ?.trim()
                ?.lowercase()
                ?: error("missing role")
            val nodeId = intent
                ?.getStringExtra(EXTRA_NODE_ID)
                ?.toLongOrNull()
                ?: error("invalid node id")
            val psk = LabCodec.parsePeerKey(
                intent?.getStringExtra(EXTRA_PSK)
                    ?: error("missing peer key"),
            )
            val port = intent?.getIntExtra(
                EXTRA_PORT,
                DEFAULT_PORT,
            ) ?: DEFAULT_PORT

            when (role) {
                "owner" -> startOwner(nodeId, psk, port)
                "client" -> startClient(nodeId, psk, port)
                else -> error("role must be owner or client")
            }
        }.onFailure { error ->
            fail(
                "startup_${error.javaClass.simpleName}",
                error.message,
            )
        }
    }

    private fun startOwner(
        nodeId: Long,
        peerKey: ByteArray,
        port: Int,
    ) {
        val path = AndroidWifiDirectDataPath(this)
        dataPath = path

        record(
            "WIFI_DIRECT_PAIR_START role=owner node_id=$nodeId " +
                "api=${Build.VERSION.SDK_INT} port=$port",
        )

        path.startGroupOwner(port) { event ->
            when (event) {
                is AndroidWifiDirectDataPathEvent.Connecting -> {
                    record("WIFI_DIRECT_GROUP_CREATING role=owner")
                }

                is AndroidWifiDirectDataPathEvent.GroupFormed -> {
                    groupFormedSeen.set(true)
                    if (!event.endpoint.groupOwner) {
                        fail(
                            "owner_role_lost",
                            "createGroup device is not group owner",
                        )
                        return@startGroupOwner
                    }

                    record(
                        "WIFI_DIRECT_GROUP_FORMED role=owner " +
                            "owner_address=" +
                            "${event.endpoint.groupOwnerAddress?.hostAddress ?: "-"}",
                    )

                    if (!peerSessionStarted.compareAndSet(
                            false,
                            true,
                        )
                    ) {
                        record(
                            "WIFI_DIRECT_GROUP_FORMED_DUPLICATE " +
                                "role=owner session_already_started=true",
                        )
                        return@startGroupOwner
                    }

                    Thread {
                        runCatching {
                            path.acceptPeerSession(
                                timeoutMillis = SESSION_TIMEOUT_MS,
                                nodeId = nodeId,
                                peerKey = peerKey,
                            ).use { session ->
                                val evidence =
                                    AndroidG8PairCourt.serveOnce(session)
                                pass(
                                    role = "owner",
                                    localNodeId = nodeId,
                                    peerNodeId = evidence.peerNodeId,
                                    challenge = evidence.challenge,
                                    groupOwner = true,
                                )
                            }
                        }.onFailure { error ->
                            fail(
                                "owner_session_${error.javaClass.simpleName}",
                                error.message,
                            )
                        }
                    }.start()
                }

                is AndroidWifiDirectDataPathEvent.Disconnected -> {
                    handleDisconnect("owner")
                }

                is AndroidWifiDirectDataPathEvent.Failed -> {
                    fail(
                        "owner_framework_${event.reasonCode ?: -1}",
                        event.detail,
                    )
                }
            }
        }
    }

    private fun startClient(
        nodeId: Long,
        peerKey: ByteArray,
        port: Int,
    ) {
        record(
            "WIFI_DIRECT_PAIR_START role=client node_id=$nodeId " +
                "api=${Build.VERSION.SDK_INT} port=$port",
        )

        val scan = AndroidWifiDirectDiscovery(this)
        discovery = scan
        scan.start { event ->
            when (event) {
                is AndroidWifiDirectEvent.DiscoveryStarted -> {
                    record("WIFI_DIRECT_DISCOVERY_STARTED role=client")
                }

                is AndroidWifiDirectEvent.State -> {
                    record(
                        "WIFI_DIRECT_STATE role=client enabled=${event.enabled}",
                    )
                }

                is AndroidWifiDirectEvent.Peers -> {
                    val peer = event.peers.firstOrNull()
                        ?: return@start
                    scan.close()
                    discovery = null
                    record(
                        "WIFI_DIRECT_PEER_FOUND role=client " +
                            "name=${sanitize(peer.deviceName)} " +
                            "address=${sanitize(peer.deviceAddressHint)}",
                    )
                    connectClient(
                        nodeId = nodeId,
                        peerKey = peerKey,
                        port = port,
                        peer = peer,
                    )
                }

                is AndroidWifiDirectEvent.Failed -> {
                    fail(
                        "client_discovery_${event.reasonCode ?: -1}",
                        event.detail,
                    )
                }
            }
        }
    }

    private fun connectClient(
        nodeId: Long,
        peerKey: ByteArray,
        port: Int,
        peer: AndroidWifiDirectPeer,
    ) {
        val path = AndroidWifiDirectDataPath(this)
        dataPath = path

        path.start(
            peer = peer,
            port = port,
            groupOwnerIntent = 0,
        ) { event ->
            when (event) {
                is AndroidWifiDirectDataPathEvent.Connecting -> {
                    record("WIFI_DIRECT_CONNECTING role=client")
                }

                is AndroidWifiDirectDataPathEvent.GroupFormed -> {
                    groupFormedSeen.set(true)
                    if (event.endpoint.groupOwner) {
                        fail(
                            "client_became_group_owner",
                            "expected existing AVD owner group",
                        )
                        return@start
                    }

                    val ownerAddress =
                        event.endpoint.groupOwnerAddress?.hostAddress
                    record(
                        "WIFI_DIRECT_GROUP_FORMED role=client " +
                            "owner_address=${ownerAddress ?: "-"}",
                    )

                    if (!peerSessionStarted.compareAndSet(
                            false,
                            true,
                        )
                    ) {
                        record(
                            "WIFI_DIRECT_GROUP_FORMED_DUPLICATE " +
                                "role=client session_already_started=true",
                        )
                        return@start
                    }

                    Thread {
                        runCatching {
                            path.connectPeerSession(
                                timeoutMillis = SESSION_TIMEOUT_MS,
                                nodeId = nodeId,
                                peerKey = peerKey,
                            ).use { session ->
                                val evidence =
                                    AndroidG8PairCourt.runClient(session)
                                pass(
                                    role = "client",
                                    localNodeId = nodeId,
                                    peerNodeId = evidence.peerNodeId,
                                    challenge = evidence.challenge,
                                    groupOwner = false,
                                )
                            }
                        }.onFailure { error ->
                            fail(
                                "client_session_${error.javaClass.simpleName}",
                                error.message,
                            )
                        }
                    }.start()
                }

                is AndroidWifiDirectDataPathEvent.Disconnected -> {
                    handleDisconnect("client")
                }

                is AndroidWifiDirectDataPathEvent.Failed -> {
                    fail(
                        "client_framework_${event.reasonCode ?: -1}",
                        event.detail,
                    )
                }
            }
        }
    }

    private fun handleDisconnect(role: String) {
        if (completed.get()) {
            return
        }

        if (!groupFormedSeen.get()) {
            record(
                "WIFI_DIRECT_DISCONNECTED_TRANSIENT role=$role " +
                    "phase=before_group_formed",
            )
            return
        }

        record(
            "WIFI_DIRECT_DISCONNECTED_TRANSIENT role=$role " +
                "phase=after_group_formed awaiting_g8=true",
        )
    }

    private fun pass(
        role: String,
        localNodeId: Long,
        peerNodeId: Long,
        challenge: ByteArray,
        groupOwner: Boolean,
    ) {
        if (!completed.compareAndSet(false, true)) {
            return
        }

        record(
            "WIFI_DIRECT_PAIR_PASS role=$role " +
                "local_node=$localNodeId peer_node=$peerNodeId " +
                "group_owner=$groupOwner " +
                "challenge=${LabCodec.hex(challenge)} " +
                "evidence_level=ANDROID_AVD",
        )
        runOnUiThread { finish() }
    }

    private fun fail(
        reason: String,
        detail: String?,
    ) {
        if (!completed.compareAndSet(false, true)) {
            return
        }

        record(
            "WIFI_DIRECT_PAIR_FAIL reason=${sanitize(reason)} " +
                "detail=${sanitize(detail)}",
        )
        runOnUiThread { finish() }
    }

    private fun record(message: String) {
        val line = buildString {
            append(Instant.now())
            append(" git=")
            append(BuildConfig.GIT_SHA)
            append(' ')
            append(message)
        }
        Log.i(LOG_TAG, line)

        runCatching {
            val base = getExternalFilesDir(null) ?: filesDir
            val directory = File(base, "wifi-direct-pair")
            check(directory.exists() || directory.mkdirs())
            File(directory, "latest.txt").writeText(line + "\n")
        }.onFailure { error ->
            Log.e(
                LOG_TAG,
                "WIFI_DIRECT_EVIDENCE_FAIL " +
                    "reason=${error.javaClass.simpleName}",
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

    override fun onDestroy() {
        discovery?.close()
        discovery = null
        dataPath?.close()
        dataPath = null
        super.onDestroy()
    }
}
