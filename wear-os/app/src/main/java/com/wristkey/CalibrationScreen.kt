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
                    text = "Калибровка запускается с ПК.\nОткройте WristKey на компьютере и нажмите 'Калибровка'.",
                    style = MaterialTheme.typography.body2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 16.dp)
                )
            }

            item {
                Spacer(modifier = Modifier.height(16.dp))
                if (settings.isProximityCalibrated) {
                    Text(
                        "✅ Калибровано:\n${settings.proximityRssi} dBm",
                        style = MaterialTheme.typography.caption2,
                        textAlign = TextAlign.Center
                    )
                } else {
                    Text(
                        "❌ Не откалибровано",
                        style = MaterialTheme.typography.caption2,
                        textAlign = TextAlign.Center
                    )
                }
            }

            item {
                Spacer(modifier = Modifier.height(16.dp))
                Chip(
                    onClick = { navController.popBackStack() },
                    label = { Text("Back") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }
        }
    }
}
