package com.wristkey.ble

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattDescriptor
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothManager
import android.bluetooth.BluetoothProfile
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.os.Binder
import android.os.IBinder
import android.os.ParcelUuid
import android.util.Log
import androidx.core.app.NotificationCompat
import com.wristkey.R
import com.wristkey.app.MainActivity
import java.util.UUID

/**
 * Foreground service that acts as BLE Peripheral (GATT Server).
 *
 * Protocol v1.0:
 * - SERVICE_UUID: a1b2c3d4-e5f6-7890-abcd-ef1234567890
 * - CHALLENGE (write): PC sends 16-byte nonce + 8-byte timestamp LE
 * - RESPONSE (notify): Watch sends [64-byte sig][1-byte user_present]
 * - STATUS (read/notify): connection state
 */
class WristKeyBleService : Service() {

    companion object {
        const val TAG = "WristKeyBLE"
        const val CHANNEL_ID = "wristkey_ble_channel"
        const val NOTIFICATION_ID = 1

        val SERVICE_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        val CHALLENGE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
        val RESPONSE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
        val STATUS_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567893")

        // CCCD descriptor UUID for notifications
        val CCCD_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")

        const val ACTION_USER_PRESENT = "com.wristkey.ACTION_USER_PRESENT"
        const val EXTRA_DEVICE_NAME = "device_name"
    }

    private val binder = LocalBinder()

    private var bluetoothManager: BluetoothManager? = null
    private var gattServer: BluetoothGattServer? = null
    private var advertiserCallback: AdvertiseCallback? = null
    private var connectedDevice: BluetoothDevice? = null

    // GATT characteristics
    private lateinit var challengeChar: BluetoothGattCharacteristic
    private lateinit var responseChar: BluetoothGattCharacteristic
    private lateinit var statusChar: BluetoothGattCharacteristic

    // State
    private var isUserPresent = false
    private var lastChallenge: ByteArray? = null

    inner class LocalBinder : Binder() {
        fun getService(): WristKeyBleService = this@WristKeyBleService
    }

    private val userPresentReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action == ACTION_USER_PRESENT) {
                Log.i(TAG, "User present confirmed via UI tap")
                isUserPresent = true
                // Auto-reset after 5 seconds
                android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
                    isUserPresent = false
                }, 5000)
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        Log.i(TAG, "Service onCreate")

        bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager

        createNotificationChannel()
        registerReceiver(userPresentReceiver, IntentFilter(ACTION_USER_PRESENT),
            Context.RECEIVER_NOT_EXPORTED)

        setupGattServer()
    }

    override fun onBind(intent: Intent?): IBinder = binder

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        startForeground(NOTIFICATION_ID, buildNotification("Waiting for PC…"))
        startAdvertising()
        return START_STICKY
    }

    override fun onDestroy() {
        stopAdvertising()
        gattServer?.close()
        unregisterReceiver(userPresentReceiver)
        super.onDestroy()
    }

    // =================================================================
    // GATT Server Setup
    // =================================================================

    private fun setupGattServer() {
        val server = bluetoothManager?.openGattServer(this, gattServerCallback)
            ?: run {
                Log.e(TAG, "Failed to open GATT server")
                return
            }

        // CHALLENGE characteristic: write-only, encrypted (requires bonded/secure)
        challengeChar = BluetoothGattCharacteristic(
            CHALLENGE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE,
            BluetoothGattCharacteristic.PERMISSION_WRITE_ENCRYPTED_MITM
        )

        // RESPONSE characteristic: notify-only
        responseChar = BluetoothGattCharacteristic(
            RESPONSE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ_ENCRYPTED_MITM
        )
        val cccd = BluetoothGattDescriptor(
            CCCD_UUID,
            BluetoothGattDescriptor.PERMISSION_WRITE_ENCRYPTED_MITM or
                    BluetoothGattDescriptor.PERMISSION_READ_ENCRYPTED_MITM
        )
        responseChar.addDescriptor(cccd)

        // STATUS characteristic: read + notify
        statusChar = BluetoothGattCharacteristic(
            STATUS_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ_ENCRYPTED_MITM
        )
        val statusCccd = BluetoothGattDescriptor(
            CCCD_UUID,
            BluetoothGattDescriptor.PERMISSION_WRITE_ENCRYPTED_MITM or
                    BluetoothGattDescriptor.PERMISSION_READ_ENCRYPTED_MITM
        )
        statusChar.addDescriptor(statusCccd)

        val service = android.bluetooth.BluetoothGattService(
            SERVICE_UUID,
            android.bluetooth.BluetoothGattService.SERVICE_TYPE_PRIMARY
        )
        service.addCharacteristic(challengeChar)
        service.addCharacteristic(responseChar)
        service.addCharacteristic(statusChar)

        server.addService(service)
        gattServer = server

        Log.i(TAG, "GATT server created with service $SERVICE_UUID")
    }

    // =================================================================
    // GATT Callbacks
    // =================================================================

    private val gattServerCallback = object : BluetoothGattServerCallback() {

        override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
            when (newState) {
                BluetoothProfile.STATE_CONNECTED -> {
                    Log.i(TAG, "Device connected: ${device?.address}")
                    connectedDevice = device
                    updateStatus(byteArrayOf(0x01)) // 0x01 = connected
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    Log.i(TAG, "Device disconnected: ${device?.address}")
                    if (connectedDevice?.address == device?.address) {
                        connectedDevice = null
                    }
                    updateStatus(byteArrayOf(0x00)) // 0x00 = disconnected
                }
            }
        }

        override fun onCharacteristicWriteRequest(
            device: BluetoothDevice?,
            requestId: Int,
            characteristic: BluetoothGattCharacteristic?,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray?
        ) {
            if (characteristic?.uuid == CHALLENGE_CHAR_UUID) {
                Log.i(TAG, "Challenge received (${value?.size} bytes) from ${device?.address}")

                lastChallenge = value?.copyOf()

                // TODO (Step 2): verify motion + sign challenge
                // For now: log and require user tap
                if (isUserPresent) {
                    Log.i(TAG, "User present, would sign challenge here")
                    // Placeholder: echo back for testing
                    sendResponse(byteArrayOf(0xDE.toByte(), 0xAD.toByte(), 0xBE.toByte(), 0xEF.toByte()))
                } else {
                    Log.w(TAG, "User NOT present, rejecting challenge. Tap screen to confirm.")
                }

                if (responseNeeded) {
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
                }
            }
        }

        override fun onDescriptorWriteRequest(
            device: BluetoothDevice?,
            requestId: Int,
            descriptor: BluetoothGattDescriptor?,
            preparedWrite: Boolean,
            responseNeeded: Boolean,
            offset: Int,
            value: ByteArray?
        ) {
            if (descriptor?.uuid == CCCD_UUID) {
                descriptor.value = value
                if (responseNeeded) {
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
                }
                Log.i(TAG, "CCCD updated for ${descriptor.characteristic?.uuid}")
            }
        }
    }

    // =================================================================
    // Helpers
    // =================================================================

    private fun sendResponse(data: ByteArray) {
        responseChar.value = data
        connectedDevice?.let { device ->
            gattServer?.notifyCharacteristicChanged(device, responseChar, false)
            Log.i(TAG, "Notification sent: ${data.size} bytes")
        } ?: Log.w(TAG, "No connected device to notify")
    }

    private fun updateStatus(data: ByteArray) {
        statusChar.value = data
        connectedDevice?.let { device ->
            gattServer?.notifyCharacteristicChanged(device, statusChar, false)
        }
    }

    // =================================================================
    // Advertising
    // =================================================================

    private fun startAdvertising() {
        val adapter = bluetoothManager?.adapter ?: return
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

        advertiserCallback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                Log.i(TAG, "Advertising started")
            }
            override fun onStartFailure(errorCode: Int) {
                Log.e(TAG, "Advertising failed: $errorCode")
            }
        }

        advertiser.startAdvertising(settings, data, advertiserCallback!!)
    }

    private fun stopAdvertising() {
        val adapter = bluetoothManager?.adapter ?: return
        val advertiser = adapter.bluetoothLeAdvertiser ?: return
        advertiserCallback?.let { advertiser.stopAdvertising(it) }
    }

    // =================================================================
    // Notification
    // =================================================================

    private fun createNotificationChannel() {
        val channel = NotificationChannel(
            CHANNEL_ID,
            "WristKey BLE",
            NotificationManager.IMPORTANCE_LOW
        )
        getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    private fun buildNotification(text: String): Notification {
        val pendingIntent = PendingIntent.getActivity(
            this,
            0,
            Intent(this, MainActivity::class.java),
            PendingIntent.FLAG_IMMUTABLE
        )

        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("WristKey")
            .setContentText(text)
            .setSmallIcon(android.R.drawable.ic_lock_idle_lock)
            .setContentIntent(pendingIntent)
            .setOngoing(true)
            .build()
    }
}
