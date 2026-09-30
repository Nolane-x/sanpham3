package dev.nolane.sanpham3.recoverylab

import android.Manifest
import android.app.Activity
import android.content.Context
import android.content.pm.PackageManager
import android.graphics.ImageFormat
import android.hardware.camera2.CameraCaptureSession
import android.hardware.camera2.CameraCharacteristics
import android.hardware.camera2.CameraDevice
import android.hardware.camera2.CameraManager
import android.media.Image
import android.media.ImageReader
import android.os.Bundle
import android.os.Handler
import android.os.HandlerThread
import android.util.Log
import android.util.Size
import java.io.File
import java.io.FileOutputStream
import java.time.Instant
import java.util.concurrent.atomic.AtomicBoolean
import kotlin.math.abs

class CameraOpticalCaptureActivity : Activity() {
    companion object {
        const val LOG_TAG = "SP3CameraOptical"
        const val EXTRA_FRAME_COUNT =
            "dev.nolane.sanpham3.recoverylab.CAMERA_FRAME_COUNT"
        const val EXTRA_WARMUP_FRAMES =
            "dev.nolane.sanpham3.recoverylab.CAMERA_WARMUP_FRAMES"
        const val EXTRA_WIDTH =
            "dev.nolane.sanpham3.recoverylab.CAMERA_WIDTH"
        const val EXTRA_HEIGHT =
            "dev.nolane.sanpham3.recoverylab.CAMERA_HEIGHT"

        private const val DEFAULT_WIDTH = 640
        private const val DEFAULT_HEIGHT = 480
        private const val DEFAULT_FRAME_COUNT = 12
        private const val DEFAULT_WARMUP_FRAMES = 15
        private const val COURT_TIMEOUT_MS = 60_000L
    }

    private val completed = AtomicBoolean(false)
    private var cameraThread: HandlerThread? = null
    private var cameraHandler: Handler? = null
    private var imageReader: ImageReader? = null
    private var cameraDevice: CameraDevice? = null
    private var captureSession: CameraCaptureSession? = null
    private var output: FileOutputStream? = null
    private var outputFile: File? = null
    private var capturedFrames = 0
    private var observedFrames = 0
    private var targetFrames = DEFAULT_FRAME_COUNT
    private var warmupFrames = DEFAULT_WARMUP_FRAMES
    private var captureWidth = DEFAULT_WIDTH
    private var captureHeight = DEFAULT_HEIGHT
    private var outputWidth = DEFAULT_WIDTH
    private var outputHeight = DEFAULT_HEIGHT
    private var sensorOrientationDegrees = 0

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        targetFrames = intent?.getIntExtra(
            EXTRA_FRAME_COUNT,
            DEFAULT_FRAME_COUNT,
        ) ?: DEFAULT_FRAME_COUNT
        warmupFrames = intent?.getIntExtra(
            EXTRA_WARMUP_FRAMES,
            DEFAULT_WARMUP_FRAMES,
        ) ?: DEFAULT_WARMUP_FRAMES
        val preferredWidth = intent?.getIntExtra(
            EXTRA_WIDTH,
            DEFAULT_WIDTH,
        ) ?: DEFAULT_WIDTH
        val preferredHeight = intent?.getIntExtra(
            EXTRA_HEIGHT,
            DEFAULT_HEIGHT,
        ) ?: DEFAULT_HEIGHT

        if (targetFrames !in 1..120 || warmupFrames !in 0..300) {
            fail("invalid_frame_budget", null)
            return
        }

        if (checkSelfPermission(Manifest.permission.CAMERA) !=
            PackageManager.PERMISSION_GRANTED
        ) {
            fail("camera_permission_missing", null)
            return
        }

        cameraThread = HandlerThread("sp3-camera-optical").also {
            it.start()
        }
        cameraHandler = Handler(cameraThread!!.looper)

        Thread {
            Thread.sleep(COURT_TIMEOUT_MS)
            if (completed.compareAndSet(false, true)) {
                record("CAMERA_VIDEO_SOURCE_FAIL reason=court_timeout")
                cleanup()
                runOnUiThread { finish() }
            }
        }.start()

        runCatching {
            startCamera(preferredWidth, preferredHeight)
        }.onFailure { error ->
            fail(
                "startup_${error.javaClass.simpleName}",
                error.message,
            )
        }
    }

    private fun startCamera(
        preferredWidth: Int,
        preferredHeight: Int,
    ) {
        val manager = getSystemService(Context.CAMERA_SERVICE)
            as CameraManager
        val cameraId = chooseBackCamera(manager)
        val characteristics =
            manager.getCameraCharacteristics(cameraId)
        val map = checkNotNull(
            characteristics.get(
                CameraCharacteristics.SCALER_STREAM_CONFIGURATION_MAP,
            ),
        ) {
            "camera stream configuration map unavailable"
        }

        val sizes = map.getOutputSizes(ImageFormat.YUV_420_888)
            ?.toList()
            .orEmpty()
        val selected = chooseCaptureSize(
            sizes,
            preferredWidth,
            preferredHeight,
        )
        captureWidth = selected.width
        captureHeight = selected.height
        sensorOrientationDegrees =
            (characteristics.get(
                CameraCharacteristics.SENSOR_ORIENTATION,
            ) ?: 0)
                .mod(360)
        require(
            sensorOrientationDegrees == 0 ||
                sensorOrientationDegrees == 90 ||
                sensorOrientationDegrees == 180 ||
                sensorOrientationDegrees == 270,
        ) {
            "unsupported sensor orientation $sensorOrientationDegrees"
        }
        if (
            sensorOrientationDegrees == 90 ||
            sensorOrientationDegrees == 270
        ) {
            outputWidth = captureHeight
            outputHeight = captureWidth
        } else {
            outputWidth = captureWidth
            outputHeight = captureHeight
        }

        val directory = File(filesDir, "camera-optical")
        check(directory.exists() || directory.mkdirs()) {
            "failed to create camera evidence directory"
        }
        outputFile = File(directory, "latest.y4m")
        output = FileOutputStream(outputFile!!, false).also { stream ->
            stream.write(
                "YUV4MPEG2 W$outputWidth H$outputHeight F30:1 Ip Cmono\n"
                    .toByteArray(Charsets.US_ASCII),
            )
        }

        imageReader = ImageReader.newInstance(
            captureWidth,
            captureHeight,
            ImageFormat.YUV_420_888,
            4,
        ).also { reader ->
            reader.setOnImageAvailableListener(
                { available ->
                    onImageAvailable(available)
                },
                cameraHandler,
            )
        }

        record(
            "CAMERA_VIDEO_SOURCE_START camera_id=$cameraId " +
                "sensor_width=$captureWidth sensor_height=$captureHeight " +
                "output_width=$outputWidth output_height=$outputHeight " +
                "sensor_orientation=$sensorOrientationDegrees " +
                "target_frames=$targetFrames warmup_frames=$warmupFrames",
        )

        @Suppress("MissingPermission")
        manager.openCamera(
            cameraId,
            object : CameraDevice.StateCallback() {
                override fun onOpened(camera: CameraDevice) {
                    cameraDevice = camera
                    createSession(camera)
                }

                override fun onDisconnected(camera: CameraDevice) {
                    camera.close()
                    fail("camera_disconnected", null)
                }

                override fun onError(
                    camera: CameraDevice,
                    error: Int,
                ) {
                    camera.close()
                    fail("camera_error_$error", null)
                }
            },
            cameraHandler,
        )
    }

    private fun chooseBackCamera(
        manager: CameraManager,
    ): String {
        val ids = manager.cameraIdList
        check(ids.isNotEmpty()) {
            "no camera IDs available"
        }

        return ids.firstOrNull { id ->
            manager
                .getCameraCharacteristics(id)
                .get(CameraCharacteristics.LENS_FACING) ==
                CameraCharacteristics.LENS_FACING_BACK
        } ?: ids.first()
    }

    private fun chooseCaptureSize(
        sizes: List<Size>,
        preferredWidth: Int,
        preferredHeight: Int,
    ): Size {
        check(sizes.isNotEmpty()) {
            "camera exposes no YUV_420_888 output size"
        }

        sizes.firstOrNull {
            it.width == preferredWidth &&
                it.height == preferredHeight
        }?.let { return it }

        val preferredRatio =
            preferredWidth.toDouble() / preferredHeight.toDouble()
        return sizes
            .filter {
                abs(
                    it.width.toDouble() / it.height.toDouble() -
                        preferredRatio,
                ) <= 0.05
            }
            .minByOrNull {
                abs(
                    it.width.toLong() * it.height -
                        preferredWidth.toLong() * preferredHeight,
                )
            }
            ?: error(
                "camera has no output size near " +
                    "$preferredWidth:$preferredHeight aspect ratio",
            )
    }

    @Suppress("DEPRECATION")
    private fun createSession(camera: CameraDevice) {
        val reader = checkNotNull(imageReader)
        camera.createCaptureSession(
            listOf(reader.surface),
            object : CameraCaptureSession.StateCallback() {
                override fun onConfigured(
                    session: CameraCaptureSession,
                ) {
                    if (completed.get()) {
                        session.close()
                        return
                    }
                    captureSession = session

                    val request = camera.createCaptureRequest(
                        CameraDevice.TEMPLATE_PREVIEW,
                    ).apply {
                        addTarget(reader.surface)
                    }.build()

                    session.setRepeatingRequest(
                        request,
                        null,
                        cameraHandler,
                    )
                }

                override fun onConfigureFailed(
                    session: CameraCaptureSession,
                ) {
                    session.close()
                    fail("capture_session_configure_failed", null)
                }
            },
            cameraHandler,
        )
    }

    private fun onImageAvailable(reader: ImageReader) {
        if (completed.get()) {
            reader.acquireLatestImage()?.close()
            return
        }

        val image = reader.acquireLatestImage() ?: return
        image.use {
            observedFrames += 1
            if (observedFrames <= warmupFrames) {
                return
            }

            runCatching {
                val luma = orientedTightLuma(image)
                check(luma.size == outputWidth * outputHeight) {
                    "unexpected oriented luma size ${luma.size}"
                }

                val stream = checkNotNull(output)
                stream.write("FRAME\n".toByteArray(Charsets.US_ASCII))
                stream.write(luma)
                capturedFrames += 1

                if (capturedFrames >= targetFrames) {
                    stream.flush()
                    stream.fd.sync()
                    val file = checkNotNull(outputFile)
                    val digest = LabCodec.sha256Hex(file.readBytes())
                    pass(file, digest)
                }
            }.onFailure { error ->
                fail(
                    "frame_${error.javaClass.simpleName}",
                    error.message,
                )
            }
        }
    }

    private fun orientedTightLuma(image: Image): ByteArray {
        check(image.format == ImageFormat.YUV_420_888) {
            "unexpected image format ${image.format}"
        }
        check(image.width == captureWidth && image.height == captureHeight) {
            "unexpected image dimensions ${image.width}x${image.height}"
        }

        val plane = image.planes[0]
        val buffer = plane.buffer.duplicate()
        val rowStride = plane.rowStride
        val pixelStride = plane.pixelStride
        check(rowStride > 0 && pixelStride > 0) {
            "invalid Y plane strides"
        }

        val output = ByteArray(image.width * image.height)
        var target = 0
        for (y in 0 until image.height) {
            val rowBase = y * rowStride
            for (x in 0 until image.width) {
                val source = rowBase + x * pixelStride
                check(source < buffer.limit()) {
                    "Y plane stride exceeds buffer"
                }
                output[target] = buffer.get(source)
                target += 1
            }
        }
        return rotateLuma(
            input = output,
            width = image.width,
            height = image.height,
            clockwiseDegrees = sensorOrientationDegrees,
        )
    }

    private fun rotateLuma(
        input: ByteArray,
        width: Int,
        height: Int,
        clockwiseDegrees: Int,
    ): ByteArray {
        require(input.size == width * height)
        return when (clockwiseDegrees) {
            0 -> input
            90 -> {
                val out = ByteArray(input.size)
                val outWidth = height
                for (y in 0 until height) {
                    for (x in 0 until width) {
                        val destX = height - 1 - y
                        val destY = x
                        out[destY * outWidth + destX] =
                            input[y * width + x]
                    }
                }
                out
            }
            180 -> {
                val out = ByteArray(input.size)
                for (y in 0 until height) {
                    for (x in 0 until width) {
                        val destX = width - 1 - x
                        val destY = height - 1 - y
                        out[destY * width + destX] =
                            input[y * width + x]
                    }
                }
                out
            }
            270 -> {
                val out = ByteArray(input.size)
                val outWidth = height
                for (y in 0 until height) {
                    for (x in 0 until width) {
                        val destX = y
                        val destY = width - 1 - x
                        out[destY * outWidth + destX] =
                            input[y * width + x]
                    }
                }
                out
            }
            else -> error(
                "unsupported sensor orientation $clockwiseDegrees",
            )
        }
    }

    private fun pass(
        file: File,
        sha256: String,
    ) {
        if (!completed.compareAndSet(false, true)) {
            return
        }

        record(
            "CAMERA_VIDEO_SOURCE_PASS frames=$capturedFrames " +
                "observed_frames=$observedFrames " +
                "width=$outputWidth height=$outputHeight " +
                "sensor_orientation=$sensorOrientationDegrees " +
                "sha256=$sha256 file=${file.name} " +
                "evidence_level=ANDROID_AVD_CAMERA",
        )
        cleanup()
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
            "CAMERA_VIDEO_SOURCE_FAIL reason=${sanitize(reason)} " +
                "detail=${sanitize(detail)}",
        )
        cleanup()
        runOnUiThread { finish() }
    }

    private fun cleanup() {
        runCatching { captureSession?.stopRepeating() }
        runCatching { captureSession?.close() }
        captureSession = null
        runCatching { cameraDevice?.close() }
        cameraDevice = null
        runCatching { imageReader?.close() }
        imageReader = null
        runCatching { output?.close() }
        output = null
        cameraThread?.quitSafely()
        cameraThread = null
        cameraHandler = null
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
            val directory = File(filesDir, "camera-optical")
            check(directory.exists() || directory.mkdirs())
            File(directory, "latest.txt").writeText(line + "\n")
        }.onFailure { error ->
            Log.e(
                LOG_TAG,
                "CAMERA_VIDEO_SOURCE_EVIDENCE_FAIL " +
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
        cleanup()
        super.onDestroy()
    }
}
