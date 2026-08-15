package com.wristkey.ble

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.Service
import android.bluetooth.BluetoothAdapter
import android.bluetooth.BluetoothDevice
import android.bluetooth.BluetoothGatt
import android.bluetooth.BluetoothGattCharacteristic
import android.bluetooth.BluetoothGattServer
import android.bluetooth.BluetoothGattServerCallback
import android.bluetooth.BluetoothGattService
import android.bluetooth.le.AdvertiseCallback
import android.bluetooth.le.AdvertiseData
import android.bluetooth.le.AdvertiseSettings
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.content.IntentFilter
import android.content.SharedPreferences
import android.os.Binder
import android.os.Build
import android.os.IBinder
import android.os.ParcelUuid
import android.os.PowerManager
import android.util.Log
import androidx.core.app.NotificationCompat
import com.wristkey.R
import com.wristkey.security.KeyStoreManager
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

class WristKeyBleService : Service() {

    companion object {
        private const val TAG = "WristKeyBleService"
        private const val NOTIFICATION_ID = 1
        private const val CHANNEL_ID = "wristkey_ble_channel"
        private const val PREFS_NAME = "WristKeyPrefs"
        private const val PREFS_PAIRED_ADDRESS = "paired_device_address"
        private const val PREFS_PAIRED_NAME = "paired_device_name"

        val SERVICE_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        val CHALLENGE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
        val RESPONSE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
        val CONFIG_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567894")
        val SAMSUNG_SERVICE_UUID: UUID = UUID.fromString("0000fd50-0000-1000-8000-00805f9b34fb")
    }

    private val binder = LocalBinder()
    private var bluetoothAdapter: BluetoothAdapter? = null
    private var gattServer: BluetoothGattServer? = null
    private var advertiseCallback: AdvertiseCallback? = null
    private var challengeCharacteristic: BluetoothGattCharacteristic? = null
    private var responseCharacteristic: BluetoothGattCharacteristic? = null
    private var configCharacteristic: BluetoothGattCharacteristic? = null
    private var currentChallenge: ByteArray? = null
    private var wakeLock: PowerManager.WakeLock? = null

    private val keyStoreManager = KeyStoreManager()
    private val _pairingRequested = AtomicBoolean(false)
    val pairingRequested: AtomicBoolean get() = _pairingRequested

    private val _userPresent = AtomicBoolean(false)
    val userPresent: AtomicBoolean get() = _userPresent

    private val _userPresentCountdown = AtomicInteger(0)
    val userPresentCountdown: AtomicInteger get() = _userPresentCountdown

    private var pairedDeviceAddress: String? = null
    private var pairedDeviceName: String? = null
    private var pairingDeviceAddress: String? = null
    private var lastRssi: Int = 0
    private var currentPin: Int = (1000..9999).random()

    private lateinit var prefs: SharedPreferences

    inner class LocalBinder : Binder() {
        fun getService(): WristKeyBleService = this@WristKeyBleService
    }

    override fun onCreate() {
        super.onCreate()
        prefs = getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

        pairedDeviceAddress = prefs.getString(PREFS_PAIRED_ADDRESS, null)
        pairedDeviceName = prefs.getString(PREFS_PAIRED_NAME, null)
        if (pairedDeviceAddress != null) {
            Log.i(TAG, "Restored paired device: $pairedDeviceName ($pairedDeviceAddress)")
        }

        val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as android.bluetooth.BluetoothManager
        bluetoothAdapter = bluetoothManager.adapter

        if (bluetoothAdapter == null) {
            Log.e(TAG, "Bluetooth not supported")
            stopSelf()
            return
        }

        // Acquire partial wake lock to keep CPU alive when screen is off
        val powerManager = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = powerManager.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "WristKey::BleWakeLock")
        wakeLock?.acquire(10 * 60 * 1000L) // 10 minutes timeout, renew in onStartCommand if needed
        Log.i(TAG, "WakeLock acquired")

        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification())

        if (pairedDeviceAddress != null) {
            Log.i(TAG, "Paired device restored -- starting GATT server immediately")
            startGattServer()
        }

        startAdvertising()
        registerBluetoothStateReceiver()
    }

    override fun onBind(intent: Intent): IBinder = binder

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        // Renew wake lock if needed
        wakeLock?.let {
            if (!it.isHeld) {
                it.acquire(10 * 60 * 1000L)
                Log.i(TAG, "WakeLock renewed")
            }
        }
        return START_STICKY
    }

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            val channel = NotificationChannel(
                CHANNEL_ID,
                "WristKey BLE",
                NotificationManager.IMPORTANCE_LOW
            )
            val manager = getSystemService(NotificationManager::class.java)
            manager.createNotificationChannel(channel)
        }
    }

    private fun buildNotification(): Notification {
        return NotificationCompat.Builder(this, CHANNEL_ID)
            .setContentTitle("WristKey")
            .setContentText("BLE advertising active -- PIN: ${getAdvertisePin()}")
            .setSmallIcon(R.drawable.ic_launcher)
            .setOngoing(true)
            .build()
    }

    fun updateNotification() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, buildNotification())
    }

    fun getAdvertisePin(): String = String.format("%04d", currentPin)

    fun getPairedDeviceAddress(): String? = pairedDeviceAddress

    fun getPairedDeviceName(): String? = pairedDeviceName

    fun setPairedDevice(address: String, name: String) {
        pairedDeviceAddress = address
        pairedDeviceName = name
        prefs.edit()
            .putString(PREFS_PAIRED_ADDRESS, address)
            .putString(PREFS_PAIRED_NAME, name)
            .apply()
        Log.i(TAG, "Paired device saved: $name ($address)")
    }

    fun setPairedDeviceAddress(address: String) {
        pairedDeviceAddress = address
        prefs.edit().putString(PREFS_PAIRED_ADDRESS, address).apply()
        Log.i(TAG, "Paired device address saved: $address")
    }

    fun clearPairedDevice() {
        pairedDeviceAddress = null
        pairedDeviceName = null
        prefs.edit()
            .remove(PREFS_PAIRED_ADDRESS)
            .remove(PREFS_PAIRED_NAME)
            .apply()
        Log.i(TAG, "Paired device cleared")
    }

    fun forgetDevice() {
        clearPairedDevice()
        stopGattServer()
        resetPin()
        Log.i(TAG, "Device forgotten, restarting advertising")
    }

    fun isPaired(): Boolean = pairedDeviceAddress != null

    fun resetPin() {
        currentPin = (1000..9999).random()
        updateNotification()
    }

    // --- Methods required by Compose UI ---

    fun rejectPairing() {
        _pairingRequested.set(false)
        currentChallenge = null
        Log.i(TAG, "Pairing rejected")
    }

    fun getConnectedDeviceAddress(): String = pairedDeviceAddress ?: "--"

    fun isAdvertising(): Boolean = advertiseCallback != null

    fun getPairedDeviceCount(): Int = if (isPaired()) 1 else 0

    fun getPairingDeviceAddress(): String = pairingDeviceAddress ?: "--"

    fun getLastRssi(): Int = lastRssi

    fun getDeviceName(): String = pairedDeviceName ?: pairedDeviceAddress ?: "Unknown"

    // --- Advertising ---

    fun startAdvertising() {
        val adapter = bluetoothAdapter ?: return
        if (advertiseCallback != null) return

        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setConnectable(true)
            .setTimeout(0)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
            .build()

        val data = AdvertiseData.Builder()
            .addServiceUuid(ParcelUuid(SERVICE_UUID))
            .addServiceUuid(ParcelUuid(SAMSUNG_SERVICE_UUID))
            .setIncludeDeviceName(true)
            .build()

        val callback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                Log.i(TAG, "Advertising started successfully")
            }
            override fun onStartFailure(errorCode: Int) {
                Log.e(TAG, "Advertising failed: $errorCode")
            }
        }

        adapter.bluetoothLeAdvertiser?.startAdvertising(settings, data, callback)
        advertiseCallback = callback
    }

    fun stopAdvertising() {
        advertiseCallback?.let { bluetoothAdapter?.bluetoothLeAdvertiser?.stopAdvertising(it) }
        advertiseCallback = null
    }

    // --- GATT Server ---

    private fun startGattServer() {
        val manager = getSystemService(Context.BLUETOOTH_SERVICE) as android.bluetooth.BluetoothManager
        gattServer = manager.openGattServer(this, gattServerCallback)

        val service = BluetoothGattService(SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)

        challengeCharacteristic = BluetoothGattCharacteristic(
            CHALLENGE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
            BluetoothGattCharacteristic.PERMISSION_WRITE
        )

        responseCharacteristic = BluetoothGattCharacteristic(
            RESPONSE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ
        )

        configCharacteristic = BluetoothGattCharacteristic(
            CONFIG_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_WRITE,
            BluetoothGattCharacteristic.PERMISSION_READ or BluetoothGattCharacteristic.PERMISSION_WRITE
        )

        service.addCharacteristic(challengeCharacteristic)
        service.addCharacteristic(responseCharacteristic)
        service.addCharacteristic(configCharacteristic)

        gattServer?.addService(service)
        Log.i(TAG, "GATT server started")
    }

    private fun stopGattServer() {
        gattServer?.close()
        gattServer = null
        challengeCharacteristic = null
        responseCharacteristic = null
        configCharacteristic = null
        Log.i(TAG, "GATT server stopped")
    }

    private val gattServerCallback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
            when (newState) {
                BluetoothGatt.STATE_CONNECTED -> {
                    Log.i(TAG, "Device connected: ${device?.address}")
                    pairingDeviceAddress = device?.address
                    if (pairedDeviceAddress == null) {
                        Log.i(TAG, "New device -- setting pairing request")
                        _pairingRequested.set(true)
                    }
                }
                BluetoothGatt.STATE_DISCONNECTED -> {
                    Log.i(TAG, "Device disconnected: ${device?.address}")
                    _pairingRequested.set(false)
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
                Log.i(TAG, "Challenge received: ${value?.size ?: 0} bytes")
                currentChallenge = value
                _pairingRequested.set(true)
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
            } else if (characteristic?.uuid == CONFIG_CHAR_UUID) {
                Log.i(TAG, "Config write: ${value?.size ?: 0} bytes")
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
            } else {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
            }
        }

        override fun onCharacteristicReadRequest(
            device: BluetoothDevice?,
            requestId: Int,
            offset: Int,
            characteristic: BluetoothGattCharacteristic?
        ) {
            if (characteristic?.uuid == CONFIG_CHAR_UUID) {
                val configData = byteArrayOf(
                    0x01, // version
                    (currentPin shr 8).toByte(),
                    currentPin.toByte(),
                    if (isPaired()) 0x01 else 0x00
                )
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, configData)
            } else {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
            }
        }
    }

    // --- Pairing ---

    fun confirmPairing(): Boolean {
        return try {
            val challenge = currentChallenge ?: return false
            val response = keyStoreManager.signChallenge(challenge)
            responseCharacteristic?.value = response
            gattServer?.notifyCharacteristicChanged(null, responseCharacteristic, false)
            Log.i(TAG, "Pairing confirmed -- response sent (${response.size} bytes)")

            pairingDeviceAddress?.let { addr ->
                setPairedDevice(addr, "PC")
            }

            _pairingRequested.set(false)
            true
        } catch (e: Exception) {
            Log.e(TAG, "Pairing confirmation failed", e)
            false
        }
    }

    fun requestUserPresence(): Boolean {
        if (_userPresent.get()) return true
        _userPresentCountdown.set(10)
        return false
    }

    fun setUserPresent(present: Boolean) {
        _userPresent.set(present)
    }

    // --- Bluetooth State Receiver ---

    private fun registerBluetoothStateReceiver() {
        val filter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED)
        registerReceiver(bluetoothStateReceiver, filter)
    }

    private val bluetoothStateReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            when (intent?.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
                BluetoothAdapter.STATE_OFF -> {
                    Log.w(TAG, "Bluetooth turned off")
                    stopAdvertising()
                }
                BluetoothAdapter.STATE_ON -> {
                    Log.i(TAG, "Bluetooth turned on")
                    startAdvertising()
                }
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        stopAdvertising()
        stopGattServer()
        unregisterReceiver(bluetoothStateReceiver)
        wakeLock?.let {
            if (it.isHeld) {
                it.release()
                Log.i(TAG, "WakeLock released")
            }
        }
    }
}
