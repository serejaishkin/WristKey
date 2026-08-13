package com.wristkey

import android.widget.Toast
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.unit.sp
import androidx.compose.ui.platform.LocalContext
import androidx.wear.compose.material.*
import androidx.navigation.NavHostController
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

@Composable
fun CalibrationScreenRoute(
    navController: NavHostController,
    settings: WristKeySettings,
    bleService: WristKeyBleService?
) {
    CalibrationScreen(navController, settings, bleService)
}

@Composable
fun CalibrationScreen(
    navController: androidx.navigation.NavHostController,
    settings: WristKeySettings,
    bleService: WristKeyBleService?
) {
    val context = LocalContext.current
    val scope = rememberCoroutineScope()

    var isCalibrating by remember { mutableStateOf(false) }
    var progress by remember { mutableFloatStateOf(0f) }
    var countdown by remember { mutableIntStateOf(10) }
    var resultText by remember { mutableStateOf("") }
    var showResult by remember { mutableStateOf(false) }

    val isCalibrated = settings.isProximityCalibrated
    val currentThreshold = settings.proximityRssi

    Scaffold(
        timeText = { TimeText() },
        vignette = { Vignette(vignettePosition = VignettePosition.TopAndBottom) }
    ) {
        Column(
            modifier = Modifier
                .fillMaxSize()
                .padding(16.dp),
            horizontalAlignment = Alignment.CenterHorizontally,
            verticalArrangement = Arrangement.Center
        ) {
            if (!isCalibrating && !showResult) {
                // Idle state
                Text(
                    text = "📏 Distance",
                    style = MaterialTheme.typography.title2,
                    modifier = Modifier.padding(bottom = 8.dp)
                )

                if (isCalibrated) {
                    Text(
                        text = "✅ Calibrated",
                        style = MaterialTheme.typography.body1,
                        color = MaterialTheme.colors.primary
                    )
                    Text(
                        text = "Touch threshold: ${currentThreshold} dBm",
                        style = MaterialTheme.typography.caption2,
                        modifier = Modifier.padding(bottom = 16.dp)
                    )
                } else {
                    Text(
                        text = "Not calibrated yet",
                        style = MaterialTheme.typography.body1,
                        color = MaterialTheme.colors.error
                    )
                    Text(
                        text = "Calibrate to enable touch-to-unlock",
                        style = MaterialTheme.typography.caption2,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(bottom = 16.dp)
                    )
                }

                Button(
                    onClick = {
                        bleService?.requestCalibration()
                        isCalibrating = true
                        progress = 0f
                        countdown = 10

                        scope.launch {
                            while (countdown > 0) {
                                delay(1000)
                                countdown--
                                progress = (10 - countdown) / 10f
                            }
                            isCalibrating = false
                            showResult = true
                            resultText = if (settings.isProximityCalibrated)
                                "Saved: ${settings.proximityRssi} dBm"
                            else "Waiting for PC..."
                        }
                    },
                    modifier = Modifier.fillMaxWidth(0.8f)
                ) {
                    Text(if (isCalibrated) "Recalibrate" else "Calibrate")
                }

                if (isCalibrated) {
                    Spacer(modifier = Modifier.height(8.dp))
                    Chip(
                        label = { Text("Reset calibration") },
                        onClick = {
                            settings.clearCalibration()
                            Toast.makeText(context, "Calibration reset", Toast.LENGTH_SHORT).show()
                        },
                        modifier = Modifier.fillMaxWidth(0.8f)
                    )
                }
            } else if (isCalibrating) {
                // Calibration in progress
                Text(
                    text = "📡 Calibrating...",
                    style = MaterialTheme.typography.title2,
                    modifier = Modifier.padding(bottom = 16.dp)
                )

                Box(
                    modifier = Modifier
                        .size(80.dp)
                        .padding(bottom = 16.dp),
                    contentAlignment = Alignment.Center
                ) {
                    Text("⌚", fontSize = 48.sp)
                }

                Text(
                    text = "Bring your watch close to the monitor",
                    style = MaterialTheme.typography.body1,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 16.dp, vertical = 8.dp)
                )

                Text(
                    text = "Hold for $countdown seconds...",
                    style = MaterialTheme.typography.title3,
                    color = MaterialTheme.colors.primary,
                    modifier = Modifier.padding(bottom = 16.dp)
                )

                CircularProgressIndicator(
                    progress = progress,
                    modifier = Modifier.size(60.dp),
                    strokeWidth = 6.dp
                )

                Spacer(modifier = Modifier.height(16.dp))
                Chip(
                    label = { Text("Cancel") },
                    onClick = {
                        isCalibrating = false
                        bleService?.cancelCalibration()
                    }
                )
            } else if (showResult) {
                // Result
                Text(
                    text = if (settings.isProximityCalibrated) "✅ Done!" else "⏳ Waiting...",
                    style = MaterialTheme.typography.title2,
                    modifier = Modifier.padding(bottom = 8.dp)
                )
                Text(
                    text = resultText,
                    style = MaterialTheme.typography.body1,
                    textAlign = TextAlign.Center
                )
                Spacer(modifier = Modifier.height(16.dp))
                Button(
                    onClick = {
                        showResult = false
                        navController.popBackStack()
                    }
                ) {
                    Text("OK")
                }
            }
        }
    }
}
