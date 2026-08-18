package com.wristkey.ui

import android.content.ComponentName
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.IBinder
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.runtime.LaunchedEffect
import androidx.compose.runtime.getValue
import androidx.compose.runtime.mutableStateOf
import androidx.compose.runtime.setValue
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.wear.compose.material.Button
import androidx.wear.compose.material.ButtonDefaults
import androidx.wear.compose.material.MaterialTheme
import androidx.wear.compose.material.Text
import com.wristkey.ble.WristKeyBleService
import kotlinx.coroutines.delay

class PairingActivity : ComponentActivity() {
    private var service: WristKeyBleService? = null
    private var bound = false

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
            service = (binder as? WristKeyBleService.LocalBinder)?.getService()
        }
        override fun onServiceDisconnected(name: ComponentName?) {
            service = null
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pcName = intent.getStringExtra("pcName") ?: "Windows PC"

        val serviceIntent = Intent(this, WristKeyBleService::class.java)
        startService(serviceIntent)
        bound = bindService(serviceIntent, connection, BIND_AUTO_CREATE)

        setContent {
            var ready by mutableStateOf(false)
            var error by mutableStateOf<String?>(null)

            LaunchedEffect(Unit) {
                repeat(100) {
                    ready = service?.hasPendingPairing() == true
                    if (ready) return@LaunchedEffect
                    delay(100)
                }
                ready = service?.hasPendingPairing() == true
            }

            MaterialTheme {
                Column(
                    modifier = Modifier.fillMaxSize().padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center
                ) {
                    Text("Pair WristKey", style = MaterialTheme.typography.title2, textAlign = TextAlign.Center)
                    Spacer(Modifier.height(8.dp))
                    Text(pcName, textAlign = TextAlign.Center)
                    Spacer(Modifier.height(8.dp))
                    Text(
                        if (ready) "Allow this PC to use your watch?" else "Waiting for the PC...",
                        textAlign = TextAlign.Center
                    )
                    error?.let {
                        Spacer(Modifier.height(6.dp))
                        Text(it, textAlign = TextAlign.Center)
                    }
                    Spacer(Modifier.height(12.dp))
                    Button(
                        onClick = {
                            val ok = service?.confirmPairing() == true
                            if (ok) finish() else error = "No pairing challenge received"
                        },
                        enabled = ready,
                        modifier = Modifier.fillMaxWidth(0.85f)
                    ) { Text("Allow") }
                    Spacer(Modifier.height(8.dp))
                    Button(
                        onClick = { service?.rejectPairing(); finish() },
                        colors = ButtonDefaults.secondaryButtonColors(),
                        modifier = Modifier.fillMaxWidth(0.85f)
                    ) { Text("Cancel") }
                }
            }
        }
    }

    override fun onDestroy() {
        if (bound) unbindService(connection)
        bound = false
        super.onDestroy()
    }
}
