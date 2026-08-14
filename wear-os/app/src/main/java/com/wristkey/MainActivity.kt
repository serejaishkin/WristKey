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
import androidx.compose.foundation.background
import androidx.compose.runtime.*
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.compose.ui.window.Dialog
import androidx.core.content.ContextCompat
import androidx.wear.compose.material.*
import androidx.compose.ui.graphics.Color
import com.wristkey.ble.WristKeyBleService
import kotlinx.coroutines.delay

class MainActivity : ComponentActivity() {

    private val permissionLauncher = registerForActivityResult(
        ActivityResultContracts.RequestMultiplePermissions()
    ) { permissions: Map<String, Boolean> ->
        val allGranted = permissions.entries.all { it.value }
        if (allGranted) {
            Log.i("MainActivity", "All BLE permissions granted")
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
            permissionLauncher.launch(requiredPermissions.toTypedArray())
        }

        setContent {
            MaterialTheme {
                WristKeyApp()
            }
        }
    }
}

@Composable
fun WristKeyApp() {
    val context = androidx.compose.ui.platform.LocalContext.current
    var bleService by remember { mutableStateOf<WristKeyBleService?>(null) }
    var serviceBound by remember { mutableStateOf(false) }

    val serviceConnection = remember {
        object : ServiceConnection {
            override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
                val binder = service as WristKeyBleService.LocalBinder
                bleService = binder.getService()
                serviceBound = true
                Log.i("MainActivity", "Service CONNECTED")
            }
            override fun onServiceDisconnected(name: ComponentName?) {
                bleService = null
                serviceBound = false
                Log.i("MainActivity", "Service DISCONNECTED")
            }
        }
    }

    DisposableEffect(Unit) {
        val intent = Intent(context, WristKeyBleService::class.java)
        context.bindService(intent, serviceConnection, Context.BIND_AUTO_CREATE)
        onDispose {
            if (serviceBound) {
                context.unbindService(serviceConnection)
            }
        }
    }

    MainScreen(bleService)
}

@Composable
fun MainScreen(bleService: WristKeyBleService?) {
    val context = androidx.compose.ui.platform.LocalContext.current

    var isLocked by remember { mutableStateOf(false) }
    var isPaired by remember { mutableStateOf(false) }
    var deviceName by remember { mutableStateOf("Not connected") }
    var deviceAddress by remember { mutableStateOf("--") }
    var showForgetDialog by remember { mutableStateOf(false) }
    var isServerRunning by remember { mutableStateOf(false) }
    var currentPin by remember { mutableStateOf("----") }

    var hasPairingRequest by remember { mutableStateOf(false) }
    var pairingAddress by remember { mutableStateOf("--") }
    var showPairingDialog by remember { mutableStateOf(false) }
    var pairedCount by remember { mutableStateOf(0) }

    // Use rememberUpdatedState so the loop always sees the latest bleService
    val currentService by rememberUpdatedState(bleService)

    LaunchedEffect(Unit) {
        while (true) {
            delay(300)
            val svc = currentService ?: continue

            isPaired = svc.isPaired()
            deviceName = svc.getDeviceName()
            deviceAddress = svc.getConnectedDeviceAddress()
            isServerRunning = svc.isAdvertising()
            currentPin = svc.getAdvertisePin()
            pairedCount = svc.getPairedDeviceCount()

            val requested = svc.pairingRequested.get()
            if (requested && !hasPairingRequest) {
                hasPairingRequest = true
                pairingAddress = svc.getPairingDeviceAddress()
                showPairingDialog = true
                Log.i("MainActivity", "PAIRING REQUEST from addr=$pairingAddress")
            } else if (!requested && hasPairingRequest) {
                hasPairingRequest = false
                showPairingDialog = false
                Log.i("MainActivity", "Pairing cleared")
            }
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
                Text(
                    "MAC: $deviceAddress",
                    style = MaterialTheme.typography.caption3,
                    textAlign = TextAlign.Center,
                    modifier = Modifier.fillMaxWidth(0.9f)
                )
            }

            item {
                Spacer(modifier = Modifier.height(4.dp))
                Text(
                    if (isServerRunning) "📡 Server PIN $currentPin" else "❌ Server offline",
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

            // PAIR BUTTON
            if (hasPairingRequest) {
                item {
                    Spacer(modifier = Modifier.height(16.dp))
                    Button(
                        onClick = {
                            val ok = bleService?.confirmPairing() ?: false
                            hasPairingRequest = false
                            showPairingDialog = false
                            if (ok) {
                                Toast.makeText(context, "✅ Paired!", Toast.LENGTH_SHORT).show()
                            } else {
                                Toast.makeText(context, "❌ Pairing failed", Toast.LENGTH_SHORT).show()
                            }
                        },
                        modifier = Modifier.fillMaxWidth(0.9f)
                    ) {
                        Text("✅ Pair Request")
                    }
                }

                item {
                    Spacer(modifier = Modifier.height(8.dp))
                    Chip(
                        onClick = {
                            bleService?.rejectPairing()
                            hasPairingRequest = false
                            showPairingDialog = false
                            Toast.makeText(context, "❌ Pairing rejected", Toast.LENGTH_SHORT).show()
                        },
                        label = { Text("❌ Reject") },
                        modifier = Modifier.fillMaxWidth(0.9f)
                    )
                }
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

    if (showPairingDialog && hasPairingRequest) {
        Dialog(onDismissRequest = { }) {
            Box(
                modifier = Modifier.fillMaxWidth().padding(8.dp).background(
                    color = MaterialTheme.colors.surface,
                    shape = androidx.compose.foundation.shape.RoundedCornerShape(16.dp)
                )
            ) {
                Column(
                    modifier = Modifier.fillMaxWidth().padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    Text("🔗 Pair Request", style = MaterialTheme.typography.title3)
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(
                        "Device: $pairingAddress",
                        textAlign = TextAlign.Center,
                        style = MaterialTheme.typography.body2
                    )
                    Spacer(modifier = Modifier.height(16.dp))
                    Row {
                        Button(onClick = {
                            showPairingDialog = false
                            hasPairingRequest = false
                            bleService?.rejectPairing()
                            Toast.makeText(context, "❌ Pairing rejected", Toast.LENGTH_SHORT).show()
                        }) {
                            Text("❌ Cancel")
                        }
                        Spacer(modifier = Modifier.width(8.dp))
                        Button(onClick = {
                            showPairingDialog = false
                            hasPairingRequest = false
                            val ok = bleService?.confirmPairing() ?: false
                            if (ok) {
                                Toast.makeText(context, "✅ Paired!", Toast.LENGTH_SHORT).show()
                            } else {
                                Toast.makeText(context, "❌ Pairing failed", Toast.LENGTH_SHORT).show()
                            }
                        }) {
                            Text("✅ Pair")
                        }
                    }
                }
            }
        }
    }

    if (showForgetDialog) {
        Dialog(onDismissRequest = { showForgetDialog = false }) {
            Box(
                modifier = Modifier.fillMaxWidth().padding(8.dp).background(
                    color = MaterialTheme.colors.surface,
                    shape = androidx.compose.foundation.shape.RoundedCornerShape(16.dp)
                )
            ) {
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
}
