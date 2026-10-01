package dev.nolane.sanpham3.recoverylab

import android.app.Activity
import android.content.Context
import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioTrack
import android.os.Build
import android.os.Bundle
import android.os.VibrationEffect
import android.os.Vibrator
import android.os.VibratorManager
import android.util.Log
import java.io.File
import java.time.Instant
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.PI
import kotlin.math.roundToInt
import kotlin.math.sin

internal object PhysicalSignalEmitterCodec {
    const val acousticSampleRateHz = 48_000
    const val acousticBitRateBps = 50
    const val acousticSamplesPerBit =
        acousticSampleRateHz / acousticBitRateBps
    const val acousticZeroHz = 18_500.0
    const val acousticOneHz = 19_500.0
    const val acousticAmplitude = 0.65

    const val vibrationBitDurationMs = 400L
    const val vibrationEnvelopeHz = 35.0
    const val vibrationAmplitude = 0.75
    private const val vibrationHalfPeriodMs = 14L

    fun bitsFromHex(value: String): ByteArray {
        val bytes = LabCodec.parseHex(value)
        require(bytes.isNotEmpty()) {
            "payload hex must contain at least one byte"
        }

        val bits = ByteArray(bytes.size * 8)
        var cursor = 0
        for (byte in bytes) {
            val unsigned = byte.toInt() and 0xff
            for (shift in 7 downTo 0) {
                bits[cursor++] =
                    ((unsigned ushr shift) and 1).toByte()
            }
        }
        return bits
    }

    fun acousticPcm16(bits: ByteArray): ShortArray {
        require(bits.isNotEmpty())
        require(bits.all { it.toInt() == 0 || it.toInt() == 1 })

        val output = ShortArray(bits.size * acousticSamplesPerBit)
        var cursor = 0
        for (bit in bits) {
            val frequency = if (bit.toInt() == 0) {
                acousticZeroHz
            } else {
                acousticOneHz
            }

            for (sampleIndex in 0 until acousticSamplesPerBit) {
                val time =
                    sampleIndex.toDouble() /
                        acousticSampleRateHz.toDouble()
                val normalized =
                    acousticAmplitude *
                        sin(2.0 * PI * frequency * time)
                output[cursor++] =
                    (normalized * Short.MAX_VALUE.toDouble())
                        .roundToInt()
                        .coerceIn(
                            Short.MIN_VALUE.toInt(),
                            Short.MAX_VALUE.toInt(),
                        )
                        .toShort()
            }
        }
        return output
    }

    data class VibrationPattern(
        val timingsMs: LongArray,
        val amplitudes: IntArray,
    ) {
        val durationMs: Long
            get() = timingsMs.sum()
    }

    fun vibrationPattern(bits: ByteArray): VibrationPattern {
        require(bits.isNotEmpty())
        require(bits.all { it.toInt() == 0 || it.toInt() == 1 })

        val timings = mutableListOf<Long>()
        val amplitudes = mutableListOf<Int>()
        val highAmplitude =
            (vibrationAmplitude * 255.0)
                .roundToInt()
                .coerceIn(1, 255)

        for (bit in bits) {
            if (bit.toInt() == 0) {
                timings += vibrationBitDurationMs
                amplitudes += 0
                continue
            }

            var remaining = vibrationBitDurationMs
            var high = true
            while (remaining > 0L) {
                val slice =
                    minOf(vibrationHalfPeriodMs, remaining)
                timings += slice
                amplitudes += if (high) highAmplitude else 0
                high = !high
                remaining -= slice
            }
        }

        return VibrationPattern(
            timingsMs = timings.toLongArray(),
            amplitudes = amplitudes.toIntArray(),
        )
    }
}

class PhysicalSignalEmitterActivity : Activity() {
    companion object {
        const val LOG_TAG = "SP3SignalEmit"
        const val EXTRA_MODE =
            "dev.nolane.sanpham3.recoverylab.SIGNAL_EMIT_MODE"
        const val EXTRA_HEX =
            "dev.nolane.sanpham3.recoverylab.SIGNAL_EMIT_HEX"
        const val EXTRA_DELAY_MS =
            "dev.nolane.sanpham3.recoverylab.SIGNAL_EMIT_DELAY_MS"

        private const val defaultDelayMs = 1_000
        private const val maxDelayMs = 10_000
        private const val maxAudioBytes = 64
        private const val maxVibrationBytes = 8
    }

    private val finished = AtomicBoolean(false)
    private var audioTrack: AudioTrack? = null
    private var vibrator: Vibrator? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val mode = intent
            ?.getStringExtra(EXTRA_MODE)
            ?.trim()
            ?.lowercase()
            ?: "audio"
        val hex = intent
            ?.getStringExtra(EXTRA_HEX)
            ?.trim()
            ?: ""
        val delayMs = intent?.getIntExtra(
            EXTRA_DELAY_MS,
            defaultDelayMs,
        ) ?: defaultDelayMs

        if (delayMs !in 0..maxDelayMs) {
            fail(
                mode,
                "invalid_delay",
                "delay_ms=$delayMs",
            )
            return
        }

        runCatching {
            val bits =
                PhysicalSignalEmitterCodec.bitsFromHex(hex)
            val payloadBytes = bits.size / 8

            when (mode) {
                "audio" -> {
                    require(payloadBytes <= maxAudioBytes) {
                        "audio payload exceeds $maxAudioBytes bytes"
                    }
                    emitAudio(bits, hex.lowercase(), delayMs)
                }

                "vibration" -> {
                    require(payloadBytes <= maxVibrationBytes) {
                        "vibration payload exceeds $maxVibrationBytes bytes"
                    }
                    emitVibration(bits, hex.lowercase(), delayMs)
                }

                else -> error(
                    "mode must be audio or vibration",
                )
            }
        }.onFailure { error ->
            fail(
                mode,
                "setup_${error.javaClass.simpleName}",
                error.message,
            )
        }
    }

    override fun onDestroy() {
        runCatching { audioTrack?.stop() }
        audioTrack?.release()
        audioTrack = null
        runCatching { vibrator?.cancel() }
        vibrator = null
        super.onDestroy()
    }

    private fun emitAudio(
        bits: ByteArray,
        hex: String,
        delayMs: Int,
    ) {
        Thread {
            runCatching {
                if (delayMs > 0) {
                    Thread.sleep(delayMs.toLong())
                }

                val pcm =
                    PhysicalSignalEmitterCodec.acousticPcm16(
                        bits,
                    )
                val track = AudioTrack.Builder()
                    .setAudioAttributes(
                        AudioAttributes.Builder()
                            .setUsage(
                                AudioAttributes.USAGE_MEDIA,
                            )
                            .setContentType(
                                AudioAttributes
                                    .CONTENT_TYPE_SONIFICATION,
                            )
                            .build(),
                    )
                    .setAudioFormat(
                        AudioFormat.Builder()
                            .setSampleRate(
                                PhysicalSignalEmitterCodec
                                    .acousticSampleRateHz,
                            )
                            .setEncoding(
                                AudioFormat
                                    .ENCODING_PCM_16BIT,
                            )
                            .setChannelMask(
                                AudioFormat.CHANNEL_OUT_MONO,
                            )
                            .build(),
                    )
                    .setTransferMode(AudioTrack.MODE_STATIC)
                    .setBufferSizeInBytes(pcm.size * 2)
                    .build()

                check(track.state ==
                    AudioTrack.STATE_INITIALIZED) {
                    "AudioTrack failed to initialize"
                }

                audioTrack = track
                val written = track.write(
                    pcm,
                    0,
                    pcm.size,
                    AudioTrack.WRITE_BLOCKING,
                )
                check(written == pcm.size) {
                    "AudioTrack wrote $written / ${pcm.size} samples"
                }

                val signalDurationMs =
                    bits.size * 1_000L /
                        PhysicalSignalEmitterCodec
                            .acousticBitRateBps
                record(
                    "PHYSICAL_SIGNAL_EMIT_START mode=audio " +
                        "bits=${bits.size} payload_hex=$hex " +
                        "sample_rate_hz=" +
                        "${PhysicalSignalEmitterCodec.acousticSampleRateHz} " +
                        "zero_hz=" +
                        "${PhysicalSignalEmitterCodec.acousticZeroHz} " +
                        "one_hz=" +
                        "${PhysicalSignalEmitterCodec.acousticOneHz} " +
                        "amplitude=" +
                        "${PhysicalSignalEmitterCodec.acousticAmplitude} " +
                        "delay_ms=$delayMs",
                )

                track.play()
                Thread.sleep(signalDurationMs + 200L)
                if (track.playState ==
                    AudioTrack.PLAYSTATE_PLAYING
                ) {
                    track.stop()
                }

                pass(
                    mode = "audio",
                    hex = hex,
                    bits = bits.size,
                    durationMs = signalDurationMs,
                    detail =
                        "pcm_samples=${pcm.size} " +
                            "sample_rate_hz=" +
                            "${PhysicalSignalEmitterCodec.acousticSampleRateHz}",
                )
            }.onFailure { error ->
                fail(
                    "audio",
                    "emit_${error.javaClass.simpleName}",
                    error.message,
                )
            }
        }.start()
    }

    private fun emitVibration(
        bits: ByteArray,
        hex: String,
        delayMs: Int,
    ) {
        Thread {
            runCatching {
                if (delayMs > 0) {
                    Thread.sleep(delayMs.toLong())
                }

                val pattern =
                    PhysicalSignalEmitterCodec
                        .vibrationPattern(bits)
                val effect = VibrationEffect.createWaveform(
                    pattern.timingsMs,
                    pattern.amplitudes,
                    -1,
                )

                val target = if (
                    Build.VERSION.SDK_INT >=
                    Build.VERSION_CODES.S
                ) {
                    getSystemService(
                        VibratorManager::class.java,
                    ).defaultVibrator
                } else {
                    @Suppress("DEPRECATION")
                    getSystemService(
                        Context.VIBRATOR_SERVICE,
                    ) as Vibrator
                }
                check(target.hasVibrator()) {
                    "device has no vibrator"
                }
                vibrator = target

                record(
                    "PHYSICAL_SIGNAL_EMIT_START mode=vibration " +
                        "bits=${bits.size} payload_hex=$hex " +
                        "bit_duration_ms=" +
                        "${PhysicalSignalEmitterCodec.vibrationBitDurationMs} " +
                        "envelope_hz=" +
                        "${PhysicalSignalEmitterCodec.vibrationEnvelopeHz} " +
                        "amplitude=" +
                        "${PhysicalSignalEmitterCodec.vibrationAmplitude} " +
                        "delay_ms=$delayMs",
                )

                target.vibrate(effect)
                Thread.sleep(pattern.durationMs + 200L)
                target.cancel()

                pass(
                    mode = "vibration",
                    hex = hex,
                    bits = bits.size,
                    durationMs = pattern.durationMs,
                    detail =
                        "segments=${pattern.timingsMs.size} " +
                            "android_haptic_envelope_approximation=true",
                )
            }.onFailure { error ->
                fail(
                    "vibration",
                    "emit_${error.javaClass.simpleName}",
                    error.message,
                )
            }
        }.start()
    }

    private fun pass(
        mode: String,
        hex: String,
        bits: Int,
        durationMs: Long,
        detail: String,
    ) {
        if (!finished.compareAndSet(false, true)) {
            return
        }

        val message =
            "PHYSICAL_SIGNAL_EMIT_PASS mode=$mode " +
                "payload_hex=$hex bits=$bits " +
                "signal_duration_ms=$durationMs " +
                "$detail " +
                "evidence_level=ANDROID_RUNTIME_EMIT"
        record(message)
        saveEvidence(message)
        runOnUiThread { finish() }
    }

    private fun fail(
        mode: String,
        reason: String,
        detail: String?,
    ) {
        if (!finished.compareAndSet(false, true)) {
            return
        }

        val message =
            "PHYSICAL_SIGNAL_EMIT_FAIL mode=$mode " +
                "reason=${sanitize(reason)} " +
                "detail=${sanitize(detail)}"
        record(message)
        saveEvidence(message)
        runOnUiThread { finish() }
    }

    private fun record(message: String) {
        Log.i(
            LOG_TAG,
            "${Instant.now()} git=${BuildConfig.GIT_SHA} $message",
        )
    }

    private fun saveEvidence(message: String) {
        runCatching {
            val base = getExternalFilesDir(null) ?: filesDir
            val directory = File(
                base,
                "signal-emitter",
            )
            check(
                directory.exists() || directory.mkdirs(),
            )
            File(directory, "latest.txt").writeText(
                buildString {
                    appendLine(
                        "timestamp_utc=${Instant.now()}",
                    )
                    appendLine(
                        "git_commit=${BuildConfig.GIT_SHA}",
                    )
                    appendLine(
                        "device=${Build.MANUFACTURER} ${Build.MODEL}",
                    )
                    appendLine(
                        "android_sdk=${Build.VERSION.SDK_INT}",
                    )
                    appendLine(message)
                },
            )
        }.onFailure { error ->
            Log.e(
                LOG_TAG,
                "SIGNAL_EMIT_EVIDENCE_FAIL " +
                    "reason=${error.javaClass.simpleName}",
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
