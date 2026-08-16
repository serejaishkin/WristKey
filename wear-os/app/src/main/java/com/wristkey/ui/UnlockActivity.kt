package com.wristkey.ui

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.runtime.Composable
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.text.style.TextAlign
import androidx.compose.ui.unit.dp
import androidx.wear.compose.material.*

class UnlockActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val user = intent.getStringExtra("user") ?: "Unknown PC"
        setContent {
            MaterialTheme {
                UnlockScreen(user = user, onApprove = {
                    sendBroadcast(Intent("com.wristkey.UNLOCK_ACTION").apply {
                        putExtra("approved", true)
                    })
                    finish()
                }, onCancel = {
                    sendBroadcast(Intent("com.wristkey.UNLOCK_ACTION").apply {
                        putExtra("approved", false)
                    })
                    finish()
                })
            }
        }
    }
}

@Composable
fun UnlockScreen(user: String, onApprove: () -> Unit, onCancel: () -> Unit) {
    Column(
        modifier = Modifier.fillMaxSize().padding(16.dp),
        horizontalAlignment = Alignment.CenterHorizontally,
        verticalArrangement = Arrangement.Center
    ) {
        Text("🔓 Unlock PC?", style = MaterialTheme.typography.title2, textAlign = TextAlign.Center)
        Spacer(Modifier.height(8.dp))
        Text(user, style = MaterialTheme.typography.body1, textAlign = TextAlign.Center)
        Spacer(Modifier.height(16.dp))
        Button(onClick = onApprove, modifier = Modifier.fillMaxWidth(0.8f)) {
            Text("✓ Unlock")
        }
        Spacer(Modifier.height(8.dp))
        Button(onClick = onCancel, colors = ButtonDefaults.secondaryButtonColors(), modifier = Modifier.fillMaxWidth(0.8f)) {
            Text("✗ Cancel")
        }
    }
}
