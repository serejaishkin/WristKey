package com.wristkey.ui

import android.app.Activity
import android.os.Bundle
import android.view.HapticFeedbackConstants
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.Canvas
import androidx.compose.foundation.gestures.detectTapGestures
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.geometry.Offset
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.input.pointer.pointerInput
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.compose.ui.layout.onSizeChanged
import androidx.wear.compose.material.*
import com.wristkey.security.TouchPoint
import com.wristkey.security.TouchPointStore

class TrainingActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                TrainingScreen(
                    onDone = { setResult(Activity.RESULT_OK); finish() },
                    onCancelled = { setResult(Activity.RESULT_CANCELED); finish() }
                )
            }
        }
    }
}

private enum class TrainPhase { SET, REPEAT, DONE }

@Composable
fun TrainingScreen(onDone: () -> Unit, onCancelled: () -> Unit) {
    val context = LocalContext.current
    val store = remember { TouchPointStore(context) }
    val view = LocalView.current
    val primaryColor = MaterialTheme.colors.primary

    var phase by remember { mutableStateOf(TrainPhase.SET) }
    var anchor by remember { mutableStateOf<TouchPoint?>(null) }
    var repeatsOk by remember { mutableIntStateOf(0) }
    var sizePx by remember { mutableStateOf(IntSize.Zero) }
    val requiredRepeats = 2

    fun hit(nx: Float, ny: Float): Boolean {
        val a = anchor ?: return false
        val w = sizePx.width.toFloat()
        if (w <= 0f) return false
        val dx = (nx - a.x) * w
        val dy = (ny - a.y) * w
        return kotlin.math.sqrt(dx * dx + dy * dy) <= TouchPointStore.TRAIN_TOLERANCE * w
    }

    Box(modifier = Modifier.fillMaxSize().onSizeChanged { sizePx = it }) {
        Canvas(modifier = Modifier.fillMaxSize()) {
            val a = anchor ?: return@Canvas
            val center = Offset(a.x * size.width, a.y * size.height)
            val radius = TouchPointStore.TRAIN_TOLERANCE * size.width
            drawCircle(Color.White.copy(alpha = 0.10f), radius = radius, center = center)
            drawCircle(if (phase == TrainPhase.DONE) Color(0xFF4CAF50) else primaryColor, radius = 14f, center = center)
            drawCircle(Color.Black, radius = 5f, center = center)
        }

        Column(
            modifier = Modifier.fillMaxSize().padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center
        ) {
            when (phase) {
                TrainPhase.SET -> Text("Обучение точки\n\nКоснитесь экрана там,\nгде будет точка разблокировки", style = MaterialTheme.typography.body1, textAlign = TextAlign.Center)
                TrainPhase.REPEAT -> Text("Повторите касание\n(${requiredRepeats - repeatsOk} осталось)", style = MaterialTheme.typography.body1, textAlign = TextAlign.Center)
                TrainPhase.DONE -> {
                    Text("Точка сохранена ✓", style = MaterialTheme.typography.title3, textAlign = TextAlign.Center, color = Color(0xFF4CAF50))
                    Spacer(Modifier.height(12.dp))
                    Button(onClick = onDone, modifier = Modifier.fillMaxWidth(0.6f)) { Text("Готово") }
                }
            }
            if (phase != TrainPhase.DONE) {
                Spacer(Modifier.height(10.dp))
                Button(onClick = onCancelled, colors = ButtonDefaults.secondaryButtonColors(), modifier = Modifier.fillMaxWidth(0.5f)) { Text("Отмена") }
            }
        }

        Box(
            modifier = Modifier
                .fillMaxSize()
                .pointerInput(phase) {
                    detectTapGestures { offset ->
                        if (phase == TrainPhase.DONE || size.width == 0) return@detectTapGestures
                        val nx = offset.x / size.width
                        val ny = offset.y / size.height
                        when (phase) {
                            TrainPhase.SET -> {
                                anchor = TouchPoint(nx, ny)
                                repeatsOk = 0
                                phase = TrainPhase.REPEAT
                                view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                            }
                            TrainPhase.REPEAT -> {
                                if (hit(nx, ny)) {
                                    repeatsOk++
                                    view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                                    val saved = anchor
                                    if (repeatsOk >= requiredRepeats && saved != null) {
                                        store.save(saved)
                                        view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                                        phase = TrainPhase.DONE
                                    }
                                } else {
                                    anchor = null
                                    repeatsOk = 0
                                    phase = TrainPhase.SET
                                    view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                                }
                            }
                            else -> Unit
                        }
                    }
                }
        )
    }
}
