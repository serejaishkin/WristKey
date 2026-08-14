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
import android.util.Log
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

class MainActivity : ComponentActivity() {
    private var bleService: WristKeyBleService? = null
    private var serviceBound by mutableStateOf(false)

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as WristKeyBleService.LocalBinder
            bleService = binder.getService()
            serviceBound = true
            Log.i("MainActivity", "Service bound, GATT server running")
        }
        override fun onServiceDisconnected(name: ComponentName?) {
            bleService = null
            serviceBound = false
        }
    }

    // FIX: explicit type annotation for Kotlin type inference
    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions: Map<String, Boolean> ->
        val allGranted = permissions.entries.all { it.value }
        if (allGranted) {
            Log.i("MainActivity", "All BLE permissions granted")
            startBleService()
        } else {
            Log.e("MainActivity", "Some BLE permissions denied: $permissions")
            Toast.makeText(this, "BLE permissions required", Toast.LENGTH_LONG).show()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)

        val requiredPermissions = mutableListOf<String>()
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.BLUETOOTH_CONNECT) != PackageManager.PERMISSION_GRANTED) {
                requiredPermissions.add(Manifest.permission.BLUETOOTH_CONNECT)
            }
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.BLUETOOTH_ADVERTISE) != PackageManager.PERMISSION_GRANTED) {
                requiredPermissions.add(Manifest.permission.BLUETOOTH_ADVERTISE)
            }
            if (ContextCompat.checkSelfPermission(this, Manifest.permission.BLUETOOTH_SCAN) != PackageManager.PERMISSION_GRANTED) {
                requiredPermissions.add(Manifest.permission.BLUETOOTH_SCAN)
            }
        }
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.Q &&
            ContextCompat.checkSelfPermission(this, Manifest.permission.ACCESS_FINE_LOCATION) != PackageManager.PERMISSION_GRANTED) {
            requiredPermissions.add(Manifest.permission.ACCESS_FINE_LOCATION)
        }

        if (requiredPermissions.isNotEmpty()) {
            Log.i("MainActivity", "Requesting permissions: $requiredPermissions")
            permissionLauncher.launch(requiredPermissions.toTypedArray())
        } else {
            startBleService()
        }

        setContent {
            MaterialTheme {
                MainScreen(bleService)
            }
        }
    }

    private fun startBleService() {
        bindService(
            Intent(this, WristKeyBleService::class.java),
            serviceConnection,
            Context.BIND_AUTO_CREATE
        )
    }

    override fun onDestroy() {
        super.onDestroy()
        if (serviceBound) unbindService(serviceConnection)
    }
}

@Composable
fun MainScreen(bleService: WristKeyBleService?) {
    val context = androidx.compose.ui.platform.LocalContext.current
    val settings = remember { WristKeySettings(context) }

    var isLocked by remember { mutableStateOf(false) }
    var isPaired by remember { mutableStateOf(false) }
    var deviceName by remember { mutableStateOf("Not connected") }
    var showForgetDialog by remember { mutableStateOf(false) }
    var isServerRunning by remember { mutableStateOf(false) }
    var currentPin by remember { mutableStateOf("----") }

    LaunchedEffect(Unit) {
        while (true) {
            delay(1000)
            isPaired = bleService?.isPaired() ?: false
            deviceName = bleService?.getDeviceName() ?: "Not connected"
            isServerRunning = bleService?.isAdvertising() ?: false
            currentPin = bleService?.getAdvertisePin() ?: "----"
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
                    if (isServerRunning) "📡 GATT server: PIN $currentPin" else "❌ Server offline",
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
                        bleService?.requestUserPresence()
                    },
                    modifier = Modifier.fillMaxWidth(0.7f)
                ) {
                    Text("🔓 Unlock PC")
                }
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
