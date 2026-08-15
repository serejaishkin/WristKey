package com.wristkey.ui

import android.content.Intent
import android.os.Bundle
import androidx.activity.ComponentActivity
import androidx.activity.compose.setContent
import androidx.compose.foundation.layout.*
import androidx.compose.material3.MaterialTheme
import androidx.compose.ui.Alignment
import androidx.compose.ui.Modifier
import androidx.compose.ui.unit.dp
import androidx.wear.compose.material3.Button
import androidx.wear.compose.material3.Text

class UnlockActivity : ComponentActivity() {
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        val userName = intent.getStringExtra("user") ?: "Unknown PC"

        setContent {
            MaterialTheme {
                Column(
                    modifier = Modifier.fillMaxSize().padding(16.dp),
                    horizontalAlignment = Alignment.CenterHorizontally,
                    verticalArrangement = Arrangement.Center
                ) {
                    Text("Unlock", style = MaterialTheme.typography.titleLarge)
                    Spacer(modifier = Modifier.height(8.dp))
                    Text(userName, style = MaterialTheme.typography.bodyMedium)
                    Spacer(modifier = Modifier.height(16.dp))
                    Button(onClick = {
                        sendBroadcast(Intent("com.wristkey.UNLOCK_ACTION").apply {
                            putExtra("approved", true)
                        })
                        finish()
                    }) { Text("Unlock") }
                    Spacer(modifier = Modifier.height(8.dp))
                    Button(onClick = {
                        sendBroadcast(Intent("com.wristkey.UNLOCK_ACTION").apply {
                            putExtra("approved", false)
                        })
                        finish()
                    }) { Text("Cancel") }
                }
            }
        }
    }
}
