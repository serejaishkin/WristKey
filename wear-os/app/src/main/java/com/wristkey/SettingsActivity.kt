package com.wristkey

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.LocalContext
import androidx.wear.compose.material.*
import androidx.wear.compose.navigation.SwipeDismissableNavHost
import androidx.wear.compose.navigation.composable
import androidx.wear.compose.navigation.rememberSwipeDismissableNavController
import com.wristkey.ble.WristKeyBleService
import com.wristkey.security.TouchPointStore
import com.wristkey.ui.TrainingActivity

class SettingsActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            MaterialTheme {
                SettingsNavHost()
            }
        }
    }
}

@Composable
fun SettingsNavHost() {
    val navController = rememberSwipeDismissableNavController()
    val context = LocalContext.current
    val settings = remember { WristKeySettings(context) }

    SwipeDismissableNavHost(
        navController = navController,
        startDestination = "main"
    ) {
        composable("main") { MainSettingsScreen(navController, settings) }
        composable("confirm_mode") { ConfirmModeScreen(navController, settings) }
        composable("rssi_threshold") { RssiThresholdScreen(navController, settings) }
        composable("paired_devices") { PairedDevicesScreen(navController, settings) }
        composable("proximity_unlock") { ProximityUnlockScreen(navController, settings) }
        composable("calibration") { CalibrationScreen(navController, settings, null) }
        composable("touch_point") { TouchPointScreen(navController) }
    }
}

@Composable
fun MainSettingsScreen(
    navController: androidx.navigation.NavHostController,
    settings: WristKeySettings
) {
    val confirmModeLabel = when (settings.confirmMode) {
        WristKeySettings.CONFIRM_GESTURE -> "Gesture only"
        WristKeySettings.CONFIRM_BUTTON -> "Button only"
        else -> "Gesture or button"
    }

    val touchContext = LocalContext.current
    val touchTrained = remember { mutableStateOf(TouchPointStore(touchContext).isTrained()) }

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
                    text = "⚙ WristKey",
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                Chip(
                    label = { Text("Confirmation") },
                    secondaryLabel = { Text(confirmModeLabel) },
                    onClick = { navController.navigate("confirm_mode") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text("Unlock distance") },
                    secondaryLabel = { Text("${settings.rssiThreshold} dBm") },
                    onClick = { navController.navigate("rssi_threshold") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text("Proximity unlock") },
                    secondaryLabel = { Text(if (settings.proximityUnlockEnabled) "ON" else "OFF") },
                    onClick = { navController.navigate("proximity_unlock") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text("Paired PCs") },
                    secondaryLabel = { Text("${settings.pairedDevices.size} devices") },
                    onClick = { navController.navigate("paired_devices") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text("Touch point") },
                    secondaryLabel = { Text(if (touchTrained.value) "Trained" else "Not set") },
                    onClick = { navController.navigate("touch_point") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                ToggleChip(
                    checked = settings.vibrateEnabled,
                    onCheckedChange = { settings.vibrateEnabled = it },
                    label = { Text("Vibration") },
                    toggleControl = { Switch(checked = settings.vibrateEnabled) },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                val context = LocalContext.current
                Chip(
                    label = { Text("Reset all settings") },
                    onClick = {
                        settings.reset()
                        Toast.makeText(context, "Settings reset", Toast.LENGTH_SHORT).show()
                    },
                    colors = ChipDefaults.primaryChipColors(),
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }
        }
    }
}

@Composable
fun ConfirmModeScreen(
    navController: androidx.navigation.NavHostController,
    settings: WristKeySettings
) {
    val listState = rememberScalingLazyListState()
    var selected by remember { mutableStateOf(settings.confirmMode) }

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
                    text = "How to confirm",
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                Text(
                    text = "Choose how you confirm unlock requests",
                    style = MaterialTheme.typography.caption2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 16.dp).padding(bottom = 8.dp)
                )
            }

            val modes = listOf(
                Triple(WristKeySettings.CONFIRM_GESTURE, "🤚 Gesture", "Shake or move wrist"),
                Triple(WristKeySettings.CONFIRM_BUTTON, "🔘 Button", "Press the watch button"),
                Triple(WristKeySettings.CONFIRM_EITHER, "🤚🔘 Either", "Gesture or button")
            )

            items(modes.size) { index ->
                val (mode, title, desc) = modes[index]
                ToggleChip(
                    checked = selected == mode,
                    onCheckedChange = {
                        selected = mode
                        settings.confirmMode = mode
                    },
                    label = { Text(title) },
                    secondaryLabel = { Text(desc) },
                    toggleControl = { RadioButton(selected = selected == mode) },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }
        }
    }
}

@Composable
fun RssiThresholdScreen(
    navController: androidx.navigation.NavHostController,
    settings: WristKeySettings
) {
    val listState = rememberScalingLazyListState()
    var sliderValue by remember { mutableFloatStateOf(settings.rssiThreshold.toFloat()) }

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
                    text = "📡 Distance",
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                val label = when {
                    sliderValue >= -30f -> "Touching (≤5 cm)"
                    sliderValue >= -45f -> "Very close (≤20 cm)"
                    sliderValue >= -60f -> "Near monitor (≤1 m)"
                    sliderValue >= -75f -> "Same room (≤3 m)"
                    else -> "Far away"
                }
                Text(
                    text = label,
                    style = MaterialTheme.typography.title2,
                    modifier = Modifier.padding(vertical = 8.dp)
                )
            }

            item {
                Text(
                    text = "${sliderValue.toInt()} dBm",
                    style = MaterialTheme.typography.display3,
                    modifier = Modifier.padding(vertical = 4.dp)
                )
            }

            item {
                InlineSlider(
                    value = sliderValue,
                    onValueChange = { sliderValue = it },
                    valueRange = -90f..-20f,
                    steps = 14,
                    decreaseIcon = { Text("-") },
                    increaseIcon = { Text("+") },
                    modifier = Modifier.fillMaxWidth(0.8f)
                )
            }

            item {
                Button(
                    onClick = {
                        settings.rssiThreshold = sliderValue.toInt()
                        navController.popBackStack()
                    },
                    modifier = Modifier.padding(top = 16.dp)
                ) {
                    Text("Save")
                }
            }
        }
    }
}

@Composable
fun ProximityUnlockScreen(
    navController: androidx.navigation.NavHostController,
    settings: WristKeySettings
) {
    val listState = rememberScalingLazyListState()
    var enabled by remember { mutableStateOf(settings.proximityUnlockEnabled) }
    var sliderValue by remember { mutableFloatStateOf(settings.proximityRssi.toFloat()) }

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
                    text = "⚡ Proximity",
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                ToggleChip(
                    checked = enabled,
                    onCheckedChange = { enabled = it },
                    label = { Text("Auto-unlock") },
                    secondaryLabel = { Text("No button/gesture needed") },
                    toggleControl = { Switch(checked = enabled) },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            if (enabled) {
                item {
                    val label = when {
                        sliderValue >= -30f -> "Touching monitor"
                        sliderValue >= -40f -> "Very close"
                        else -> "Close"
                    }
                    Text(
                        text = label,
                        style = MaterialTheme.typography.title2,
                        modifier = Modifier.padding(top = 8.dp)
                    )
                }

                item {
                    Text(
                        text = "${sliderValue.toInt()} dBm",
                        style = MaterialTheme.typography.display3
                    )
                }

                item {
                    InlineSlider(
                        value = sliderValue,
                        onValueChange = { sliderValue = it },
                        valueRange = -60f..-20f,
                        steps = 8,
                        decreaseIcon = { Text("-") },
                        increaseIcon = { Text("+") },
                        modifier = Modifier.fillMaxWidth(0.8f)
                    )
                }

                item {
                    Text(
                        text = "Bring watch this close to auto-unlock without confirmation",
                        style = MaterialTheme.typography.caption2,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(horizontal = 16.dp)
                    )
                }
            }

            item {
                Button(
                    onClick = {
                        settings.proximityUnlockEnabled = enabled
                        settings.proximityRssi = sliderValue.toInt()
                        navController.popBackStack()
                    },
                    modifier = Modifier.padding(top = 16.dp)
                ) {
                    Text("Save")
                }
            }
        }
    }
}

@Composable
fun PairedDevicesScreen(
    navController: androidx.navigation.NavHostController,
    settings: WristKeySettings
) {
    val listState = rememberScalingLazyListState()
    val context = LocalContext.current
    // The BLE service owns the real pairing record; read it directly instead
    // of the legacy WristKeySettings store which was always empty.
    val prefs = remember { context.getSharedPreferences(WristKeyBleService.PREFS_NAME, Context.MODE_PRIVATE) }
    var pairedName by remember { mutableStateOf(prefs.getString(WristKeyBleService.PREFS_PAIRED_NAME, null)) }
    var pairedAddress by remember { mutableStateOf(prefs.getString(WristKeyBleService.PREFS_PAIRED_ADDRESS, null)) }

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
                    text = "💻 Paired PCs",
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            if (pairedAddress == null) {
                item {
                    Text(
                        text = "Список ПК пуст.\nПодключите ПК с главной страницы.",
                        style = MaterialTheme.typography.body1,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(16.dp)
                    )
                }
            } else {
                item {
                    Chip(
                        label = { Text(pairedName ?: "ПК") },
                        secondaryLabel = { Text(pairedAddress ?: "") },
                        onClick = { },
                        modifier = Modifier.fillMaxWidth(0.9f)
                    )
                }
                item {
                    Button(
                        onClick = {
                            // Service clears pairing, regenerates the PIN and
                            // switches advertising back to new-PC mode.
                            context.sendBroadcast(Intent(WristKeyBleService.ACTION_FORGET_DEVICE).setPackage(context.packageName))
                            pairedName = null
                            pairedAddress = null
                            Toast.makeText(context, "Текущий ПК сброшен", Toast.LENGTH_SHORT).show()
                        },
                        colors = ButtonDefaults.secondaryButtonColors(),
                        modifier = Modifier.padding(top = 6.dp)
                    ) { Text("Забыть этот ПК") }
                }
            }
        }
    }
}

@Composable
fun TouchPointScreen(navController: androidx.navigation.NavHostController) {
    val listState = rememberScalingLazyListState()
    val context = LocalContext.current
    val store = remember { TouchPointStore(context) }
    var trained by remember { mutableStateOf(store.isTrained()) }

    val trainingLauncher = rememberLauncherForActivityResult(
        ActivityResultContracts.StartActivityForResult()
    ) { trained = store.isTrained() }

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
                    text = "👆 Touch point",
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                Text(
                    text = if (trained)
                        "Точка обучена: разблокировка подтверждается касанием в неё"
                    else
                        "Точка не настроена. Без неё unlock покажет кнопку.",
                    style = MaterialTheme.typography.caption2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 16.dp).padding(bottom = 8.dp)
                )
            }

            item {
                Button(
                    onClick = { trainingLauncher.launch(Intent(context, TrainingActivity::class.java)) },
                    modifier = Modifier.padding(top = 8.dp)
                ) { Text(if (trained) "Обучить заново" else "Обучить точку") }
            }

            if (trained) {
                item {
                    Button(
                        onClick = {
                            store.clear()
                            trained = false
                            Toast.makeText(context, "Point cleared", Toast.LENGTH_SHORT).show()
                        },
                        colors = ButtonDefaults.secondaryButtonColors(),
                        modifier = Modifier.padding(top = 6.dp)
                    ) { Text("Сбросить точку") }
                }
            }
        }
    }
}
