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
        val allGranted = permissions.entries.all { it.value }
        if (!allGranted) {
            Toast.makeText(this, "Bluetooth permissions required", Toast.LENGTH_LONG).show()
        } else {
            bindAndStartService()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        if (!hasRequiredPermissions()) requestPermissions() else bindAndStartService()
        setContent { MaterialTheme { MainScreen() } }
    }

    override fun onDestroy() {
        super.onDestroy()
        if (serviceBound) { unbindService(serviceConnection); serviceBound = false }
    }

    private fun hasRequiredPermissions(): Boolean {
        val permissions = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            arrayOf(Manifest.permission.BLUETOOTH_ADVERTISE,
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.BLUETOOTH_SCAN)
        } else {
            arrayOf(Manifest.permission.BLUETOOTH,
                Manifest.permission.BLUETOOTH_ADMIN,
                Manifest.permission.ACCESS_FINE_LOCATION)
        }
        return permissions.all {
            ContextCompat.checkSelfPermission(this, it) == PackageManager.PERMISSION_GRANTED
        }
    }

    private fun requestPermissions() {
        val permissions = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            arrayOf(Manifest.permission.BLUETOOTH_ADVERTISE,
                Manifest.permission.BLUETOOTH_CONNECT,
                Manifest.permission.BLUETOOTH_SCAN)
        } else {
            arrayOf(Manifest.permission.BLUETOOTH,
                Manifest.permission.BLUETOOTH_ADMIN,
                Manifest.permission.ACCESS_FINE_LOCATION)
        }
        permissionLauncher.launch(permissions)
    }

    private fun bindAndStartService() {
        Intent(this, WristKeyBleService::class.java).also { intent ->
            bindService(intent, serviceConnection, Context.BIND_AUTO_CREATE)
            if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
                startForegroundService(intent)
            } else {
                startService(intent)
            }
        }
    }

    @Composable
    fun MainScreen() {
        var pin by remember { mutableStateOf("----") }
        var statusText by remember { mutableStateOf("Starting...") }
        var showResetConfirm by remember { mutableStateOf(false) }

        LaunchedEffect(Unit) {
            while (true) {
                pin = bleService?.getAdvertisePin() ?: "----"
                statusText = when {
                    bleService == null -> "Service connecting..."
                    bleService?.pairingRequested?.get() == true -> "PC wants to pair!"
                    bleService?.isPaired() == true -> "Paired with ${bleService?.getPairedDeviceName() ?: "PC"}"
                    else -> "Ready to pair"
                }
                delay(1000)
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
                    horizontalAlignment = Alignment.CenterHorizontally
                ) {
                    item {
                        Text("⌚ WristKey", style = MaterialTheme.typography.title2,
                            modifier = Modifier.padding(top = 16.dp, bottom = 8.dp))
                    }
                    item {
                        Text(statusText, style = MaterialTheme.typography.caption2,
                            textAlign = TextAlign.Center, modifier = Modifier.padding(horizontal = 16.dp))
                    }
                    item {
                        Spacer(Modifier.height(12.dp))
                        Text("PIN", style = MaterialTheme.typography.caption3)
                        Text(pin, style = MaterialTheme.typography.display1, color = MaterialTheme.colors.primary)
                    }
                    item {
                        Button(onClick = {
                            val svc = bleService
                            if (svc != null && svc.pairingRequested.get()) {
                                val ok = svc.confirmPairing()
                                Toast.makeText(this@MainActivity, if (ok) "Paired!" else "Pairing failed", Toast.LENGTH_SHORT).show()
                            } else {
                                Toast.makeText(this@MainActivity, "No pairing request", Toast.LENGTH_SHORT).show()
                            }
                        }, modifier = Modifier.fillMaxWidth(0.8f)) { Text("Confirm Pairing") }
                    }
                    item {
                        Chip(label = { Text("⚙ Settings") },
                            onClick = { startActivity(Intent(this@MainActivity, SettingsActivity::class.java)) },
                            modifier = Modifier.fillMaxWidth(0.8f))
                    }
                    item {
                        Chip(label = { Text("🔄 Reset pairing") },
                            onClick = { showResetConfirm = true },
                            colors = ChipDefaults.secondaryChipColors(),
                            modifier = Modifier.fillMaxWidth(0.8f))
                    }
                }
            }
            if (showResetConfirm) {
                Box(
                    modifier = Modifier
                        .fillMaxSize()
                        .background(Color.Black.copy(alpha = 0.85f)),
                    contentAlignment = Alignment.Center
                ) {
                    Column(
                        horizontalAlignment = Alignment.CenterHorizontally,
                        modifier = Modifier.padding(16.dp)
                    ) {
                        Text("Reset pairing?", style = MaterialTheme.typography.title3)
                        Spacer(Modifier.height(8.dp))
                        Text("This will generate a new PIN. You'll need to re-pair with your PC.",
                            style = MaterialTheme.typography.body2,
                            textAlign = TextAlign.Center)
                        Spacer(Modifier.height(16.dp))
                        Button(onClick = {
                            bleService?.forgetDevice()
                            Toast.makeText(this@MainActivity, "Pairing reset", Toast.LENGTH_SHORT).show()
                            showResetConfirm = false
                        }) { Text("Reset") }
                        Spacer(Modifier.height(8.dp))
                        Button(
                            onClick = { showResetConfirm = false },
                            colors = ButtonDefaults.secondaryButtonColors()
                        ) { Text("Cancel") }
                    }
                }
            }
        }
    }
}
