package com.wristkey.app

import android.app.Activity
import android.app.AlertDialog
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

    private lateinit var pinText: TextView
    private lateinit var statusText: TextView
    private lateinit var actionButton: Button
    private lateinit var resetButton: Button
    private lateinit var helpButton: Button
    private var bleService: WristKeyBleService? = null
    private var bound = false

    private val REQUEST_BT_PERMISSIONS = 1001
    private val PREFS_NAME = "WristKeyPrefs"
    private val KEY_FIRST_RUN = "first_run"

    private val serviceConnection = object : ServiceConnection {
        override fun onServiceConnected(name: ComponentName?, service: IBinder?) {
            val binder = service as WristKeyBleService.LocalBinder
            bleService = binder.getService()
            bound = true
            updatePinDisplay()
        }

        override fun onServiceDisconnected(name: ComponentName?) {
            bleService = null
            bound = false
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
        helpButton = findViewById(R.id.help_button)

        updatePinDisplay()

        actionButton.setOnClickListener {
            bleService?.confirmUserPresent()
            statusText.text = getString(R.string.status_pairing)
            actionButton.text = getString(R.string.action_unlock)
        }

        resetButton.setOnClickListener {
            bleService?.resetPairing()
            statusText.text = getString(R.string.status_disconnected)
            actionButton.text = getString(R.string.action_pair)
            updatePinDisplay()
            Toast.makeText(this, R.string.reset_done, Toast.LENGTH_SHORT).show()
        }

        helpButton.setOnClickListener {
            showInstructionDialog()
        }

        val prefs = getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)
        if (prefs.getBoolean(KEY_FIRST_RUN, true)) {
            showInstructionDialog()
            prefs.edit().putBoolean(KEY_FIRST_RUN, false).apply()
        }

        if (hasBluetoothPermissions()) {
            startBleService()
        } else {
            requestBluetoothPermissions()
        }
    }

    private fun showInstructionDialog() {
        AlertDialog.Builder(this)
            .setTitle("📖 Как подключить часы к ПК")
            .setMessage(
                "1️⃣ На ПК запустите wristkey-pair.exe\n" +
                "2️⃣ Нажмите 🔍 Scan на ПК\n" +
                "3️⃣ Выберите часы из списка (PIN: ${WristKeyBleService.pairingPin})\n" +
                "4️⃣ Нажмите 🔗 Pair на ПК\n" +
                "5️⃣ Часы вибрируют → потрясите рукой или нажмите кнопку ниже\n" +
                "6️⃣ ✅ Paired! Теперь запустите wristkeyd.exe\n\n" +
                "💡 Если не видит часы: нажмите Reset pairing и повторите"
            )
            .setPositiveButton("Понятно") { dialog, _ -> dialog.dismiss() }
            .setCancelable(false)
            .show()
    }

    private fun updatePinDisplay() {
        pinText.text = "PIN: ${WristKeyBleService.pairingPin}"
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
        if (bound) {
            unbindService(serviceConnection)
            bound = false
        }
    }
}
