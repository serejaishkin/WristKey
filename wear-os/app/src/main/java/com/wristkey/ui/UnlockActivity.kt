package com.wristkey.ui

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.view.HapticFeedbackConstants
import android.view.View
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.wear.compose.material.*
import com.wristkey.security.TouchPoint
import com.wristkey.security.TouchPointStore

class UnlockActivity : ComponentActivity() {

    private var decided = false
    private val trainedVersion = mutableStateOf(0)

    private val trainingLauncher = registerForActivityResult(
        ActivityResultContracts.StartActivityForResult()
    ) { result ->
        if (result.resultCode == Activity.RESULT_OK) trainedVersion.value++
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val user = intent.getStringExtra("user") ?: "Unknown PC"
        setContent {
            MaterialTheme {
                UnlockGateScreen(
                    user = user,
                    trainedVersion = trainedVersion.value,
                    onTrainRequested = { trainingLauncher.launch(Intent(this, TrainingActivity::class.java)) },
                    onApprove = { decide(true) },
                    onCancel = { decide(false) }
                )
            }
        }
    }

    private fun decide(approved: Boolean) {
        if (decided) return
        decided = true
        sendUnlockBroadcast(approved)
        finish()
    }

    override fun onDestroy() {
        if (!decided) {
            // Activity destroyed without an explicit decision (swipe-away,
            // system kill): tell the PC explicitly instead of letting it time out.
            decided = true
            sendUnlockBroadcast(approved = false)
        }
        super.onDestroy()
    }

    private fun sendUnlockBroadcast(approved: Boolean) {
        sendBroadcast(Intent(ACTION_UNLOCK).apply {
            setPackage(packageName)
            putExtra(EXTRA_APPROVED, approved)
        })
    }

    companion object {
        const val ACTION_UNLOCK = "com.wristkey.UNLOCK_ACTION"
        const val EXTRA_APPROVED = "approved"
    }
}

@Composable
fun UnlockGateScreen(
    user: String,
    trainedVersion: Int,
    onTrainRequested: () -> Unit,
    onApprove: () -> Unit,
    onCancel: () -> Unit
) {
    val context = LocalContext.current
    val store = remember { TouchPointStore(context) }
    val view = LocalView.current
    val point = remember(trainedVersion) { store.load() }

    if (point == null) {
        NoPointScreen(user, onTrainRequested, onCancel)
    } else {
        TouchPointUnlockScreen(point, onApprove, onCancel, view)
    }
}

@Composable
fun NoPointScreen(user: String, onTrainRequested: () -> Unit, onCancel: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text("🔓 Unlock PC?", style = MaterialTheme.typography.title2, textAlign = TextAlign.Center)
        Text(user, style = MaterialTheme.typography.caption2, textAlign = TextAlign.Center)
        Spacer(Modifier.height(10.dp))
        Text("Точка разблокировки не настроена", style = MaterialTheme.typography.caption1, textAlign = TextAlign.Center, color = Color(0xFFFFB74D))
        Spacer(Modifier.height(12.dp))
        Button(onClick = onTrainRequested, modifier = Modifier.fillMaxWidth(0.8f)) { Text("Обучить точку") }
        Spacer(Modifier.height(6.dp))
        Button(onClick = onCancel, colors = ButtonDefaults.secondaryButtonColors(), modifier = Modifier.fillMaxWidth(0.8f)) { Text("✗ Отмена") }
    }
}

@Composable
fun TouchPointUnlockScreen(
    point: TouchPoint,
    onApprove: () -> Unit,
    onCancel: () -> Unit,
    view: View
) {
    val primaryColor = MaterialTheme.colors.primary
    var sizePx by remember { mutableStateOf(IntSize.Zero) }
    var missCount by remember { mutableIntStateOf(0) }

    fun hit(nx: Float, ny: Float): Boolean {
        val w = sizePx.width.toFloat()
        if (w <= 0f) return false
        val dx = (nx - point.x) * w
        val dy = (ny - point.y) * w
        return kotlin.math.sqrt(dx * dx + dy * dy) <= TouchPointStore.HIT_TOLERANCE * w
    }

    Box(modifier = Modifier.fillMaxSize().onSizeChanged { sizePx = it }) {
        Canvas(modifier = Modifier.fillMaxSize()) {
            val center = Offset(point.x * size.width, point.y * size.height)
            drawCircle(Color.White.copy(alpha = 0.08f), radius = TouchPointStore.HIT_TOLERANCE * size.width, center = center)
            drawCircle(primaryColor.copy(alpha = 0.9f), radius = 16f, center = center)
            drawCircle(Color.Black, radius = 6f, center = center)
        }

        Column(
            modifier = Modifier.fillMaxSize().padding(horizontal = 14.dp),
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            Spacer(Modifier.height(18.dp))
            Text("Коснитесь точки\nразблокировки", style = MaterialTheme.typography.body1, textAlign = TextAlign.Center)
            if (missCount > 0) {
                Text("Мимо ($missCount)", style = MaterialTheme.typography.caption2, color = Color(0xFFEF5350), textAlign = TextAlign.Center)
            }
            Spacer(Modifier.weight(1f))
            Button(onClick = onCancel, colors = ButtonDefaults.secondaryButtonColors(), modifier = Modifier.fillMaxWidth(0.5f)) { Text("✗ Отмена") }
            Spacer(Modifier.height(14.dp))
        }

        Box(
            modifier = Modifier
                .fillMaxSize()
                .pointerInput(point) {
                    detectTapGestures { offset ->
                        if (size.width == 0) return@detectTapGestures
                        val nx = offset.x / size.width
                        val ny = offset.y / size.height
                        if (hit(nx, ny)) {
                            view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                            onApprove()
                        } else {
                            missCount++
                            view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                        }
                    }
                }
        )
    }
}
