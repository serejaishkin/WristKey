package com.wristkey.app

import android.app.Activity
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.content.ServiceConnection
import android.content.pm.PackageManager
import android.os.Build
import android.os.Bundle
import android.os.Handler
import android.os.IBinder
import android.os.Looper
import android.widget.Button
import android.widget.TextView
import android.widget.Toast
import com.wristkey.R
import com.wristkey.ble.WristKeyBleService

class MainActivity : Activity() {

    private lateinit var pinText: TextView
    private lateinit var statusText: TextView
    private lateinit var actionButton: Button
    private lateinit var resetButton: TextView
    private var bleService: WristKeyBleService? = null
    private var bound = false

    private val REQUEST_BT_PERMISSIONS = 1001
    private val uiHandler = Handler(Looper.getMainLooper())
    private var uiRunnable: Runnable? = null

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as WristKeyBleService.LocalBinder
            bleService = binder.getService()
            bound = true
            updateUi()
            startUiRefresh()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            bleService = null
            bound = false
            stopUiRefresh()
        }
    }

    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(savedInstanceState)
        window.addFlags(android.view.WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        setContentView(R.layout.activity_main)

        pinText = findViewById(R.id.pin_text)
        statusText = findViewById(R.id.status_text)
        actionButton = findViewById(R.id.action_button)
        resetButton = findViewById(R.id.reset_button)

        // FIX: update UI only after service is bound, not here
        pinText.text = "PIN: ----"
        statusText.text = "Starting..."

        actionButton.setOnClickListener {
            val svc = bleService ?: return@setOnClickListener

            if (svc.pairingRequested.get()) {
                // FIX: was confirmUserPresent() which doesn't exist
                val ok = svc.confirmPairing()
                if (ok) {
                    statusText.text = "Paired!"
                    Toast.makeText(this, "Paired successfully", Toast.LENGTH_SHORT).show()
                } else {
                    statusText.text = "Pairing failed"
                }
            } else if (svc.isPaired()) {
                // Already paired -> request user presence for unlock
                svc.requestUserPresence()
                statusText.text = "Unlock allowed (10s)"
                Toast.makeText(this, "Unlock allowed for 10 seconds", Toast.LENGTH_SHORT).show()
            } else {
                Toast.makeText(this, "Wait for PC to connect...", Toast.LENGTH_SHORT).show()
            }
            updateUi()
        }

        resetButton.setOnClickListener {
            bleService?.forgetDevice()
            statusText.text = "Forgotten"
            Toast.makeText(this, "Device forgotten", Toast.LENGTH_SHORT).show()
            updateUi()
        }

        if (hasBluetoothPermissions()) {
            startBleService()
        } else {
            requestBluetoothPermissions()
        }
    }

    private fun updateUi() {
        val svc = bleService ?: return

        // FIX: use getAdvertisePin() instead of non-existent static pairingPin
        pinText.text = "PIN: ${svc.getAdvertisePin()}"

        when {
            svc.pairingRequested.get() -> {
                statusText.text = "PC wants to pair! Tap button to confirm."
                actionButton.text = "Confirm Pairing"
                actionButton.isEnabled = true
            }
            svc.isPaired() -> {
                statusText.text = "Paired with ${svc.getPairedDeviceName()}"
                actionButton.text = "Allow Unlock"
                actionButton.isEnabled = true
            }
            else -> {
                statusText.text = "Advertising... Waiting for PC"
                actionButton.text = "Pair with PC"
                actionButton.isEnabled = false
            }
        }
    }

    private fun startUiRefresh() {
        uiRunnable = Runnable {
            updateUi()
            uiHandler.postDelayed(uiRunnable!!, 1000)
        }
        uiHandler.post(uiRunnable!!)
    }

    private fun stopUiRefresh() {
        uiRunnable?.let { uiHandler.removeCallbacks(it) }
        uiRunnable = null
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
        stopUiRefresh()
        if (bound) {
            unbindService(serviceConnection)
            bound = false
        }
    }
}
