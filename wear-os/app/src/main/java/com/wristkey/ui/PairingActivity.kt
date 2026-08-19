package com.wristkey.ui

import android.content.ComponentName
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.IBinder
import android.util.Log
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.Arrangement
import androidx.compose.foundation.layout.Column
import androidx.compose.foundation.layout.Spacer
import androidx.compose.foundation.layout.fillMaxSize
import androidx.compose.foundation.layout.fillMaxWidth
import androidx.compose.foundation.layout.height
import androidx.compose.foundation.layout.padding
import androidx.compose.foundation.rememberScrollState
import androidx.compose.foundation.verticalScroll
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
    companion object { private const val TAG = "WristKeyBLE" }

    private var service: WristKeyBleService? = null
    private var bound = false

    private val connection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, binder: IBinder?) {
            service = (binder as? WristKeyBleService.LocalBinder)?.getService()
            Log.i(TAG, "PairingActivity: service connected")
        }
        override fun onServiceDisconnected(name: ComponentName?) {
            Log.i(TAG, "PairingActivity: service disconnected")
            service = null
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val pcName = intent.getStringExtra("pcName") ?: "Windows PC"
        val pcAddress = intent.getStringExtra("pcAddress") ?: ""
        Log.i(TAG, "PairingActivity opened: pcName=$pcName pcAddress=$pcAddress")

        val serviceIntent = Intent(this, WristKeyBleService::class.java)
        startService(serviceIntent)
        bound = bindService(serviceIntent, connection, BIND_AUTO_CREATE)

        setContent {
            var ready by mutableStateOf(false)
            var error by mutableStateOf<String?>(null)

            LaunchedEffect(Unit) {
                repeat(100) {
                    val pending = service?.hasPendingPairing() == true
                    val challengeSize = service?.getCurrentChallengeSize() ?: 0
                    if (pending != ready) Log.i(TAG, "PairingActivity state: pending=$pending challengeSize=$challengeSize")
                    ready = pending
                    if (ready) return@LaunchedEffect
                    delay(100)
                }
                ready = service?.hasPendingPairing() == true
                Log.i(TAG, "PairingActivity wait finished: ready=$ready challengeSize=${service?.getCurrentChallengeSize() ?: 0}")
            }

            MaterialTheme {
                Column(
                    modifier = Modifier.fillMaxSize().verticalScroll(rememberScrollState()).padding(horizontal = 12.dp, vertical = 6.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center
                ) {
                    Text("Pair WristKey", style = MaterialTheme.typography.title2, textAlign = TextAlign.Center)
                    Spacer(Modifier.height(4.dp))
                    Text(pcName, textAlign = TextAlign.Center, maxLines = 1)
                    Spacer(Modifier.height(4.dp))
                    Text(if (ready) "Allow this PC?" else "Waiting for PC...", textAlign = TextAlign.Center)
                    error?.let {
                        Spacer(Modifier.height(3.dp))
                        Text(it, textAlign = TextAlign.Center)
                    }
                    Spacer(Modifier.height(7.dp))
                    Button(
                        onClick = {
                            Log.i(TAG, "ALLOW pressed: challengeSize=${service?.getCurrentChallengeSize() ?: 0}")
                            val ok = service?.confirmPairing() == true
                            Log.i(TAG, "confirmPairing result=$ok")
                            if (ok) finish() else error = "No challenge"
                        },
                        enabled = ready,
                        modifier = Modifier.fillMaxWidth(0.9f).height(42.dp)
                    ) { Text("ALLOW") }
                    Spacer(Modifier.height(5.dp))
                    Button(
                        onClick = {
                            Log.i(TAG, "CANCEL pressed")
                            service?.rejectPairing()
                            finish()
                        },
                        colors = ButtonDefaults.secondaryButtonColors(),
                        modifier = Modifier.fillMaxWidth(0.9f).height(38.dp)
                    ) { Text("CANCEL") }
                }
            }
        }
    }

    override fun onDestroy() {
        Log.i(TAG, "PairingActivity destroyed")
        if (bound) unbindService(connection)
        bound = false
        super.onDestroy()
    }
}
