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
import android.bluetooth.BluetoothManager
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

    private val keyStoreManager = KeyStoreManager()
    private val _pairingRequested = AtomicBoolean(false)
    val pairingRequested: AtomicBoolean get() = _pairingRequested

    private val _userPresent = AtomicBoolean(false)
    val userPresent: AtomicBoolean get() = _userPresent

    private val _userPresentCountdown = AtomicInteger(0)
    val userPresentCountdown: AtomicInteger get() = _userPresentCountdown

    private var pairedDeviceAddress: String? = null
    private var currentPin: Int = (1000..9999).random()

    private lateinit var prefs: SharedPreferences

    inner class LocalBinder : Binder() {
        fun getService(): WristKeyBleService = this@WristKeyBleService
    }

    override fun onCreate() {
        super.onCreate()
        prefs = getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE)

        // FIX: restore paired device address from persistent storage
        pairedDeviceAddress = prefs.getString(PREFS_PAIRED_ADDRESS, null)
        if (pairedDeviceAddress != null) {
            Log.i(TAG, "Restored paired device address: $pairedDeviceAddress")
        }

        val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        bluetoothAdapter = bluetoothManager.adapter

        if (bluetoothAdapter == null) {
            Log.e(TAG, "Bluetooth not supported")
            stopSelf()
            return
        }

        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification())

        // FIX: if we have a paired device, start GATT server immediately
        if (pairedDeviceAddress != null) {
            Log.i(TAG, "Paired device restored — starting GATT server immediately")
            startGattServer()
        }

        startAdvertising()
        registerBluetoothStateReceiver()
    }

    override fun onBind(intent: Intent): IBinder = binder

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
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
            .setContentText("BLE advertising active — PIN: ${getAdvertisePin()}")
            .setSmallIcon(R.drawable.ic_launcher_foreground)
            .setOngoing(true)
            .build()
    }

    private fun updateNotification() {
        val manager = getSystemService(NotificationManager::class.java)
        manager.notify(NOTIFICATION_ID, buildNotification())
    }

    private fun registerBluetoothStateReceiver() {
        val filter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED)
        registerReceiver(bluetoothStateReceiver, filter)
    }

    private val bluetoothStateReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            when (intent?.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
                BluetoothAdapter.STATE_ON -> {
                    Log.i(TAG, "Bluetooth ON — restarting advertising")
                    startAdvertising()
                    if (pairedDeviceAddress != null) {
                        startGattServer()
                    }
                }
                BluetoothAdapter.STATE_OFF -> {
                    Log.w(TAG, "Bluetooth OFF — stopping advertising")
                    stopAdvertising()
                }
            }
        }
    }

    private fun startAdvertising() {
        val adapter = bluetoothAdapter ?: return
        if (!adapter.isEnabled) {
            Log.w(TAG, "Bluetooth disabled — cannot advertise")
            return
        }

        val advertiser = adapter.bluetoothLeAdvertiser ?: run {
            Log.e(TAG, "BLE advertiser not available")
            return
        }

        stopAdvertising()

        val settings = AdvertiseSettings.Builder()
            .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
            .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
            .setConnectable(true)
            .build()

        val pinBytes = currentPin.toString().toByteArray(Charsets.UTF_8)
        val manufacturerData = byteArrayOf(0xFF.toByte(), 0xFF.toByte()) + pinBytes

        val data = AdvertiseData.Builder()
            .setIncludeDeviceName(false)
            .addServiceUuid(ParcelUuid(SAMSUNG_SERVICE_UUID))
            .addManufacturerData(0xFFFF, manufacturerData)
            .build()

        advertiseCallback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                Log.i(TAG, "Advertising started — PIN: ${getAdvertisePin()}")
            }
            override fun onStartFailure(errorCode: Int) {
                Log.e(TAG, "Advertising failed: $errorCode")
            }
        }

        advertiser.startAdvertising(settings, data, advertiseCallback!!)
    }

    private fun stopAdvertising() {
        advertiseCallback?.let {
            bluetoothAdapter?.bluetoothLeAdvertiser?.stopAdvertising(it)
            advertiseCallback = null
        }
    }

    private fun startGattServer() {
        if (gattServer != null) return

        val bluetoothManager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        gattServer = bluetoothManager.openGattServer(this, gattServerCallback)

        val service = BluetoothGattService(SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)

        challengeCharacteristic = BluetoothGattCharacteristic(
            CHALLENGE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE,
            BluetoothGattCharacteristic.PERMISSION_WRITE_ENCRYPTED or BluetoothGattCharacteristic.PERMISSION_WRITE
        )

        responseCharacteristic = BluetoothGattCharacteristic(
            RESPONSE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY,
            BluetoothGattCharacteristic.PERMISSION_READ_ENCRYPTED or BluetoothGattCharacteristic.PERMISSION_READ
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
        Log.i(TAG, "GATT server started with WristKey service")
    }

    private fun stopGattServer() {
        gattServer?.close()
        gattServer = null
        Log.i(TAG, "GATT server stopped")
    }

    private val gattServerCallback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
            when (newState) {
                BluetoothGatt.STATE_CONNECTED -> {
                    Log.i(TAG, "Device connected: ${device?.address}")
                    if (pairedDeviceAddress == null) {
                        Log.i(TAG, "New device — setting pairing request")
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
            when (characteristic?.uuid) {
                CHALLENGE_CHAR_UUID -> {
                    Log.i(TAG, "Challenge received (${value?.size} bytes)")
                    currentChallenge = value
                    _pairingRequested.set(true)
                    if (responseNeeded) {
                        gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
                    }
                }
                CONFIG_CHAR_UUID -> {
                    Log.i(TAG, "Config write: ${value?.toHex()}")
                    if (responseNeeded) {
                        gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
                    }
                }
                else -> {
                    if (responseNeeded) {
                        gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_WRITE_NOT_PERMITTED, offset, null)
                    }
                }
            }
        }

        override fun onCharacteristicReadRequest(
            device: BluetoothDevice?,
            requestId: Int,
            offset: Int,
            characteristic: BluetoothGattCharacteristic?
        ) {
            when (characteristic?.uuid) {
                CONFIG_CHAR_UUID -> {
                    val config = byteArrayOf(
                        if (isPaired()) 1 else 0,
                        userPresentCountdown.get().toByte()
                    )
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, config)
                }
                else -> {
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_READ_NOT_PERMITTED, offset, null)
                }
            }
        }
    }

    fun confirmPairing(): Boolean {
        return try {
            val challenge = currentChallenge ?: return false
            val response = keyStoreManager.signChallenge(challenge)
            responseCharacteristic?.value = response
            gattServer?.notifyCharacteristicChanged(null, responseCharacteristic, false)
            Log.i(TAG, "Pairing confirmed — response sent (${response.size} bytes)")
            _pairingRequested.set(false)
            true
        } catch (e: Exception) {
            Log.e(TAG, "Pairing confirmation failed", e)
            false
        }
    }

    fun requestUserPresence() {
        _userPresent.set(true)
        _userPresentCountdown.set(10)
        Log.i(TAG, "User presence requested — 10s countdown started")

        Thread {
            for (i in 10 downTo 0) {
                _userPresentCountdown.set(i)
                Thread.sleep(1000)
            }
            _userPresent.set(false)
            Log.i(TAG, "User presence expired")
        }.start()
    }

    fun isPaired(): Boolean = pairedDeviceAddress != null

    fun getPairedDeviceName(): String {
        return pairedDeviceAddress ?: "Unknown"
    }

    fun getAdvertisePin(): String = currentPin.toString().padStart(4, '0')

    fun forgetDevice() {
        pairedDeviceAddress = null
        currentChallenge = null
        _pairingRequested.set(false)
        _userPresent.set(false)
        _userPresentCountdown.set(0)
        prefs.edit().remove(PREFS_PAIRED_ADDRESS).apply()
        stopGattServer()
        startAdvertising()
        Log.i(TAG, "Device forgotten")
    }

    fun setPairedDeviceAddress(address: String) {
        pairedDeviceAddress = address
        prefs.edit().putString(PREFS_PAIRED_ADDRESS, address).apply()
        Log.i(TAG, "Paired device address saved: $address")
    }

    private fun ByteArray.toHex(): String = joinToString("") { "%02x".format(it) }

    override fun onDestroy() {
        super.onDestroy()
        stopAdvertising()
        stopGattServer()
        try {
            unregisterReceiver(bluetoothStateReceiver)
        } catch (_: IllegalArgumentException) {}
    }
}
