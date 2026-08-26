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
import androidx.compose.ui.layout.onSizeChanged
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.IntSize
import androidx.compose.ui.unit.dp
import androidx.wear.compose.material.*
import com.wristkey.security.TouchPoint
import com.wristkey.security.TouchPointStore
import kotlinx.coroutines.delay

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

private enum class TrainPhase { PREP, RECORD, DONE }

// The training must be almost touch-free: one tap during the initial 10 s
// window places the point, then a 10 s recording window runs on its own.
// Extra touches during recording only refine the stored position.
private const val PREP_SECONDS = 10
private const val RECORD_SECONDS = 10

@Composable
fun TrainingScreen(onDone: () -> Unit, onCancelled: () -> Unit) {
    val context = LocalContext.current
    val store = remember { TouchPointStore(context) }
    val view = LocalView.current
    val primaryColor = MaterialTheme.colors.primary

    var phase by remember { mutableStateOf(TrainPhase.PREP) }
    var secondsLeft by remember { mutableIntStateOf(PREP_SECONDS) }
    var anchor by remember { mutableStateOf<TouchPoint?>(null) }
    val samples = remember { mutableStateListOf<TouchPoint>() }
    var sizePx by remember { mutableStateOf(IntSize.Zero) }

    LaunchedEffect(phase) {
        when (phase) {
            TrainPhase.PREP -> {
                // Re-arm the 10 s touch window until the first tap arrives.
                while (phase == TrainPhase.PREP) {
                    secondsLeft = PREP_SECONDS
                    while (secondsLeft > 0) {
                        delay(1000)
                        if (phase != TrainPhase.PREP) return@LaunchedEffect
                        secondsLeft--
                    }
                    view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                }
            }
            TrainPhase.RECORD -> {
                secondsLeft = RECORD_SECONDS
                while (secondsLeft > 0) {
                    delay(1000)
                    secondsLeft--
                }
                // Recording finished: average every accepted touch so the
                // stored point reflects where the finger naturally lands.
                val base = anchor
                if (base != null) {
                    val xs = samples.map { it.x }
                    val ys = samples.map { it.y }
                    val refined = if (xs.isEmpty()) base
                        else TouchPoint(xs.average().toFloat(), ys.average().toFloat())
                    store.save(refined)
                    view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                }
                phase = TrainPhase.DONE
            }
            TrainPhase.DONE -> Unit
        }
    }

    Box(modifier = Modifier.fillMaxSize().onSizeChanged { sizePx = it }) {
        Canvas(modifier = Modifier.fillMaxSize()) {
            val a = anchor ?: return@Canvas
            val center = Offset(a.x * size.width, a.y * size.height)
            val radius = TouchPointStore.TRAIN_TOLERANCE * size.width
            drawCircle(Color.White.copy(alpha = 0.10f), radius = radius, center = center)
            drawCircle(
                if (phase == TrainPhase.DONE) Color(0xFF4CAF50) else primaryColor,
                radius = 14f,
                center = center
            )
            drawCircle(Color.Black, radius = 5f, center = center)
        }

        Column(
            modifier = Modifier.fillMaxSize().padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center
        ) {
            when (phase) {
                TrainPhase.PREP -> Text(
                    "Обучение\n\nКоснитесь точки разблокировки\nОсталось: ${secondsLeft} с",
                    style = MaterialTheme.typography.body1,
                    textAlign = TextAlign.Center
                )
                TrainPhase.RECORD -> Text(
                    "Запись точки…\n${secondsLeft} с\n\nМожно коснуться ещё раз,\nчтобы уточнить положение",
                    style = MaterialTheme.typography.body1,
                    textAlign = TextAlign.Center
                )
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
                            TrainPhase.PREP -> {
                                anchor = TouchPoint(nx, ny)
                                samples.clear()
                                samples.add(anchor!!)
                                view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                                phase = TrainPhase.RECORD
                            }
                            TrainPhase.RECORD -> {
                                // Accept touches anywhere within a generous
                                // band around the anchor; they only refine it.
                                val a = anchor
                                val w = size.width.toFloat()
                                if (a != null && w > 0f) {
                                    val dx = (nx - a.x) * w
                                    val dy = (ny - a.y) * w
                                    if (kotlin.math.sqrt(dx * dx + dy * dy) <= TouchPointStore.TRAIN_TOLERANCE * 2f * w) {
                                        samples.add(TouchPoint(nx, ny))
                                        view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                                    }
                                }
                            }
                            else -> Unit
                        }
                    }
                }
        )
    }
}
