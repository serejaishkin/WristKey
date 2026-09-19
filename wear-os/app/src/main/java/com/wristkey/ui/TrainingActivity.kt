package com.wristkey.ui

import android.app.Activity
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.IBinder
import android.view.HapticFeedbackConstants
import android.view.WindowManager
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material.Text
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.platform.LocalView
import androidx.compose.ui.res.stringResource
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.wear.compose.material.Button
import androidx.wear.compose.material.ButtonDefaults
import androidx.wear.compose.material.MaterialTheme
import com.wristkey.R
import com.wristkey.ble.WristKeyBleService
import com.wristkey.security.TouchPointStore
import kotlinx.coroutines.delay

class TrainingActivity : ComponentActivity() {
    private var bleService: WristKeyBleService? = null
    private var bound = false

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            bleService = (service as WristKeyBleService.LocalBinder).getService()
            bound = true
        }
        override fun onServiceDisconnected(name: ComponentName?) { bleService = null; bound = false }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        Intent(this, WristKeyBleService::class.java).also {
            bindService(it, connection, Context.BIND_AUTO_CREATE)
        }
        setContent {
            MaterialTheme {
                TrainingScreen(
                    bleService = { bleService },
                    onDone = { setResult(Activity.RESULT_OK); finish() },
                    onCancelled = { setResult(Activity.RESULT_CANCELED); finish() }
                )
            }
        }
    }

    override fun onDestroy() {
        if (bound) { unbindService(connection); bound = false }
        super.onDestroy()
    }
}

private enum class TrainPhase { PREP, RECORD, DONE }

private const val PREP_SECONDS = 10
private const val RECORD_SECONDS = 10

@Composable
fun TrainingScreen(bleService: () -> WristKeyBleService?, onDone: () -> Unit, onCancelled: () -> Unit) {
    val context = LocalContext.current
    val store = remember { TouchPointStore(context) }
    val view = LocalView.current
    val primaryColor = MaterialTheme.colors.primary

    var phase by remember { mutableStateOf(TrainPhase.PREP) }
    var secondsLeft by remember { mutableIntStateOf(PREP_SECONDS) }

    // Notify BLE service on phase changes so the PC can mirror the request.
    LaunchedEffect(phase) {
        val svc = bleService() ?: return@LaunchedEffect
        when (phase) {
            TrainPhase.PREP -> svc.sendTrainingState("prep", mapOf("countdown" to PREP_SECONDS))
            TrainPhase.RECORD -> svc.sendTrainingState("record", mapOf("countdown" to RECORD_SECONDS))
            TrainPhase.DONE -> svc.sendTrainingState("done")
        }
    }

    LaunchedEffect(phase) {
        when (phase) {
            TrainPhase.PREP -> {
                secondsLeft = PREP_SECONDS
                while (secondsLeft > 0) {
                    delay(1000)
                    if (phase != TrainPhase.PREP) return@LaunchedEffect
                    secondsLeft--
                    bleService()?.sendTrainingState("prep", mapOf("countdown" to secondsLeft))
                }
                view.performHapticFeedback(HapticFeedbackConstants.CLOCK_TICK)
                phase = TrainPhase.RECORD
            }
            TrainPhase.RECORD -> {
                secondsLeft = RECORD_SECONDS
                while (secondsLeft > 0) {
                    delay(1000)
                    if (phase != TrainPhase.RECORD) return@LaunchedEffect
                    secondsLeft--
                    bleService()?.sendTrainingState("record", mapOf("countdown" to secondsLeft))
                }
                store.markTrained()
                view.performHapticFeedback(HapticFeedbackConstants.LONG_PRESS)
                phase = TrainPhase.DONE
            }
            TrainPhase.DONE -> Unit
        }
    }

    Box(modifier = Modifier.fillMaxSize()) {
        Column(
            modifier = Modifier.fillMaxSize().padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center
        ) {
            when (phase) {
                TrainPhase.PREP -> {
                    Text(
                        stringResource(R.string.training_zone_prep),
                        style = MaterialTheme.typography.body1,
                        textAlign = TextAlign.Center,
                        color = primaryColor
                    )
                    Spacer(Modifier.height(12.dp))
                    Text(
                        stringResource(R.string.training_countdown, secondsLeft),
                        style = MaterialTheme.typography.display2,
                        textAlign = TextAlign.Center
                    )
                }
                TrainPhase.RECORD -> {
                    Text(
                        stringResource(R.string.training_zone_record),
                        style = MaterialTheme.typography.body1,
                        textAlign = TextAlign.Center,
                        color = Color(0xFF4CAF50)
                    )
                    Spacer(Modifier.height(12.dp))
                    Text(
                        stringResource(R.string.training_countdown, secondsLeft),
                        style = MaterialTheme.typography.display2,
                        textAlign = TextAlign.Center
                    )
                }
                TrainPhase.DONE -> {
                    Text(stringResource(R.string.training_zone_done), style = MaterialTheme.typography.title3, textAlign = TextAlign.Center, color = Color(0xFF4CAF50))
                    Spacer(Modifier.height(12.dp))
                    Button(onClick = onDone, modifier = Modifier.fillMaxWidth(0.6f)) { Text(stringResource(R.string.btn_done)) }
                }
            }
            if (phase != TrainPhase.DONE) {
                Spacer(Modifier.height(10.dp))
                Button(onClick = onCancelled, colors = ButtonDefaults.secondaryButtonColors(), modifier = Modifier.fillMaxWidth(0.5f)) { Text(stringResource(R.string.btn_cancel)) }
            }
        }
    }
}