package com.wristkey.app

import android.Manifest
import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.provider.Settings
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import androidx.core.app.ActivityCompat
import androidx.core.content.ContextCompat
import com.wristkey.R
import com.wristkey.ble.WristKeyBleService

class MainActivity : Activity() {

    private lateinit var statusText: TextView
    private lateinit var actionButton: Button

    companion object {
        const val REQ_PERMISSIONS = 1001
    }

    private val requiredPermissions = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
        arrayOf(
            Manifest.permission.BLUETOOTH_CONNECT,
            Manifest.permission.BLUETOOTH_ADVERTISE,
            Manifest.permission.ACCESS_FINE_LOCATION,
            Manifest.permission.POST_NOTIFICATIONS
        )
    } else {
        arrayOf(
            Manifest.permission.BLUETOOTH,
            Manifest.permission.BLUETOOTH_ADMIN,
            Manifest.permission.ACCESS_FINE_LOCATION
        )
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        setContentView(R.layout.activity_main)

        statusText = findViewById(R.id.status_text)
        actionButton = findViewById(R.id.action_button)

        if (checkPermissions()) {
            onPermissionsGranted()
        } else {
            ActivityCompat.requestPermissions(this, requiredPermissions, REQ_PERMISSIONS)
        }
    }

    private fun checkPermissions(): Boolean {
        return requiredPermissions.all {
            ContextCompat.checkSelfPermission(this, it) == PackageManager.PERMISSION_GRANTED
        }
    }

    private fun onPermissionsGranted() {
        // Check Bluetooth enabled
        val btManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        val btAdapter = btManager.adapter
        if (btAdapter == null) {
            statusText.text = "No Bluetooth"
            actionButton.isEnabled = false
            return
        }
        if (!btAdapter.isEnabled) {
            statusText.text = "Enable Bluetooth"
            actionButton.text = "Open Settings"
            actionButton.setOnClickListener {
                startActivity(Intent(Settings.ACTION_BLUETOOTH_SETTINGS))
            }
            return
        }

        // Start service
        startForegroundService(Intent(this, WristKeyBleService::class.java))
        statusText.text = getString(R.string.status_disconnected)
        actionButton.text = getString(R.string.action_pair)

        actionButton.setOnClickListener {
            sendBroadcast(Intent(WristKeyBleService.ACTION_USER_PRESENT).apply {
                putExtra(WristKeyBleService.EXTRA_DEVICE_NAME, "PC")
            })
            statusText.text = getString(R.string.status_pairing)
            actionButton.text = getString(R.string.action_unlock)
        }
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQ_PERMISSIONS) {
            if (grantResults.all { it == PackageManager.PERMISSION_GRANTED }) {
                onPermissionsGranted()
            } else {
                Toast.makeText(this, "Permissions required for BLE", Toast.LENGTH_LONG).show()
                statusText.text = "Permissions denied"
                actionButton.isEnabled = false
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
    }
}
