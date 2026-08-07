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
import android.os.Build
import android.os.IBinder
import android.os.ParcelUuid
import android.util.Log
import androidx.core.app.NotificationCompat
import com.wristkey.R
import com.wristkey.app.MainActivity
import com.wristkey.security.SecurityManager
import com.wristkey.sensors.MotionDetector
import java.util.UUID

class WristKeyBleService : Service() {

    companion object {
        const val TAG = "WristKeyBLE"
        const val CHANNEL_ID = "wristkey_ble_channel"
        const val NOTIFICATION_ID = 1

        val SERVICE_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        val CHALLENGE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
        val RESPONSE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
        val STATUS_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567893")
        val CCCD_UUID: UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")

        const val ACTION_USER_PRESENT = "com.wristkey.ACTION_USER_PRESENT"
        const val EXTRA_DEVICE_NAME = "device_name"
    }

    private val binder = LocalBinder()
    private var bluetoothManager: BluetoothManager? = null
    private var gattServer: BluetoothGattServer? = null
    private var advertiserCallback: AdvertiseCallback? = null
    private var connectedDevice: BluetoothDevice? = null

    private lateinit var challengeChar: BluetoothGattCharacteristic
    private lateinit var responseChar: BluetoothGattCharacteristic
    private lateinit var statusChar: BluetoothGattCharacteristic

    private val securityManager = SecurityManager()
    private val motionDetector by lazy { MotionDetector(this) }
    private var isUserPresent = false

    inner class LocalBinder : Binder() {
        fun getService(): WristKeyBleService = this@WristKeyBleService
    }

    private val userPresentReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            if (intent?.action == ACTION_USER_PRESENT) {
                Log.i(TAG, "User present confirmed")
                isUserPresent = true
                android.os.Handler(android.os.Looper.getMainLooper()).postDelayed({
                    isUserPresent = false
                }, 5000)
            }
        }
    }

    override fun onCreate() {
        super.onCreate()
        Log.i(TAG, "Service onCreate")

        bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as? BluetoothManager
        if (bluetoothManager == null) {
            Log.e(TAG, "BluetoothManager not available")
            stopSelf()
            return
        }

        createNotificationChannel()

        // FIX: REceiver_NOT_EXPORTED only on API 33+
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(userPresentReceiver, IntentFilter(ACTION_USER_PRESENT), Context.RECEIVER_NOT_EXPORTED)
        } else {
            registerReceiver(userPresentReceiver, IntentFilter(ACTION_USER_PRESENT))
        }

        try {
            securityManager.generateKeyPairIfNeeded()
        } catch (e: Exception) {
            Log.e(TAG, "Failed to generate keypair: ${e.message}")
        }

        motionDetector.start()
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
        try { gattServer?.close() } catch (_: Exception) {}
        try { unregisterReceiver(userPresentReceiver) } catch (_: Exception) {}
        motionDetector.stop()
        super.onDestroy()
    }

    private fun setupGattServer() {
        val adapter = bluetoothManager?.adapter
        if (adapter == null) {
            Log.e(TAG, "Bluetooth adapter null")
            return
        }

        val server = bluetoothManager?.openGattServer(this, gattServerCallback)
        if (server == null) {
            Log.e(TAG, "Failed to open GATT server")
            return
        }

        challengeChar = BluetoothGattCharacteristic(
            CHALLENGE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE,
            BluetoothGattCharacteristic.PERMISSION_WRITE_ENCRYPTED_MITM
        )

        responseChar = BluetoothGattCharacteristic(
            RESPONSE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ_ENCRYPTED_MITM
        )
        responseChar.addDescriptor(BluetoothGattDescriptor(
            CCCD_UUID,
            BluetoothGattDescriptor.PERMISSION_WRITE_ENCRYPTED_MITM or
                    BluetoothGattDescriptor.PERMISSION_READ_ENCRYPTED_MITM
        ))

        statusChar = BluetoothGattCharacteristic(
            STATUS_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ_ENCRYPTED_MITM
        )
        statusChar.addDescriptor(BluetoothGattDescriptor(
            CCCD_UUID,
            BluetoothGattDescriptor.PERMISSION_WRITE_ENCRYPTED_MITM or
                    BluetoothGattDescriptor.PERMISSION_READ_ENCRYPTED_MITM
        ))

        val service = android.bluetooth.BluetoothGattService(
            SERVICE_UUID,
            android.bluetooth.BluetoothGattService.SERVICE_TYPE_PRIMARY
        )
        service.addCharacteristic(challengeChar)
        service.addCharacteristic(responseChar)
        service.addCharacteristic(statusChar)

        server.addService(service)
        gattServer = server
        Log.i(TAG, "GATT server created")
    }

    private val gattServerCallback = object : BluetoothGattServerCallback() {

        override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
            when (newState) {
                BluetoothProfile.STATE_CONNECTED -> {
                    Log.i(TAG, "Connected: ${device?.address}")
                    connectedDevice = device
                    updateStatus(byteArrayOf(0x01))
                }
                BluetoothProfile.STATE_DISCONNECTED -> {
                    Log.i(TAG, "Disconnected: ${device?.address}")
                    if (connectedDevice?.address == device?.address) connectedDevice = null
                    updateStatus(byteArrayOf(0x00))
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
                Log.i(TAG, "Challenge: ${value?.size} bytes")

                if (!motionDetector.isMoving) {
                    Log.w(TAG, "Motion check FAILED")
                    if (responseNeeded) {
                        gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
                    }
                    sendResponse(byteArrayOf())
                    return
                }

                if (!isUserPresent) {
                    Log.w(TAG, "User NOT present — tap screen")
                    if (responseNeeded) {
                        gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
                    }
                    sendResponse(byteArrayOf())
                    return
                }

                val challenge = value ?: byteArrayOf()
                val sig = try {
                    securityManager.sign(challenge)
                } catch (e: Exception) {
                    Log.e(TAG, "Sign failed: ${e.message}")
                    if (responseNeeded) {
                        gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, value)
                    }
                    sendResponse(byteArrayOf())
                    return
                }

                val response = sig + byteArrayOf(1)
                sendResponse(response)
                Log.i(TAG, "Response sent: ${response.size} bytes")

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
            }
        }
    }

    private fun sendResponse(data: ByteArray) {
        responseChar.value = data
        connectedDevice?.let {
            try {
                gattServer?.notifyCharacteristicChanged(it, responseChar, false)
            } catch (e: Exception) {
                Log.e(TAG, "Notify failed: ${e.message}")
            }
        }
    }

    private fun updateStatus(data: ByteArray) {
        statusChar.value = data
        connectedDevice?.let {
            try {
                gattServer?.notifyCharacteristicChanged(it, statusChar, false)
            } catch (_: Exception) {}
        }
    }

    private fun startAdvertising() {
        val adapter = bluetoothManager?.adapter
        if (adapter == null) {
            Log.e(TAG, "BT adapter null")
            return
        }
        if (!adapter.isEnabled) {
            Log.e(TAG, "BT disabled")
            return
        }

        val advertiser = adapter.bluetoothLeAdvertiser
        if (advertiser == null) {
            Log.e(TAG, "LE Advertiser not supported")
            return
        }

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

        try {
            advertiser.startAdvertising(settings, data, advertiserCallback!!)
        } catch (e: SecurityException) {
            Log.e(TAG, "Advertising SecurityException: ${e.message}")
        }
    }

    private fun stopAdvertising() {
        val adapter = bluetoothManager?.adapter ?: return
        try {
            adapter.bluetoothLeAdvertiser?.stopAdvertising(advertiserCallback!!)
        } catch (_: Exception) {}
    }

    private fun createNotificationChannel() {
        val channel = NotificationChannel(CHANNEL_ID, "WristKey BLE", NotificationManager.IMPORTANCE_LOW)
        getSystemService(NotificationManager::class.java)?.createNotificationChannel(channel)
    }

    private fun buildNotification(text: String): Notification {
        val pendingIntent = PendingIntent.getActivity(
            this, 0, Intent(this, MainActivity::class.java), PendingIntent.FLAG_IMMUTABLE
        )
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("WristKey")
            .setContentText(text)
            .setSmallIcon(android.R.drawable.sym_def_app_icon)
            .setContentIntent(pendingIntent)
            .setOngoing(true)
            .build()
    }
}
