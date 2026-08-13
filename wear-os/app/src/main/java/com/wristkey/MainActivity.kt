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
import android.widget.Toast
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.activity.result.contract.ActivityResultContracts
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.core.content.ContextCompat
import androidx.wear.compose.material.*
import com.wristkey.ble.WristKeyBleService
import kotlinx.coroutines.delay
import kotlinx.coroutines.launch

class MainActivity : ComponentActivity() {
    private var bleService: WristKeyBleService? = null
    private var serviceBound by mutableStateOf(false)

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as WristKeyBleService.LocalBinder
            bleService = binder.getService()
            serviceBound = true
            // Start advertising once service is connected
            startAdvertisingIfPermitted()
        }
        override fun onServiceDisconnected(name: ComponentName?) {
            bleService = null
            serviceBound = false
        }
    }

    private val blePermissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions ->
        val allGranted = permissions.entries.all { it.value }
        if (allGranted) {
            bleService?.startAdvertising()
        } else {
            Toast.makeText(this, "Bluetooth advertise permission required", Toast.LENGTH_LONG).show()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        bindService(
            Intent(this, WristKeyBleService::class.java),
            serviceConnection,
            Context.BIND_AUTO_CREATE
        )
        setContent {
            MaterialTheme {
                MainScreen(bleService)
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        if (serviceBound) unbindService(serviceConnection)
    }

    private fun startAdvertisingIfPermitted() {
        val permissions = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            arrayOf(Manifest.permission.BLUETOOTH_ADVERTISE, Manifest.permission.BLUETOOTH_CONNECT)
        } else {
            arrayOf(Manifest.permission.BLUETOOTH, Manifest.permission.BLUETOOTH_ADMIN)
        }

        val missing = permissions.filter {
            ContextCompat.checkSelfPermission(this, it) != PackageManager.PERMISSION_GRANTED
        }

        if (missing.isEmpty()) {
            bleService?.startAdvertising()
        } else {
            blePermissionLauncher.launch(missing.toTypedArray())
        }
    }
}

@Composable
fun MainScreen(bleService: WristKeyBleService?) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val scope = rememberCoroutineScope()
    val settings = remember { WristKeySettings(context) }

    var isLocked by remember { mutableStateOf(false) }
    var isPaired by remember { mutableStateOf(false) }
    var deviceName by remember { mutableStateOf("Not connected") }
    var showForgetDialog by remember { mutableStateOf(false) }
    var isAdvertising by remember { mutableStateOf(false) }
    var advertisePin by remember { mutableStateOf("----") }

    LaunchedEffect(Unit) {
        while (true) {
            delay(1000)
            isPaired = bleService?.isPaired() ?: false
            deviceName = bleService?.getDeviceName() ?: "Not connected"
            isAdvertising = bleService?.isAdvertising() ?: false
            advertisePin = bleService?.getAdvertisePin() ?: "----"
        }
    }

    val listState = rememberScalingLazyListState()

    Scaffold(
        timeText = { TimeText() },
        vignette = { Vignette(VignettePosition.TopAndBottom) },
        positionIndicator = { PositionIndicator(scalingLazyListState = listState) }
    ) {
        ScalingLazyColumn(
            modifier = Modifier.fillMaxSize(),
            state = listState,
            horizontalAlignment = Alignment.CenterHorizontally
        ) {
            item {
                Text(
                    "WristKey",
                    style = MaterialTheme.typography.title1,
                    modifier = Modifier.padding(top = 8.dp, bottom = 4.dp)
                )
            }

            item {
                Text(
                    if (isPaired) "🔗 $deviceName" else "⚪ Not paired",
                    style = MaterialTheme.typography.body2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    if (isAdvertising) "📡 Ad: PIN $advertisePin" else "❌ Not advertising",
                    style = MaterialTheme.typography.caption2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    if (isLocked) "🔒 Locked" else "🔓 Unlocked",
                    style = MaterialTheme.typography.body2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Spacer(modifier = Modifier.height(8.dp))
                Text(
                    "RSSI: ${bleService?.getLastRssi() ?: "--"} dBm",
                    style = MaterialTheme.typography.body2,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Spacer(modifier = Modifier.height(16.dp))
                Button(
                    onClick = {
                        if (isPaired) {
                            bleService?.sendUnlockChallenge()
                        } else {
                            Toast.makeText(context, "Not paired", Toast.LENGTH_SHORT).show()
                        }
                    },
                    modifier = Modifier.fillMaxWidth(0.7f)
                ) {
                    Text("🔓 Unlock PC")
                }
            }

            item {
                Spacer(modifier = Modifier.height(8.dp))
                Chip(
                    onClick = {
                        bleService?.startAdvertising()
                        Toast.makeText(context, "Advertising restarted", Toast.LENGTH_SHORT).show()
                    },
                    label = { Text("🔄 Restart Ad") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Spacer(modifier = Modifier.height(8.dp))
                Chip(
                    onClick = { showForgetDialog = true },
                    label = { Text("Forget PC") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Spacer(modifier = Modifier.height(8.dp))
                Chip(
                    onClick = {
                        context.startActivity(Intent(context, SettingsActivity::class.java))
                    },
                    label = { Text("⚙ Settings") },
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }
        }
    }

    if (showForgetDialog) {
        Dialog(onDismissRequest = { showForgetDialog = false }) {
            Column(
                modifier = Modifier.fillMaxWidth().padding(16.dp),
                horizontalAlignment = Alignment.CenterHorizontally
            ) {
                Text("Forget PC?", style = MaterialTheme.typography.title3)
                Spacer(modifier = Modifier.height(8.dp))
                Text("This PC will need to be paired again.", textAlign = TextAlign.Center)
                Spacer(modifier = Modifier.height(16.dp))
                Row {
                    Button(onClick = { showForgetDialog = false }) {
                        Text("Cancel")
                    }
                    Spacer(modifier = Modifier.width(8.dp))
                    Button(onClick = {
                        showForgetDialog = false
                        bleService?.forgetDevice()
                        Toast.makeText(context, "Device forgotten", Toast.LENGTH_SHORT).show()
                    }) {
                        Text("Forget")
                    }
                }
            }
        }
    }
}
