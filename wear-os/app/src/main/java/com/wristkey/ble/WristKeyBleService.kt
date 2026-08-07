package com.wristkey.ble

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothManager
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.content.Context
import android.content.Intent
import android.os.IBinder
import android.os.ParcelUuid
import android.util.Log
import androidx.core.app.NotificationCompat
import com.wristkey.R
import java.util.UUID

/**
 * Foreground service that acts as BLE Peripheral.
 *
 * Responsibilities:
 * 1. Advertise custom GATT service (challenge-response)
 * 2. Android Keystore: sign challenges with ECDSA P-256
 * 3. Accelerometer: verify watch is on wrist before signing
 * 4. User-present tap: anti-relay confirmation
 */
class WristKeyBleService : Service() {

    companion object {
        const val CHANNEL_ID = "wristkey_ble_channel"
        const val NOTIFICATION_ID = 1
        val SERVICE_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        val CHALLENGE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
        val RESPONSE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
    }

    private var bluetoothAdapter: BluetoothAdapter? = null
    private var advertiseCallback: AdvertiseCallback? = null

    override fun onCreate() {
        super.onCreate()
        createNotificationChannel()
        val manager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        bluetoothAdapter = manager.adapter
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(NOTIFICATION_ID, buildNotification())
        startAdvertising()
        return START_STICKY
    }

    override fun onBind(intent: Intent?): IBinder? = null

    override fun onDestroy() {
        stopAdvertising()
        super.onDestroy()
    }

    private fun startAdvertising() {
        val adapter = bluetoothAdapter ?: return
        val advertiser = adapter.bluetoothLeAdvertiser ?: return

        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_BALANCED)
            .setConnectable(true)
            .setTimeout(0)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_MEDIUM)
            .build()

        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(true)
            .addServiceUuid(ParcelUuid(SERVICE_UUID))
            .build()

        advertiseCallback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                Log.i("WristKeyBLE", "Advertising started")
            }

            override fun onStartFailure(errorCode: Int) {
                Log.e("WristKeyBLE", "Advertising failed: $errorCode")
            }
        }

        advertiser.startAdvertising(settings, data, advertiseCallback!!)
    }

    private fun stopAdvertising() {
        val adapter = bluetoothAdapter ?: return
        val advertiser = adapter.bluetoothLeAdvertiser ?: return
        advertiseCallback?.let { advertiser.stopAdvertising(it) }
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "WristKey BLE",
            NotificationManager.IMPORTANCE_LOW
        )
        val manager = getSystemService(NotificationManager::class.java)
        manager.createNotificationChannel(channel)
    }

    private fun buildNotification(): Notification {
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("WristKey")
            .setContentText("Waiting for PC connection…")
            .setSmallIcon(android.R.drawable.ic_lock_idle_lock)
            .setOngoing(true)
            .build()
    }
}
