package com.wristkey

import android.os.Bundle
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.wear.compose.material.*
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.LocalContext
import androidx.wear.compose.navigation.SwipeDismissableNavHost
import androidx.wear.compose.navigation.composable
import androidx.wear.compose.navigation.rememberSwipeDismissableNavController

/**
 * Wear OS Settings screen for WristKey.
 * 
 * Screens:
 * 1. Main settings list
 * 2. Confirmation mode (gesture / button / either)
 * 3. RSSI threshold (distance) slider
 * 4. Paired devices list
 * 5. Proximity unlock settings
 */
class SettingsActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContent {
            WristKeyTheme {
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
                    icon = { Icon(androidx.wear.compose.material.IconDefaults.TapChipIcon, contentDescription = null) },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text("Unlock distance") },
                    secondaryLabel = { Text("${settings.rssiThreshold} dBm") },
                    onClick = { navController.navigate("rssi_threshold") },
                    icon = { Icon(androidx.wear.compose.material.IconDefaults.TapChipIcon, contentDescription = null) },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text("Proximity unlock") },
                    secondaryLabel = { Text(if (settings.proximityUnlockEnabled) "ON" else "OFF") },
                    onClick = { navController.navigate("proximity_unlock") },
                    icon = { Icon(androidx.wear.compose.material.IconDefaults.TapChipIcon, contentDescription = null) },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text("Paired PCs") },
                    secondaryLabel = { Text("${settings.pairedDevices.size} devices") },
                    onClick = { navController.navigate("paired_devices") },
                    icon = { Icon(androidx.wear.compose.material.IconDefaults.TapChipIcon, contentDescription = null) },
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
                    colors = ChipDefaults.primaryChipColors(
                        backgroundColor = MaterialTheme.colors.error
                    ),
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
                    modifier = Modifier.padding(horizontal = 16.dp, bottom = 8.dp)
                )
            }

            val modes = listOf(
                Triple(WristKeySettings.CONFIRM_GESTURE, "🤚 Gesture", "Shake or move wrist"),
                Triple(WristKeySettings.CONFIRM_BUTTON, "🔘 Button", "Press the watch button"),
                Triple(WristKeySettings.CONFIRM_EITHER, "🤚🔘 Either", "Gesture or button")
            )

            items(modes) { (mode, title, desc) ->
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
                    style = MaterialTheme.typology.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                val label = when {
                    sliderValue >= -30 -> "Touching (≤5 cm)"
                    sliderValue >= -45 -> "Very close (≤20 cm)"
                    sliderValue >= -60 -> "Near monitor (≤1 m)"
                    sliderValue >= -75 -> "Same room (≤3 m)"
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
                        sliderValue >= -30 -> "Touching monitor"
                        sliderValue >= -40 -> "Very close"
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
    val devices = remember { settings.pairedDevices.toList() }
    val context = LocalContext.current

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

            if (devices.isEmpty()) {
                item {
                    Text(
                        text = "No paired PCs yet.\nPair from the main screen.",
                        style = MaterialTheme.typography.body1,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(16.dp)
                    )
                }
            } else {
                items(devices) { deviceId ->
                    Chip(
                        label = { Text(deviceId.take(8) + "...") },
                        secondaryLabel = { Text("Paired") },
                        onClick = {
                            // Show confirmation dialog to forget
                            // For simplicity, just forget immediately
                            settings.removePairedDevice(deviceId)
                            Toast.makeText(context, "Forgot device", Toast.LENGTH_SHORT).show()
                        },
                        icon = { Icon(androidx.wear.compose.material.IconDefaults.TapChipIcon, contentDescription = null) },
                        modifier = Modifier.fillMaxWidth(0.9f)
                    )
                }
            }
        }
    }
}
