package com.wristkey

import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.navigation.NavHostController
import androidx.wear.compose.material.*
import com.wristkey.ble.WristKeyBleService

@Composable
fun CalibrationScreen(
    navController: NavHostController,
    settings: WristKeySettings,
    bleService: WristKeyBleService?
) {
    val listState = rememberScalingLazyListState()
    var isCalibrating by remember { mutableStateOf(false) }
    var progress by remember { mutableStateOf(0) }

    Scaffold(
        timeText = { TimeText() },
        vignette = { Vignette(vignettePosition = VignettePosition.TopAndBottom) },
        positionIndicator = { PositionIndicator(scalingLazyListState = listState) }
    ) {
        ScalingLazyColumn(
            modifier = Modifier.fillMaxSize(),
            state = listState,
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            item {
                Text(
                    text = "📏 Calibrate",
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                Text(
                    text = if (isCalibrating) "Hold watch near PC..." else "Calibrate proximity",
                    style = MaterialTheme.typography.body2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 16.dp)
                )
            }

            item {
                Spacer(modifier = Modifier.height(16.dp))
                if (isCalibrating) {
                    CircularProgressIndicator()
                    Spacer(modifier = Modifier.height(8.dp))
                    Text("$progress/20", style = MaterialTheme.typography.display3)
                } else {
                    Button(
                        onClick = {
                            isCalibrating = true
                            progress = 0
                            bleService?.requestCalibration()
                        },
                        modifier = Modifier.fillMaxWidth(0.7f)
                    ) {
                        Text("Start")
                    }
                }
            }

            item {
                Spacer(modifier = Modifier.height(16.dp))
                if (settings.isProximityCalibrated) {
                    Text(
                        "✅ Calibrated: ${settings.proximityRssi} dBm",
                        style = MaterialTheme.typography.caption2
                    )
                } else {
                    Text(
                        "❌ Not calibrated",
                        style = MaterialTheme.typography.caption2
                    )
                }
            }

            item {
                Spacer(modifier = Modifier.height(8.dp))
                Chip(
                    onClick = { navController.popBackStack() },
                    label = { Text("Back") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }
        }
    }
}
