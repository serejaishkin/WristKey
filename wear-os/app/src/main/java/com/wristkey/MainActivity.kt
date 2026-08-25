package com.wristkey

import android.Manifest
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.view.WindowManager
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.background
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.graphics.Color
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.core.content.ContextCompat
import androidx.wear.compose.material.*
import com.wristkey.ble.WristKeyBleService
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {
    private var bleService: WristKeyBleService? = null
    private var serviceBound = false

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as WristKeyBleService.LocalBinder
            bleService = binder.getService()
            serviceBound = true
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            bleService = null
            serviceBound = false
        }
    }

    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions ->
        if (!permissions.entries.all { it.value }) {
            Toast.makeText(this, "Bluetooth permissions required", Toast.LENGTH_LONG).show()
        } else {
            bindAndStartService()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)

        if (!hasRequiredPermissions()) requestPermissions() else bindAndStartService()

        setContent {
            MaterialTheme {
                MainScreen()
            }
        }
    }

    override fun onDestroy() {
        if (serviceBound) {
            unbindService(serviceConnection)
            serviceBound = false
        }
        super.onDestroy()
    }

    private fun hasRequiredPermissions(): Boolean {
        val permissions = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            arrayOf(
                Manifest.permission.BLUETOOTH_ADVERTISE,
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.BLUETOOTH_SCAN
            )
        } else {
            arrayOf(
                Manifest.permission.BLUETOOTH,
                Manifest.permission.BLUETOOTH_ADMIN,
                Manifest.permission.ACCESS_FINE_LOCATION
            )
        }
        return permissions.all {
            ContextCompat.checkSelfPermission(this, it) == PackageManager.PERMISSION_GRANTED
        }
    }

    private fun requestPermissions() {
        val permissions = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            arrayOf(
                Manifest.permission.BLUETOOTH_ADVERTISE,
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.BLUETOOTH_SCAN
            )
        } else {
            arrayOf(
                Manifest.permission.BLUETOOTH,
                Manifest.permission.BLUETOOTH_ADMIN,
                Manifest.permission.ACCESS_FINE_LOCATION
            )
        }
        permissionLauncher.launch(permissions)
    }

    private fun bindAndStartService() {
        val intent = Intent(this, WristKeyBleService::class.java)
        bindService(intent, serviceConnection, Context.BIND_AUTO_CREATE)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(intent)
        } else {
            startService(intent)
        }
    }

    @Composable
    private fun MainScreen() {
        var pin by remember { mutableStateOf("----") }
        var statusText by remember { mutableStateOf("Запуск…") }
        var paired by remember { mutableStateOf(false) }
        var advertising by remember { mutableStateOf(false) }
        var pairingRequested by remember { mutableStateOf(false) }
        var showNewPcConfirm by remember { mutableStateOf(false) }
        var newSetupMode by remember { mutableStateOf(false) }

        LaunchedEffect(Unit) {
            while (true) {
                val svc = bleService
                paired = svc?.isPaired() == true
                pairingRequested = svc?.pairingRequested?.get() == true
                advertising = svc?.isAdvertising() == true
                pin = if (!paired || newSetupMode) svc?.getAdvertisePin() ?: "----" else "----"

                val pcName = svc?.getRequestingPcName()
                statusText = when {
                    svc == null -> "Подключение службы…"
                    pairingRequested -> if (pcName != null) "ПК «$pcName»\nзапрашивает сопряжение" else "ПК запрашивает\nсопряжение"
                    paired -> "Последний ПК:\n${svc.getPairedDeviceName() ?: "ПК"}"
                    else -> "ПК не настроен"
                }
                delay(500)
            }
        }

        Box(modifier = Modifier.fillMaxSize()) {
            val listState = rememberScalingLazyListState()

            Scaffold(
                timeText = { TimeText() },
                vignette = { Vignette(vignettePosition = VignettePosition.TopAndBottom) },
                positionIndicator = { PositionIndicator(scalingLazyListState = listState) }
            ) {
                ScalingLazyColumn(
                    modifier = Modifier.fillMaxSize(),
                    state = listState,
                    contentPadding = PaddingValues(top = 10.dp, bottom = 28.dp, start = 6.dp, end = 6.dp),
                    verticalArrangement = Arrangement.spacedBy(5.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    item {
                        Text("⌚ WristKey", style = MaterialTheme.typography.title2, modifier = Modifier.padding(vertical = 4.dp))
                    }
                    item {
                        Text(statusText, style = MaterialTheme.typography.caption2, textAlign = TextAlign.Center, modifier = Modifier.padding(horizontal = 12.dp))
                    }
                    if (!paired || newSetupMode) {
                        item { Text("Код настройки", style = MaterialTheme.typography.caption3) }
                        item { Text(pin, style = MaterialTheme.typography.display1, color = MaterialTheme.colors.primary) }
                    }
                    if (pairingRequested) {
                        item {
                            Button(
                                onClick = {
                                    val ok = bleService?.confirmPairing() == true
                                    if (ok) {
                                        newSetupMode = false
                                        Toast.makeText(this@MainActivity, "ПК сохранён", Toast.LENGTH_SHORT).show()
                                    } else {
                                        Toast.makeText(this@MainActivity, "Сопряжение не удалось", Toast.LENGTH_SHORT).show()
                                    }
                                },
                                modifier = Modifier.fillMaxWidth(0.82f)
                            ) { Text("Подтвердить") }
                        }
                    }
                    if (paired) {
                        item {
                            Chip(
                                label = { Text(if (advertising) "↻ Подключить последний ПК" else "▶ Подключить последний ПК") },
                                onClick = {
                                    bleService?.startAdvertising()
                                    Toast.makeText(this@MainActivity, "Ожидаю подключение последнего ПК", Toast.LENGTH_SHORT).show()
                                },
                                modifier = Modifier.fillMaxWidth(0.92f)
                            )
                        }
                    }
                    item {
                        Chip(
                            label = { Text("⚙ Настройки") },
                            onClick = { startActivity(Intent(this@MainActivity, SettingsActivity::class.java)) },
                            modifier = Modifier.fillMaxWidth(0.82f)
                        )
                    }
                    item {
                        Chip(
                            label = { Text("＋ Настроить новый ПК") },
                            onClick = { showNewPcConfirm = true },
                            colors = ChipDefaults.secondaryChipColors(),
                            modifier = Modifier.fillMaxWidth(0.82f)
                        )
                    }
                }
            }

            if (showNewPcConfirm) {
                Box(
                    modifier = Modifier.fillMaxSize().background(Color.Black.copy(alpha = 0.92f)),
                    contentAlignment = Alignment.Center
                ) {
                    Column(horizontalAlignment = Alignment.CenterHorizontally, modifier = Modifier.fillMaxWidth().padding(14.dp)) {
                        Text("Настроить новый ПК?", style = MaterialTheme.typography.title3, textAlign = TextAlign.Center)
                        Spacer(Modifier.height(8.dp))
                        Text("Будет создан новый код настройки.\nТекущий ПК будет заменён новым.", style = MaterialTheme.typography.body2, textAlign = TextAlign.Center)
                        Spacer(Modifier.height(14.dp))
                        Button(
                            onClick = {
                                bleService?.forgetDevice()
                                newSetupMode = true
                                showNewPcConfirm = false
                                Toast.makeText(this@MainActivity, "Новый код создан", Toast.LENGTH_SHORT).show()
                            },
                            modifier = Modifier.fillMaxWidth(0.78f)
                        ) { Text("Начать") }
                        Spacer(Modifier.height(7.dp))
                        Button(
                            onClick = { showNewPcConfirm = false },
                            colors = ButtonDefaults.secondaryButtonColors(),
                            modifier = Modifier.fillMaxWidth(0.78f)
                        ) { Text("Отмена") }
                    }
                }
            }
        }
    }
}
