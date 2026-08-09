package com.wristkey.app

import android.app.Activity
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.IBinder
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import com.wristkey.R
import com.wristkey.ble.WristKeyBleService

class MainActivity : Activity() {

    private lateinit var macText: TextView
    private lateinit var statusText: TextView
    private lateinit var actionButton: Button
    private lateinit var resetButton: Button
    private var bleService: WristKeyBleService? = null
    private var bound = false

    private val REQUEST_BT_PERMISSIONS = 1001

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

        macText = findViewById(R.id.mac_text)
        statusText = findViewById(R.id.status_text)
        actionButton = findViewById(R.id.action_button)
        resetButton = findViewById(R.id.reset_button)

        // Show Bluetooth MAC address
        showMacAddress()

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

        if (hasBluetoothPermissions()) {
            startBleService()
        } else {
            requestBluetoothPermissions()
        }
    }

    private fun showMacAddress() {
        try {
            val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
            val adapter = bluetoothManager.adapter
            val address = adapter?.address ?: "Unknown"
            macText.text = "MAC: $address"
        } catch (e: SecurityException) {
            macText.text = "MAC: permission denied"
        }
    }

    private fun hasBluetoothPermissions(): Boolean {
        return if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            checkSelfPermission(android.Manifest.permission.BLUETOOTH_CONNECT) == PackageManager.PERMISSION_GRANTED &&
            checkSelfPermission(android.Manifest.permission.BLUETOOTH_ADVERTISE) == PackageManager.PERMISSION_GRANTED &&
            checkSelfPermission(android.Manifest.permission.BLUETOOTH_SCAN) == PackageManager.PERMISSION_GRANTED
        } else {
            checkSelfPermission(android.Manifest.permission.BLUETOOTH) == PackageManager.PERMISSION_GRANTED &&
            checkSelfPermission(android.Manifest.permission.BLUETOOTH_ADMIN) == PackageManager.PERMISSION_GRANTED &&
            checkSelfPermission(android.Manifest.permission.ACCESS_FINE_LOCATION) == PackageManager.PERMISSION_GRANTED
        }
    }

    private fun requestBluetoothPermissions() {
        val permissions = if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
            arrayOf(
                android.Manifest.permission.BLUETOOTH_CONNECT,
                android.Manifest.permission.BLUETOOTH_ADVERTISE,
                android.Manifest.permission.BLUETOOTH_SCAN
            )
        } else {
            arrayOf(
                android.Manifest.permission.BLUETOOTH,
                android.Manifest.permission.BLUETOOTH_ADMIN,
                android.Manifest.permission.ACCESS_FINE_LOCATION
            )
        }
        requestPermissions(permissions, REQUEST_BT_PERMISSIONS)
    }

    override fun onRequestPermissionsResult(
        requestCode: Int,
        permissions: Array<out String>,
        grantResults: IntArray
    ) {
        super.onRequestPermissionsResult(requestCode, permissions, grantResults)
        if (requestCode == REQUEST_BT_PERMISSIONS) {
            if (grantResults.all { it == PackageManager.PERMISSION_GRANTED }) {
                startBleService()
                showMacAddress() // refresh MAC after permissions granted
            } else {
                statusText.text = "Bluetooth permissions denied"
                Toast.makeText(this, "Bluetooth permissions required", Toast.LENGTH_LONG).show()
            }
        }
    }

    private fun startBleService() {
        val serviceIntent = Intent(this, WristKeyBleService::class.java)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            startForegroundService(serviceIntent)
        } else {
            startService(serviceIntent)
        }
        bindService(serviceIntent, serviceConnection, Context.BIND_AUTO_CREATE)
    }

    override fun onDestroy() {
        super.onDestroy()
        if (bound) {
            unbindService(serviceConnection)
            bound = false
        }
    }
}
