package com.wristkey.app

import android.app.Activity
import android.content.Intent
import android.os.Bundle
import android.widget.Button
import android.widget.TextView
import com.wristkey.R
import com.wristkey.ble.WristKeyBleService

class MainActivity : Activity() {

    private lateinit var statusText: TextView
    private lateinit var actionButton: Button

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        statusText = findViewById(R.id.status_text)
        actionButton = findViewById(R.id.action_button)

        // Start BLE foreground service
        startService(Intent(this, WristKeyBleService::class.java))

        actionButton.setOnClickListener {
            // TODO: trigger user-present confirmation for unlock
            statusText.text = getString(R.string.status_pairing)
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        // Service continues running in foreground
    }
}
