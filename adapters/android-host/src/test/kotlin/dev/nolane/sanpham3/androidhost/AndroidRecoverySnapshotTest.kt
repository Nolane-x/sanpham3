package dev.nolane.sanpham3.androidhost

import org.junit.Assert.assertFalse
import org.junit.Assert.assertTrue
import org.junit.Test

class AndroidRecoverySnapshotTest {
    @Test
    fun configurationSuccessAloneIsNotInternetEvidence() {
        val snapshot = AndroidRecoverySnapshot(
            networks = emptyList(),
            probes = listOf(
                AndroidProbeRecord(
                    id = "1:ipv4:configured",
                    kind = AndroidProbeKind.IPV4,
                    status = AndroidProbeStatus.SUCCEEDED,
                    detail = "configured=true",
                ),
            ),
        )

        assertFalse(snapshot.informationPathFound)
    }

    @Test
    fun successfulTinyHttpsExchangeIsInformationPathEvidence() {
        val snapshot = AndroidRecoverySnapshot(
            networks = emptyList(),
            probes = listOf(
                AndroidProbeRecord(
                    id = "1:https:example.com",
                    kind = AndroidProbeKind.TINY_HTTPS,
                    status = AndroidProbeStatus.SUCCEEDED,
                    detail = "useful_bytes=128",
                ),
            ),
        )

        assertTrue(snapshot.informationPathFound)
    }

    @Test
    fun successfulDnsExchangeIsInformationPathEvidence() {
        val snapshot = AndroidRecoverySnapshot(
            networks = emptyList(),
            probes = listOf(
                AndroidProbeRecord(
                    id = "1:dns:8.8.8.8",
                    kind = AndroidProbeKind.DNS,
                    status = AndroidProbeStatus.SUCCEEDED,
                    detail = "bytes=17",
                ),
            ),
        )

        assertTrue(snapshot.informationPathFound)
    }
}
