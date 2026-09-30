package dev.nolane.sanpham3.recoverylab

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.pm.PackageManager
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.media.AudioFormat
import android.media.AudioRecord
import android.media.MediaRecorder
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.os.SystemClock
import android.util.Log
import java.io.BufferedWriter
import java.io.File
import java.io.FileOutputStream
import java.io.OutputStreamWriter
import java.nio.ByteBuffer
import java.nio.ByteOrder
import java.time.Instant
import java.util.Locale
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.sqrt

class RecordedTraceCaptureActivity : Activity() {
    companion object {
        const val LOG_TAG = "SP3TraceCapture"
        const val EXTRA_MODE =
            "dev.nolane.sanpham3.recoverylab.TRACE_CAPTURE_MODE"
        const val EXTRA_DURATION_MS =
            "dev.nolane.sanpham3.recoverylab.TRACE_CAPTURE_DURATION_MS"

        private const val AUDIO_SAMPLE_RATE_HZ = 48_000
        private const val ACCEL_SAMPLE_RATE_HZ = 200
        private const val ACCEL_PERIOD_US = 1_000_000 / ACCEL_SAMPLE_RATE_HZ
        private const val DEFAULT_DURATION_MS = 4_000
        private const val MIN_DURATION_MS = 250
        private const val MAX_DURATION_MS = 30_000
    }

    private val finished = AtomicBoolean(false)
    private var workerThread: HandlerThread? = null
    private var sensorManager: SensorManager? = null
    private var sensorListener: SensorEventListener? = null

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val mode = intent
            ?.getStringExtra(EXTRA_MODE)
            ?.trim()
            ?.lowercase()
            ?: "audio"
        val durationMs = intent?.getIntExtra(
            EXTRA_DURATION_MS,
            DEFAULT_DURATION_MS,
        ) ?: DEFAULT_DURATION_MS

        if (durationMs !in MIN_DURATION_MS..MAX_DURATION_MS) {
            fail(
                mode = mode,
                reason = "invalid_duration",
                detail = "duration_ms=$durationMs",
            )
            return
        }

        when (mode) {
            "audio" -> captureAudio(durationMs)
            "accelerometer" -> captureAccelerometer(durationMs)
            else -> fail(
                mode = mode,
                reason = "invalid_mode",
                detail = "expected audio or accelerometer",
            )
        }
    }

    private fun captureAudio(durationMs: Int) {
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            fail(
                mode = "audio",
                reason = "record_audio_permission_missing",
                detail = null,
            )
            return
        }

        Thread {
            var recorder: AudioRecord? = null
            runCatching {
                val minBuffer = AudioRecord.getMinBufferSize(
                    AUDIO_SAMPLE_RATE_HZ,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT,
                )
                check(minBuffer > 0) {
                    "AudioRecord reports invalid minimum buffer $minBuffer"
                }

                val bufferBytes = maxOf(minBuffer, 4096)
                recorder = AudioRecord(
                    MediaRecorder.AudioSource.MIC,
                    AUDIO_SAMPLE_RATE_HZ,
                    AudioFormat.CHANNEL_IN_MONO,
                    AudioFormat.ENCODING_PCM_16BIT,
                    bufferBytes,
                )
                check(recorder?.state == AudioRecord.STATE_INITIALIZED) {
                    "AudioRecord failed to initialize"
                }

                val directory = traceDirectory()
                val file = File(directory, "latest-audio.wav")
                val targetSamples =
                    AUDIO_SAMPLE_RATE_HZ.toLong() * durationMs / 1000L

                FileOutputStream(file, false).use { stream ->
                    stream.write(ByteArray(44))
                    recorder?.startRecording()

                    val buffer = ByteArray(bufferBytes)
                    var samplesWritten = 0L
                    while (samplesWritten < targetSamples) {
                        val remainingSamples =
                            (targetSamples - samplesWritten)
                                .coerceAtMost((buffer.size / 2).toLong())
                                .toInt()
                        val requestedBytes = remainingSamples * 2
                        val read = recorder?.read(
                            buffer,
                            0,
                            requestedBytes,
                        ) ?: AudioRecord.ERROR_INVALID_OPERATION

                        if (read < 0) {
                            error("AudioRecord.read failed with $read")
                        }
                        if (read == 0) {
                            continue
                        }

                        val wholeSampleBytes = read - (read % 2)
                        stream.write(buffer, 0, wholeSampleBytes)
                        samplesWritten += wholeSampleBytes / 2
                    }

                    recorder?.stop()
                    stream.fd.sync()

                    val dataBytes = samplesWritten
                        .checked_mul(2)
                        ?: error("WAV data size overflow")
                    check(dataBytes <= UInt.MAX_VALUE.toLong()) {
                        "WAV data exceeds RIFF32 bound"
                    }

                    stream.channel.position(0)
                    stream.write(
                        pcm16MonoWavHeader(
                            sampleRateHz = AUDIO_SAMPLE_RATE_HZ,
                            dataBytes = dataBytes.toInt(),
                        ),
                    )
                    stream.fd.sync()

                    pass(
                        mode = "audio",
                        file = file,
                        detail =
                            "sample_rate_hz=$AUDIO_SAMPLE_RATE_HZ " +
                                "samples=$samplesWritten " +
                                "duration_ms=$durationMs",
                    )
                }
            }.onFailure { error ->
                fail(
                    mode = "audio",
                    reason = "capture_${error.javaClass.simpleName}",
                    detail = error.message,
                )
            }

            runCatching {
                if (recorder?.recordingState ==
                    AudioRecord.RECORDSTATE_RECORDING
                ) {
                    recorder?.stop()
                }
            }
            recorder?.release()
        }.start()
    }

    private fun captureAccelerometer(durationMs: Int) {
        val manager = getSystemService(Context.SENSOR_SERVICE)
            as SensorManager
        val sensor = manager.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)
        if (sensor == null) {
            fail(
                mode = "accelerometer",
                reason = "accelerometer_unavailable",
                detail = null,
            )
            return
        }

        val thread = HandlerThread("sp3-trace-accelerometer")
        thread.start()
        workerThread = thread
        val handler = Handler(thread.looper)
        sensorManager = manager

        val events = mutableListOf<AccelSample>()
        val listener = object : SensorEventListener {
            override fun onSensorChanged(event: SensorEvent) {
                if (event.values.size < 3 || finished.get()) {
                    return
                }
                events += AccelSample(
                    timestampNs = event.timestamp,
                    x = event.values[0],
                    y = event.values[1],
                    z = event.values[2],
                )
            }

            override fun onAccuracyChanged(
                sensor: Sensor?,
                accuracy: Int,
            ) = Unit
        }
        sensorListener = listener

        val startedAtNs = SystemClock.elapsedRealtimeNanos()
        val registered = manager.registerListener(
            listener,
            sensor,
            ACCEL_PERIOD_US,
            handler,
        )
        if (!registered) {
            fail(
                mode = "accelerometer",
                reason = "register_listener_failed",
                detail = null,
            )
            cleanupSensors()
            return
        }

        record(
            "RECORDED_TRACE_START mode=accelerometer " +
                "requested_hz=$ACCEL_SAMPLE_RATE_HZ " +
                "duration_ms=$durationMs sensor=${sanitize(sensor.name)}",
        )

        handler.postDelayed(
            {
                runCatching {
                    manager.unregisterListener(listener)
                    val stoppedAtNs = SystemClock.elapsedRealtimeNanos()
                    val snapshot = events.toList()
                    check(snapshot.size >= 2) {
                        "fewer than two accelerometer events"
                    }

                    val resampled = resampleAccelerometer(
                        snapshot,
                        sampleRateHz = ACCEL_SAMPLE_RATE_HZ,
                    )
                    check(resampled.isNotEmpty()) {
                        "accelerometer resampling produced no output"
                    }

                    val directory = traceDirectory()
                    val file = File(
                        directory,
                        "latest-accelerometer.csv",
                    )
                    BufferedWriter(
                        OutputStreamWriter(
                            FileOutputStream(file, false),
                            Charsets.UTF_8,
                        ),
                    ).use { writer ->
                        writer.write(
                            "sample_index,timestamp_ns,x,y,z,magnitude\n",
                        )
                        for ((index, sample) in resampled.withIndex()) {
                            writer.write(
                                String.format(
                                    Locale.US,
                                    "%d,%d,%.9f,%.9f,%.9f,%.9f\n",
                                    index,
                                    sample.timestampNs,
                                    sample.x,
                                    sample.y,
                                    sample.z,
                                    sample.magnitude,
                                ),
                            )
                        }
                    }

                    val observedDurationNs =
                        snapshot.last().timestampNs -
                            snapshot.first().timestampNs
                    val observedHz = if (observedDurationNs > 0L) {
                        (snapshot.size - 1).toDouble() *
                            1_000_000_000.0 /
                            observedDurationNs.toDouble()
                    } else {
                        0.0
                    }

                    pass(
                        mode = "accelerometer",
                        file = file,
                        detail = String.format(
                            Locale.US,
                            "requested_hz=%d raw_events=%d " +
                                "resampled=%d observed_hz=%.3f " +
                                "wall_duration_ms=%.3f",
                            ACCEL_SAMPLE_RATE_HZ,
                            snapshot.size,
                            resampled.size,
                            observedHz,
                            (stoppedAtNs - startedAtNs) / 1_000_000.0,
                        ),
                    )
                }.onFailure { error ->
                    fail(
                        mode = "accelerometer",
                        reason =
                            "capture_${error.javaClass.simpleName}",
                        detail = error.message,
                    )
                }
                cleanupSensors()
            },
            durationMs.toLong(),
        )
    }

    private fun traceDirectory(): File =
        File(filesDir, "recorded-traces").also {
            check(it.exists() || it.mkdirs()) {
                "failed to create recorded-traces directory"
            }
        }

    private fun pass(
        mode: String,
        file: File,
        detail: String,
    ) {
        if (!finished.compareAndSet(false, true)) {
            return
        }

        val bytes = file.readBytes()
        val sha256 = LabCodec.sha256Hex(bytes)
        record(
            "RECORDED_TRACE_PASS mode=$mode file=${file.name} " +
                "bytes=${bytes.size} sha256=$sha256 $detail " +
                "evidence_level=ANDROID_RUNTIME_CAPTURE",
        )
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

        record(
            "RECORDED_TRACE_FAIL mode=${sanitize(mode)} " +
                "reason=${sanitize(reason)} " +
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
            val directory = traceDirectory()
            File(directory, "latest.txt").writeText(line + "\n")
        }.onFailure { error ->
            Log.e(
                LOG_TAG,
                "RECORDED_TRACE_EVIDENCE_FAIL " +
                    "reason=${error.javaClass.simpleName}",
            )
        }
    }

    private fun cleanupSensors() {
        val manager = sensorManager
        val listener = sensorListener
        if (manager != null && listener != null) {
            runCatching { manager.unregisterListener(listener) }
        }
        sensorListener = null
        sensorManager = null
        workerThread?.quitSafely()
        workerThread = null
    }

    override fun onDestroy() {
        cleanupSensors()
        super.onDestroy()
    }

    private fun sanitize(value: String?): String =
        value
            ?.replace('\n', ' ')
            ?.replace('\r', ' ')
            ?.replace(' ', '_')
            ?.take(240)
            ?: "-"

    private data class AccelSample(
        val timestampNs: Long,
        val x: Float,
        val y: Float,
        val z: Float,
    ) {
        val magnitude: Float
            get() = sqrt(x * x + y * y + z * z)
    }

    private fun resampleAccelerometer(
        input: List<AccelSample>,
        sampleRateHz: Int,
    ): List<AccelSample> {
        require(input.size >= 2)
        require(sampleRateHz > 0)

        val periodNs = 1_000_000_000L / sampleRateHz
        val start = input.first().timestampNs
        val end = input.last().timestampNs
        if (end <= start) {
            return emptyList()
        }

        val output = mutableListOf<AccelSample>()
        var left = 0
        var target = start
        while (target <= end) {
            while (
                left + 1 < input.size &&
                input[left + 1].timestampNs < target
            ) {
                left += 1
            }
            if (left + 1 >= input.size) {
                break
            }

            val first = input[left]
            val second = input[left + 1]
            val span = second.timestampNs - first.timestampNs
            val alpha = if (span <= 0L) {
                0.0
            } else {
                (target - first.timestampNs).toDouble() /
                    span.toDouble()
            }.coerceIn(0.0, 1.0)

            output += AccelSample(
                timestampNs = target,
                x = lerp(first.x, second.x, alpha),
                y = lerp(first.y, second.y, alpha),
                z = lerp(first.z, second.z, alpha),
            )
            target += periodNs
        }

        return output
    }

    private fun lerp(
        first: Float,
        second: Float,
        alpha: Double,
    ): Float =
        (first + (second - first) * alpha.toFloat())

    private fun pcm16MonoWavHeader(
        sampleRateHz: Int,
        dataBytes: Int,
    ): ByteArray {
        require(sampleRateHz > 0)
        require(dataBytes >= 0)
        val byteRate = sampleRateHz * 2
        val riffSize = 36L + dataBytes.toLong()
        require(riffSize <= UInt.MAX_VALUE.toLong())

        return ByteBuffer
            .allocate(44)
            .order(ByteOrder.LITTLE_ENDIAN)
            .apply {
                put("RIFF".toByteArray(Charsets.US_ASCII))
                putInt(riffSize.toInt())
                put("WAVE".toByteArray(Charsets.US_ASCII))
                put("fmt ".toByteArray(Charsets.US_ASCII))
                putInt(16)
                putShort(1)
                putShort(1)
                putInt(sampleRateHz)
                putInt(byteRate)
                putShort(2)
                putShort(16)
                put("data".toByteArray(Charsets.US_ASCII))
                putInt(dataBytes)
            }
            .array()
    }
}
