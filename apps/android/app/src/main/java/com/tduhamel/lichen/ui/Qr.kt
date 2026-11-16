package com.tduhamel.lichen.ui

import android.Manifest
import android.content.pm.PackageManager
import android.view.HapticFeedbackConstants
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.result.contract.ActivityResultContracts
import androidx.camera.core.CameraSelector
import androidx.camera.core.ImageAnalysis
import androidx.camera.core.ImageProxy
import androidx.camera.core.Preview
import androidx.camera.lifecycle.ProcessCameraProvider
import androidx.camera.view.PreviewView
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.aspectRatio
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.Composable
import androidx.compose.runtime.DisposableEffect
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.remember
import androidx.compose.runtime.setValue
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.geometry.Size
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.semantics.contentDescription
import androidx.compose.ui.semantics.semantics
import androidx.compose.ui.unit.dp
import androidx.compose.ui.viewinterop.AndroidView
import androidx.core.content.ContextCompat
import androidx.lifecycle.compose.LocalLifecycleOwner
import com.google.zxing.BinaryBitmap
import com.google.zxing.PlanarYUVLuminanceSource
import com.google.zxing.common.HybridBinarizer
import com.google.zxing.qrcode.QRCodeReader
import com.tduhamel.lichen.core.qrCode
import java.util.concurrent.Executors

/** A QR code drawn from the core's modules (level M, as on iOS), black on white. */
@Composable
fun QrImage(text: String, label: String, modifier: Modifier = Modifier) {
    val code = remember(text) { runCatching { qrCode(text) }.getOrNull() } ?: return
    val n = code.width.toInt()
    Canvas(
        modifier
            .aspectRatio(1f)
            .background(Color.White, CardShape)
            .padding(14.dp)
            .semantics { contentDescription = label },
    ) {
        val cell = size.minDimension / n
        for (y in 0 until n) {
            for (x in 0 until n) {
                if (code.dark[y * n + x]) {
                    drawRect(Color.Black, Offset(x * cell, y * cell), Size(cell + 0.5f, cell + 0.5f))
                }
            }
        }
    }
}

/**
 * The camera, reporting the first lichen code it reads (ZXing on the luminance plane). Asks for
 * the camera permission; without it nothing shows (the code can be pasted instead).
 */
@Composable
fun QrScanner(found: (String) -> Unit, modifier: Modifier = Modifier) {
    val context = LocalContext.current
    val owner = LocalLifecycleOwner.current
    val view = LocalView.current
    fun granted() = ContextCompat.checkSelfPermission(context, Manifest.permission.CAMERA) == PackageManager.PERMISSION_GRANTED
    var allowed by remember { mutableStateOf(granted()) }
    val ask = rememberLauncherForActivityResult(ActivityResultContracts.RequestPermission()) { allowed = it }
    LaunchedEffect(Unit) { if (!allowed) ask.launch(Manifest.permission.CAMERA) }
    if (!allowed) return

    val executor = remember { Executors.newSingleThreadExecutor() }
    var reported by remember { mutableStateOf(false) }
    val reader = remember { QRCodeReader() }
    DisposableEffect(Unit) {
        onDispose {
            ProcessCameraProvider.getInstance(context).get().unbindAll()
            executor.shutdown()
        }
    }
    AndroidView(
        factory = { ctx ->
            // FILL_CENTER scales the picture past the view's bounds: the caller clips it, which
            // only works on a TextureView (COMPATIBLE); a SurfaceView ignores Compose's clip.
            val preview = PreviewView(ctx).apply {
                implementationMode = PreviewView.ImplementationMode.COMPATIBLE
                scaleType = PreviewView.ScaleType.FILL_CENTER
            }
            val providerFuture = ProcessCameraProvider.getInstance(ctx)
            providerFuture.addListener({
                val provider = providerFuture.get()
                val p = Preview.Builder().build().also { it.surfaceProvider = preview.surfaceProvider }
                val analysis = ImageAnalysis.Builder()
                    .setBackpressureStrategy(ImageAnalysis.STRATEGY_KEEP_ONLY_LATEST)
                    .build()
                analysis.setAnalyzer(executor) { image ->
                    val text = decode(reader, image)
                    image.close()
                    if (text != null && text.startsWith("lichen:")) {
                        preview.post {
                            if (!reported) {
                                reported = true
                                view.performHapticFeedback(HapticFeedbackConstants.CONFIRM)
                                found(text)
                            }
                        }
                    }
                }
                provider.unbindAll()
                runCatching { provider.bindToLifecycle(owner, CameraSelector.DEFAULT_BACK_CAMERA, p, analysis) }
            }, ContextCompat.getMainExecutor(ctx))
            preview
        },
        modifier = modifier,
    )
}

/** The QR code in a camera frame, from its luminance (Y) plane. */
private fun decode(reader: QRCodeReader, image: ImageProxy): String? {
    val plane = image.planes.firstOrNull() ?: return null
    val buffer = plane.buffer
    val stride = plane.rowStride
    val bytes = ByteArray(buffer.remaining()).also { buffer.get(it) }
    val source = PlanarYUVLuminanceSource(bytes, stride, image.height, 0, 0, image.width, image.height, false)
    return try {
        reader.decode(BinaryBitmap(HybridBinarizer(source))).text
    } catch (_: Exception) {
        null
    } finally {
        reader.reset()
    }
}
