package com.wristkey.app

import android.app.Activity
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.os.Bundle
import android.os.IBinder
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import com.wristkey.R
import com.wristkey.ble.WristKeyBleService

class MainActivity : Activity() {

    private lateinit var statusText: TextView
    private lateinit var actionButton: Button
    private lateinit var resetButton: Button
    private var bleService: WristKeyBleService? = null
    private var bound = false

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as WristKeyBleService.LocalBinder
            bleService = binder.getService()
            bound = true
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            bleService = null
            bound = false
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        statusText = findViewById(R.id.status_text)
        actionButton = findViewById(R.id.action_button)
        resetButton = findViewById(R.id.reset_button)

        // Start foreground service first (required for BLE on Android 12+)
        val serviceIntent = Intent(this, WristKeyBleService::class.java)
        startForegroundService(serviceIntent)
        bindService(serviceIntent, serviceConnection, Context.BIND_AUTO_CREATE)

        actionButton.setOnClickListener {
            bleService?.confirmUserPresent()
            statusText.text = getString(R.string.status_pairing)
            actionButton.text = getString(R.string.action_unlock)
        }

        resetButton.setOnClickListener {
            bleService?.resetPairing()
            statusText.text = getString(R.string.status_disconnected)
            actionButton.text = getString(R.string.action_pair)
            Toast.makeText(this, R.string.reset_done, Toast.LENGTH_SHORT).show()
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        if (bound) {
            unbindService(serviceConnection)
            bound = false
        }
    }
}
