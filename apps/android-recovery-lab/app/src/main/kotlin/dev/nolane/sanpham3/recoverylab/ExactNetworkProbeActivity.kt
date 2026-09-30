package dev.nolane.sanpham3.recoverylab

import android.app.Activity
import android.content.Context
import android.net.ConnectivityManager
import android.net.Network
import android.os.Build
import android.os.Bundle
import android.util.Log
import dev.nolane.sanpham3.androidhost.AndroidBoundDnsProbe
import java.io.File
import java.time.Instant

class ExactNetworkProbeActivity : Activity() {
    companion object {
        const val EXTRA_NETWORK_HANDLE =
            "dev.nolane.sanpham3.recoverylab.NETWORK_HANDLE"
        const val LOG_TAG = "SP3ExactNetwork"
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        Thread {
            runCourt()
            runOnUiThread { finish() }
        }.start()
    }

    private fun runCourt() {
        val localNetworkPermission =
            RecoveryLabPermissions.localNetworkPermissionState(
                this,
                Build.VERSION.SDK_INT,
            )
        record(
            "EXACT_NETWORK_PERMISSION " +
                "local_network_permission=$localNetworkPermission " +
                "dns_port53_exception=true",
        )

        val connectivity =
            getSystemService(Context.CONNECTIVITY_SERVICE)
                as ConnectivityManager

        val requestedHandle = intent
            ?.getStringExtra(EXTRA_NETWORK_HANDLE)
            ?.toLongOrNull()

        val network = chooseNetwork(
            connectivity.allNetworks.toList(),
            connectivity.activeNetwork,
            requestedHandle,
        )

        if (network == null) {
            record(
                "EXACT_NETWORK_FAIL reason=no_network " +
                    "requested_handle=${requestedHandle ?: "default"}",
            )
            return
        }

        val link = connectivity.getLinkProperties(network)
        val resolver = link?.dnsServers?.firstOrNull()
        if (resolver == null) {
            record(
                "EXACT_NETWORK_FAIL reason=no_dns " +
                    "network_handle=${network.networkHandle}",
            )
            return
        }

        try {
            val result = AndroidBoundDnsProbe(
                timeoutMillis = 2_000,
            ).probe(network, resolver)

            check(result.networkHandle == network.networkHandle) {
                "probe result handle diverged from selected Network"
            }

            record(
                "EXACT_NETWORK_PASS " +
                    "network_handle=${network.networkHandle} " +
                    "result_handle=${result.networkHandle} " +
                    "resolver=${result.resolver} " +
                    "response_bytes=${result.responseBytes} " +
                    "rcode=${result.rcode} " +
                    "default_network=${connectivity.activeNetwork == network} " +
                    "local_network_permission=$localNetworkPermission " +
                    "dns_port53_exception=true",
            )
        } catch (error: Throwable) {
            record(
                "EXACT_NETWORK_FAIL " +
                    "network_handle=${network.networkHandle} " +
                    "reason=${error.javaClass.simpleName} " +
                    "detail=${sanitize(error.message)}",
            )
        }
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
            val directory = File(base, "exact-network")
            check(directory.exists() || directory.mkdirs())
            File(directory, "latest.txt").writeText(line + "\n")
        }.onFailure { error ->
            Log.e(
                LOG_TAG,
                "EXACT_NETWORK_EVIDENCE_FAIL " +
                    "reason=${error.javaClass.simpleName}",
            )
        }
    }

    private fun sanitize(value: String?): String =
        value
            ?.replace('\n', ' ')
            ?.replace('\r', ' ')
            ?.take(240)
            ?: "-"

    internal fun chooseNetwork(
        networks: List<Network>,
        activeNetwork: Network?,
        requestedHandle: Long?,
    ): Network? {
        if (requestedHandle != null) {
            return networks.firstOrNull {
                it.networkHandle == requestedHandle
            }
        }
        return activeNetwork ?: networks.firstOrNull()
    }
}
