package com.wristkey

import android.content.Context
import android.content.Intent
import android.os.Bundle
import android.view.WindowManager
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.rememberLauncherForActivityResult
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.foundation.background
import androidx.compose.foundation.shape.RoundedCornerShape
import androidx.compose.foundation.text.BasicTextField
import androidx.compose.foundation.text.KeyboardOptions
import androidx.compose.runtime.*
import androidx.compose.runtime.rememberCoroutineScope
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.draw.clip
import androidx.compose.ui.text.input.KeyboardType
import androidx.compose.ui.text.input.PasswordVisualTransformation
import androidx.compose.ui.text.input.VisualTransformation
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.platform.LocalContext
import androidx.compose.ui.res.stringResource
import androidx.wear.compose.material.*
import androidx.wear.compose.navigation.SwipeDismissableNavHost
import androidx.wear.compose.navigation.composable
import androidx.wear.compose.navigation.rememberSwipeDismissableNavController
import com.wristkey.ble.WristKeyBleService
import com.wristkey.net.LanClient
import com.wristkey.security.KeyStoreManager
import com.wristkey.security.TouchPointStore
import com.wristkey.ui.TrainingActivity
import kotlinx.coroutines.Dispatchers
import kotlinx.coroutines.launch

class SettingsActivity : ComponentActivity() {
    companion object {
        const val EXTRA_START_ROUTE = "start_route"
        const val ROUTE_PAIRED_DEVICES = "paired_devices"
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        val startRoute = intent.getStringExtra(EXTRA_START_ROUTE) ?: "main"
        setContent {
            MaterialTheme {
                SettingsNavHost(startRoute)
            }
        }
    }
}

@Composable
fun SettingsNavHost(startRoute: String = "main") {
    val navController = rememberSwipeDismissableNavController()
    val context = LocalContext.current
    val settings = remember { WristKeySettings(context) }

    SwipeDismissableNavHost(
        navController = navController,
        startDestination = startRoute
    ) {
        composable("main") { MainSettingsScreen(navController, settings) }
        composable("confirm_mode") { ConfirmModeScreen(navController, settings) }
        composable("rssi_threshold") { RssiThresholdScreen(navController, settings) }
        composable("paired_devices") { PairedDevicesScreen(navController, settings) }
        composable("lan_setup") { LanSetupScreen(navController, settings) }
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
        WristKeySettings.CONFIRM_GESTURE -> stringResource(R.string.confirm_gesture_only)
        WristKeySettings.CONFIRM_BUTTON -> stringResource(R.string.confirm_button_only)
        else -> stringResource(R.string.confirm_gesture_or_button)
    }

    val touchContext = LocalContext.current
    val touchTrained = remember { mutableStateOf(TouchPointStore(touchContext).isTrained()) }
    // Real pairing lives in the BLE service prefs; WristKeySettings.pairedDevices
    // is a legacy set that is never written, which is why the count showed 0.
    val pairedPrefs = remember { touchContext.getSharedPreferences(WristKeyBleService.PREFS_NAME, Context.MODE_PRIVATE) }
    val pairedAddress = pairedPrefs.getString(WristKeyBleService.PREFS_PAIRED_ADDRESS, null)
    val pairedName = pairedPrefs.getString(WristKeyBleService.PREFS_PAIRED_NAME, null)
    val pairedCount = if (pairedAddress.isNullOrEmpty()) 0 else 1

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
                    text = stringResource(R.string.title_settings),
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                Chip(
                    label = { Text(stringResource(R.string.settings_confirmation)) },
                    secondaryLabel = { Text(confirmModeLabel) },
                    onClick = { navController.navigate("confirm_mode") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text(stringResource(R.string.settings_unlock_distance)) },
                    secondaryLabel = { Text("${settings.rssiThreshold} dBm") },
                    onClick = { navController.navigate("rssi_threshold") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text(stringResource(R.string.settings_proximity_unlock)) },
                    secondaryLabel = { Text(if (settings.proximityUnlockEnabled) stringResource(R.string.settings_state_on) else stringResource(R.string.settings_state_off)) },
                    onClick = { navController.navigate("proximity_unlock") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text(stringResource(R.string.settings_paired_pcs)) },
                    secondaryLabel = {
                        Text(
                            if (pairedCount == 0) "0${stringResource(R.string.settings_devices_suffix)}"
                            else "$pairedCount${stringResource(R.string.settings_devices_suffix)} · ${pairedName ?: stringResource(R.string.label_pc)}"
                        )
                    },
                    onClick = { navController.navigate("paired_devices") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                val lanRegistered = settings.lanWristkeyId != null
                Chip(
                    label = { Text(stringResource(R.string.settings_lan_server)) },
                    secondaryLabel = { Text(if (lanRegistered) stringResource(R.string.lan_registered) else stringResource(R.string.lan_not_registered)) },
                    onClick = { navController.navigate("lan_setup") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Chip(
                    label = { Text(stringResource(R.string.settings_touch_point)) },
                    secondaryLabel = { Text(if (touchTrained.value) stringResource(R.string.settings_touch_trained) else stringResource(R.string.settings_touch_not_set)) },
                    onClick = { navController.navigate("touch_point") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                ToggleChip(
                    checked = settings.vibrateEnabled,
                    onCheckedChange = { settings.vibrateEnabled = it },
                    label = { Text(stringResource(R.string.settings_vibration)) },
                    toggleControl = { Switch(checked = settings.vibrateEnabled) },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                val context = LocalContext.current
                Chip(
                    label = { Text(stringResource(R.string.settings_reset_all)) },
                    onClick = {
                        settings.reset()
                        Toast.makeText(context, context.getString(R.string.toast_settings_reset), Toast.LENGTH_SHORT).show()
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
                    text = stringResource(R.string.title_how_to_confirm),
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                Text(
                    text = stringResource(R.string.desc_choose_confirm),
                    style = MaterialTheme.typography.caption2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 16.dp).padding(bottom = 8.dp)
                )
            }

            val modes = listOf(
                Triple(WristKeySettings.CONFIRM_GESTURE, context.getString(R.string.mode_gesture_title), context.getString(R.string.mode_gesture_desc)),
                Triple(WristKeySettings.CONFIRM_BUTTON, context.getString(R.string.mode_button_title), context.getString(R.string.mode_button_desc)),
                Triple(WristKeySettings.CONFIRM_EITHER, context.getString(R.string.mode_either_title), context.getString(R.string.mode_either_desc))
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
                    text = stringResource(R.string.title_distance),
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                val label = when {
                    sliderValue >= -30f -> stringResource(R.string.distance_touching)
                    sliderValue >= -45f -> stringResource(R.string.distance_very_close)
                    sliderValue >= -60f -> stringResource(R.string.distance_near_monitor)
                    sliderValue >= -75f -> stringResource(R.string.distance_same_room)
                    else -> stringResource(R.string.distance_far)
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
                    decreaseIcon = { Text(stringResource(R.string.btn_minus)) },
                    increaseIcon = { Text(stringResource(R.string.btn_plus)) },
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
                    Text(stringResource(R.string.btn_save))
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
                    text = stringResource(R.string.title_proximity),
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                ToggleChip(
                    checked = enabled,
                    onCheckedChange = { enabled = it },
                    label = { Text(stringResource(R.string.proximity_auto_unlock)) },
                    secondaryLabel = { Text(stringResource(R.string.proximity_no_button)) },
                    toggleControl = { Switch(checked = enabled) },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            if (enabled) {
                item {
                    val label = when {
                        sliderValue >= -30f -> stringResource(R.string.proximity_touching_monitor)
                        sliderValue >= -40f -> stringResource(R.string.proximity_very_close)
                        else -> stringResource(R.string.proximity_close)
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
                    decreaseIcon = { Text(stringResource(R.string.btn_minus)) },
                    increaseIcon = { Text(stringResource(R.string.btn_plus)) },
                        modifier = Modifier.fillMaxWidth(0.8f)
                    )
                }

                item {
                    Text(
                        text = stringResource(R.string.proximity_help),
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
                    Text(stringResource(R.string.btn_save))
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
                    text = stringResource(R.string.title_paired_pcs),
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            if (pairedAddress == null) {
                item {
                    Text(
                        text = stringResource(R.string.empty_paired_pcs),
                        style = MaterialTheme.typography.body1,
                        textAlign = TextAlign.Center,
                        modifier = Modifier.padding(16.dp)
                    )
                }
            } else {
                item {
                    Chip(
                        label = { Text(pairedName ?: stringResource(R.string.label_pc)) },
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
                            Toast.makeText(context, context.getString(R.string.toast_pc_removed), Toast.LENGTH_SHORT).show()
                        },
                        colors = ButtonDefaults.secondaryButtonColors(),
                        modifier = Modifier.padding(top = 6.dp)
                    ) { Text(stringResource(R.string.btn_forget_pc)) }
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
                    text = stringResource(R.string.title_touch_point),
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 8.dp)
                )
            }

            item {
                Text(
                    text = if (trained)
                        stringResource(R.string.touch_point_trained)
                    else
                        stringResource(R.string.touch_point_not_set),
                    style = MaterialTheme.typography.caption2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.padding(horizontal = 16.dp).padding(bottom = 8.dp)
                )
            }

            item {
                Button(
                    onClick = { trainingLauncher.launch(Intent(context, TrainingActivity::class.java)) },
                    modifier = Modifier.padding(top = 8.dp)
                ) { Text(if (trained) stringResource(R.string.btn_retrain_point) else stringResource(R.string.btn_train_point)) }
            }

            if (trained) {
                item {
                    Button(
                        onClick = {
                            store.clear()
                            trained = false
                            Toast.makeText(context, context.getString(R.string.toast_point_cleared), Toast.LENGTH_SHORT).show()
                        },
                        colors = ButtonDefaults.secondaryButtonColors(),
                        modifier = Modifier.padding(top = 6.dp)
                    ) { Text(stringResource(R.string.btn_reset_point)) }
                }
            }
        }
    }
}

@Composable
private fun LanTextField(
    value: String,
    onValueChange: (String) -> Unit,
    label: String,
    placeholder: String = "",
    keyboardType: KeyboardType = KeyboardType.Text,
    isPassword: Boolean = false,
    modifier: Modifier = Modifier
) {
    Column(modifier = modifier.padding(horizontal = 6.dp, vertical = 3.dp)) {
        Text(
            text = label,
            style = MaterialTheme.typography.caption2,
            color = MaterialTheme.colors.secondary,
            modifier = Modifier.padding(start = 4.dp, bottom = 2.dp)
        )
        Box(
            modifier = Modifier
                .fillMaxWidth()
                .clip(RoundedCornerShape(12.dp))
                .background(MaterialTheme.colors.surface)
                .padding(horizontal = 12.dp, vertical = 8.dp)
        ) {
            BasicTextField(
                value = value,
                onValueChange = onValueChange,
                modifier = Modifier.fillMaxWidth(),
                textStyle = MaterialTheme.typography.body2,
                singleLine = true,
                keyboardOptions = KeyboardOptions(keyboardType = keyboardType),
                visualTransformation = if (isPassword) PasswordVisualTransformation() else VisualTransformation.None,
                decorationBox = { inner ->
                    if (value.isEmpty()) {
                        Text(
                            text = placeholder,
                            style = MaterialTheme.typography.caption2,
                            color = MaterialTheme.colors.onSurface.copy(alpha = 0.4f)
                        )
                    }
                    inner()
                }
            )
        }
    }
}

@Composable
fun LanSetupScreen(
    navController: androidx.navigation.NavHostController,
    settings: WristKeySettings
) {
    val listState = rememberScalingLazyListState()
    val context = LocalContext.current
    val scope = rememberCoroutineScope()
    val keyStoreManager = remember { KeyStoreManager() }

    val pairedPrefs = remember { context.getSharedPreferences(WristKeyBleService.PREFS_NAME, Context.MODE_PRIVATE) }
    val defaultPcName = pairedPrefs.getString(WristKeyBleService.PREFS_PAIRED_NAME, null)

    var serverUrl by remember { mutableStateOf(settings.lanServerUrl) }
    var token by remember { mutableStateOf(settings.lanToken) }
    var pcName by remember { mutableStateOf(settings.lanPcName.takeIf { it.isNotBlank() } ?: (defaultPcName ?: "")) }
    var msaAccount by remember { mutableStateOf(settings.lanMsaAccount) }
    var registeredId by remember { mutableStateOf(settings.lanWristkeyId) }
    var statusText by remember { mutableStateOf<String?>(null) }
    var busy by remember { mutableStateOf(false) }

    fun persistFields() {
        settings.lanServerUrl = serverUrl
        settings.lanToken = token
        settings.lanPcName = pcName
        settings.lanMsaAccount = msaAccount
    }

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
                    text = stringResource(R.string.lan_title),
                    style = MaterialTheme.typography.title3,
                    modifier = Modifier.padding(top = 16.dp, bottom = 4.dp)
                )
            }

            item {
                LanTextField(
                    value = serverUrl,
                    onValueChange = { serverUrl = it },
                    label = context.getString(R.string.lan_server_url),
                    placeholder = context.getString(R.string.lan_placeholder_server),
                    keyboardType = KeyboardType.Uri,
                    modifier = Modifier.fillMaxWidth(0.92f)
                )
            }

            item {
                LanTextField(
                    value = token,
                    onValueChange = { token = it },
                    label = context.getString(R.string.lan_token),
                    placeholder = context.getString(R.string.lan_placeholder_token),
                    isPassword = true,
                    modifier = Modifier.fillMaxWidth(0.92f)
                )
            }

            item {
                LanTextField(
                    value = pcName,
                    onValueChange = { pcName = it },
                    label = context.getString(R.string.lan_pc_name),
                    placeholder = context.getString(R.string.lan_placeholder_pc),
                    modifier = Modifier.fillMaxWidth(0.92f)
                )
            }

            item {
                LanTextField(
                    value = msaAccount,
                    onValueChange = { msaAccount = it },
                    label = context.getString(R.string.lan_msa_account),
                    placeholder = context.getString(R.string.lan_placeholder_account),
                    keyboardType = KeyboardType.Email,
                    modifier = Modifier.fillMaxWidth(0.92f)
                )
            }

            statusText?.let { message ->
                item {
                    Text(
                        text = message,
                        style = MaterialTheme.typography.caption2,
                        textAlign = TextAlign.Center,
                        color = MaterialTheme.colors.secondary,
                        modifier = Modifier.padding(horizontal = 12.dp, vertical = 4.dp)
                    )
                }
            }

            item {
                Button(
                    onClick = {
                        persistFields()
                        Toast.makeText(context, context.getString(R.string.lan_saved), Toast.LENGTH_SHORT).show()
                    },
                    modifier = Modifier.fillMaxWidth(0.92f)
                ) { Text(stringResource(R.string.lan_save)) }
            }

            item {
                Button(
                    enabled = !busy,
                    onClick = {
                        if (serverUrl.isBlank()) {
                            statusText = context.getString(R.string.lan_err, context.getString(R.string.lan_missing))
                            return@Button
                        }
                        busy = true
                        statusText = context.getString(R.string.lan_checking)
                        scope.launch(Dispatchers.IO) {
                            val result = try {
                                LanClient.health(serverUrl, token.takeIf { it.isNotBlank() })
                                context.getString(R.string.lan_health_ok)
                            } catch (e: Exception) {
                                context.getString(R.string.lan_err, e.message)
                            }
                            scope.launch(Dispatchers.Main) {
                                busy = false
                                statusText = result
                            }
                        }
                    },
                    colors = ButtonDefaults.secondaryButtonColors(),
                    modifier = Modifier.fillMaxWidth(0.92f)
                ) { Text(stringResource(R.string.lan_check)) }
            }

            item {
                Button(
                    enabled = !busy,
                    onClick = {
                        val url = serverUrl.trim()
                        val pc = pcName.trim()
                        val account = msaAccount.trim()
                        if (url.isBlank() || pc.isBlank() || account.isBlank()) {
                            statusText = context.getString(R.string.lan_err, context.getString(R.string.lan_missing))
                            return@Button
                        }
                        persistFields()
                        busy = true
                        statusText = context.getString(R.string.lan_linking)
                        scope.launch(Dispatchers.IO) {
                            val result = try {
                                val resp = LanClient.register(
                                    baseUrl = url,
                                    token = token.takeIf { it.isNotBlank() },
                                    pcName = pc,
                                    msaAccount = account,
                                    watchPubkey = keyStoreManager.getPublicKey(),
                                    sign = keyStoreManager::signChallenge
                                )
                                val id = resp.getString("wristkey_id")
                                settings.saveLanBinding(id)
                                context.getString(R.string.lan_status_linked, id)
                            } catch (e: Exception) {
                                context.getString(R.string.lan_err, e.message ?: "unknown")
                            }
                            scope.launch(Dispatchers.Main) {
                                busy = false
                                statusText = result
                                registeredId = settings.lanWristkeyId
                            }
                        }
                    },
                    modifier = Modifier.fillMaxWidth(0.92f)
                ) { Text(stringResource(R.string.lan_register)) }
            }

            if (registeredId != null) {
                item {
                    Button(
                        onClick = {
                            settings.clearLanBinding()
                            registeredId = null
                            statusText = null
                        },
                        colors = ButtonDefaults.secondaryButtonColors(),
                        modifier = Modifier.fillMaxWidth(0.92f)
                    ) { Text(stringResource(R.string.lan_unlink)) }
                }
            }
        }
    }
}
