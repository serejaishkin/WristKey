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
import android.util.Base64
import android.util.Log
import androidx.core.app.NotificationCompat
import com.wristkey.R
import com.wristkey.security.KeyStoreManager
import com.wristkey.ui.UnlockActivity
import org.json.JSONObject
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
        private const val PREFS_PAIRING_KEY = "pairing_key"

        val SERVICE_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        val CHALLENGE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
        val RESPONSE_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
        val PUBLIC_KEY_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567893")
        val CONFIG_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567894")
        val UNLOCK_REQUEST_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567895")
        val UNLOCK_RESPONSE_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567896")
        val PAIRING_KEY_CHAR_UUID: UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567897")
    }

    private val binder = LocalBinder()
    private var bluetoothAdapter: BluetoothAdapter? = null
    private var gattServer: BluetoothGattServer? = null
    private var advertiseCallback: AdvertiseCallback? = null
    private var challengeCharacteristic: BluetoothGattCharacteristic? = null
    private var responseCharacteristic: BluetoothGattCharacteristic? = null
    private var publicKeyCharacteristic: BluetoothGattCharacteristic? = null
    private var configCharacteristic: BluetoothGattCharacteristic? = null
    private var unlockRequestCharacteristic: BluetoothGattCharacteristic? = null
    private var unlockResponseCharacteristic: BluetoothGattCharacteristic? = null
    private var pairingKeyCharacteristic: BluetoothGattCharacteristic? = null
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
        val powerManager = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = powerManager.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "WristKey::BleWakeLock")
        wakeLock?.acquire(10 * 60 * 1000L)
        Log.i(TAG, "WakeLock acquired")
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification())
        // GATT must exist before the first pairing. The desktop initiates pairing
        // by connecting to this service, so do not gate the server on paired state.
        startGattServer()
        startAdvertising()
        registerBluetoothStateReceiver()
        registerUnlockReceiver()
    }

    override fun onBind(intent: Intent): IBinder = binder

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
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
            val channel = NotificationChannel(CHANNEL_ID, "WristKey BLE", NotificationManager.IMPORTANCE_LOW)
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
        prefs.edit().putString(PREFS_PAIRED_ADDRESS, address).putString(PREFS_PAIRED_NAME, name).apply()
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
        prefs.edit().remove(PREFS_PAIRED_ADDRESS).remove(PREFS_PAIRED_NAME).remove(PREFS_PAIRING_KEY).apply()
        Log.i(TAG, "Paired device cleared")
    }

    fun forgetDevice() {
        clearPairedDevice()
        stopGattServer()
        resetPin()
        Log.i(TAG, "Device forgotten, restarting advertising")
    }

    fun isPaired(): Boolean = pairedDeviceAddress != null
    fun resetPin() { currentPin = (1000..9999).random(); updateNotification() }
    fun rejectPairing() { _pairingRequested.set(false); currentChallenge = null; Log.i(TAG, "Pairing rejected") }
    fun getConnectedDeviceAddress(): String = pairedDeviceAddress ?: "--"
    fun isAdvertising(): Boolean = advertiseCallback != null
    fun getPairedDeviceCount(): Int = if (isPaired()) 1 else 0
    fun getPairingDeviceAddress(): String = pairingDeviceAddress ?: "--"
    fun getLastRssi(): Int = lastRssi
    fun getDeviceName(): String = pairedDeviceName ?: pairedDeviceAddress ?: "Unknown"

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
            .setIncludeDeviceName(false)
            .build()

        val scanResponse = AdvertiseData.Builder()
            .setIncludeDeviceName(true)
            .addServiceData(ParcelUuid(SERVICE_UUID), getAdvertisePin().toByteArray())
            .build()

        val callback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                Log.i(TAG, "Advertising started (UUID: $SERVICE_UUID)")
            }
            override fun onStartFailure(errorCode: Int) {
                Log.e(TAG, "Advertising failed: $errorCode")
            }
        }

        adapter.bluetoothLeAdvertiser?.startAdvertising(settings, data, scanResponse, callback)
        advertiseCallback = callback
    }

    fun stopAdvertising() {
        advertiseCallback?.let { bluetoothAdapter?.bluetoothLeAdvertiser?.stopAdvertising(it) }
        advertiseCallback = null
    }

    private fun startGattServer() {
        val manager = getSystemService(Context.BLUETOOTH_SERVICE) as android.bluetooth.BluetoothManager
        gattServer?.close()
        gattServer = manager.openGattServer(this, gattServerCallback)
        val service = BluetoothGattService(SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        challengeCharacteristic = BluetoothGattCharacteristic(CHALLENGE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE,
            BluetoothGattCharacteristic.PERMISSION_WRITE)
        responseCharacteristic = BluetoothGattCharacteristic(RESPONSE_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY, BluetoothGattCharacteristic.PERMISSION_READ)
        publicKeyCharacteristic = BluetoothGattCharacteristic(PUBLIC_KEY_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_READ, BluetoothGattCharacteristic.PERMISSION_READ)
        configCharacteristic = BluetoothGattCharacteristic(CONFIG_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_WRITE,
            BluetoothGattCharacteristic.PERMISSION_READ or BluetoothGattCharacteristic.PERMISSION_WRITE)
        unlockRequestCharacteristic = BluetoothGattCharacteristic(UNLOCK_REQUEST_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        unlockResponseCharacteristic = BluetoothGattCharacteristic(UNLOCK_RESPONSE_UUID,
            BluetoothGattCharacteristic.PROPERTY_NOTIFY, BluetoothGattCharacteristic.PERMISSION_READ)
        pairingKeyCharacteristic = BluetoothGattCharacteristic(PAIRING_KEY_CHAR_UUID,
            BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        service.addCharacteristic(challengeCharacteristic)
        service.addCharacteristic(responseCharacteristic)
        service.addCharacteristic(publicKeyCharacteristic)
        service.addCharacteristic(configCharacteristic)
        service.addCharacteristic(unlockRequestCharacteristic)
        service.addCharacteristic(unlockResponseCharacteristic)
        service.addCharacteristic(pairingKeyCharacteristic)
        gattServer?.addService(service)
        Log.i(TAG, "GATT server started")
    }

    private fun stopGattServer() {
        gattServer?.close(); gattServer = null
        challengeCharacteristic = null; responseCharacteristic = null; publicKeyCharacteristic = null; configCharacteristic = null
        unlockRequestCharacteristic = null; unlockResponseCharacteristic = null; pairingKeyCharacteristic = null
        Log.i(TAG, "GATT server stopped")
    }

    private val gattServerCallback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
            when (newState) {
                BluetoothGatt.STATE_CONNECTED -> {
                    Log.i(TAG, "Device connected: ${device?.address}")
                    pairingDeviceAddress = device?.address
                    if (pairedDeviceAddress == null) { _pairingRequested.set(true) }
                }
                BluetoothGatt.STATE_DISCONNECTED -> {
                    Log.i(TAG, "Device disconnected: ${device?.address}")
                    _pairingRequested.set(false)
                }
            }
        }

        override fun onCharacteristicWriteRequest(device: BluetoothDevice?, requestId: Int,
            characteristic: BluetoothGattCharacteristic?, preparedWrite: Boolean,
            responseNeeded: Boolean, offset: Int, value: ByteArray?) {
            when (characteristic?.uuid) {
                CHALLENGE_CHAR_UUID -> {
                    Log.i(TAG, "Challenge received: ${value?.size ?: 0} bytes")
                    currentChallenge = value; _pairingRequested.set(true)
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
                }
                CONFIG_CHAR_UUID -> {
                    Log.i(TAG, "Config write: ${value?.size ?: 0} bytes")
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
                }
                UNLOCK_REQUEST_UUID -> {
                    Log.i(TAG, "Unlock request received: ${value?.size ?: 0} bytes")
                    handleUnlockRequest(value)
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
                }
                PAIRING_KEY_CHAR_UUID -> {
                    Log.i(TAG, "Pairing key received: ${value?.size ?: 0} bytes")
                    value?.let { setPairingKey(it) }
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
                }
                else -> gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
            }
        }

        override fun onCharacteristicReadRequest(device: BluetoothDevice?, requestId: Int, offset: Int,
            characteristic: BluetoothGattCharacteristic?) {
            if (characteristic?.uuid == CONFIG_CHAR_UUID) {
                val configData = byteArrayOf(0x01, (currentPin shr 8).toByte(), currentPin.toByte(),
                    if (isPaired()) 0x01 else 0x00)
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, configData)
            } else if (characteristic?.uuid == PUBLIC_KEY_CHAR_UUID) {
                val publicKey = keyStoreManager.getPublicKey()
                if (offset >= publicKey.size) {
                    gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_INVALID_OFFSET, offset, null)
                } else {
                    gattServer?.sendResponse(
                        device,
                        requestId,
                        BluetoothGatt.GATT_SUCCESS,
                        offset,
                        publicKey.copyOfRange(offset, publicKey.size)
                    )
                }
            } else {
                gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
            }
        }
    }

    fun confirmPairing(): Boolean {
        return try {
            val challenge = currentChallenge ?: return false
            val response = keyStoreManager.signChallenge(challenge)
            responseCharacteristic?.value = response
            gattServer?.notifyCharacteristicChanged(null, responseCharacteristic, false)
            Log.i(TAG, "Pairing confirmed -- response sent (${response.size} bytes)")
            pairingDeviceAddress?.let { addr -> setPairedDevice(addr, "PC") }
            val pairingKey = UnlockProtocol.generatePasswordKey()
            setPairingKey(pairingKey)
            Log.i(TAG, "Pairing key generated and saved")
            _pairingRequested.set(false)
            true
        } catch (e: Exception) { Log.e(TAG, "Pairing confirmation failed", e); false }
    }

    fun requestUserPresence(): Boolean {
        if (_userPresent.get()) return true
        _userPresentCountdown.set(10)
        return false
    }

    fun setUserPresent(present: Boolean) { _userPresent.set(present) }

    private fun handleUnlockRequest(data: ByteArray?) {
        if (data == null) return
        val pairingKey = getPairingKey() ?: run {
            Log.e(TAG, "No pairing key available")
            sendUnlockResponse(null, "NO_PAIRING_KEY")
            return
        }
        try {
            val decrypted = UnlockProtocol.decrypt(data, pairingKey)
            val request = JSONObject(String(decrypted))
            val user = request.optString("user", "Unknown PC")
            val intent = Intent(this, UnlockActivity::class.java).apply {
                putExtra("user", user)
                flags = Intent.FLAG_ACTIVITY_NEW_TASK
            }
            startActivity(intent)
        } catch (e: Exception) {
            Log.e(TAG, "Unlock request failed", e)
            sendUnlockResponse(null, "DECRYPT_ERROR")
        }
    }

    private fun sendUnlockResponse(passwordKey: ByteArray?, error: String? = null) {
        val pairingKey = getPairingKey() ?: return
        val response = JSONObject().apply {
            put("token", "wristkey_unlock")
            if (passwordKey != null) {
                put("password_key", Base64.encodeToString(passwordKey, Base64.NO_WRAP))
            } else {
                put("error", error ?: "UNKNOWN")
            }
        }
        try {
            val encrypted = UnlockProtocol.encrypt(response.toString().toByteArray(), pairingKey)
            unlockResponseCharacteristic?.value = encrypted
            gattServer?.notifyCharacteristicChanged(null, unlockResponseCharacteristic, false)
            Log.i(TAG, "Unlock response sent")
        } catch (e: Exception) { Log.e(TAG, "Failed to send unlock response", e) }
    }

    private fun getPairingKey(): ByteArray? {
        val b64 = prefs.getString(PREFS_PAIRING_KEY, null) ?: return null
        return Base64.decode(b64, Base64.DEFAULT)
    }

    private fun setPairingKey(key: ByteArray) {
        prefs.edit().putString(PREFS_PAIRING_KEY, Base64.encodeToString(key, Base64.DEFAULT)).apply()
    }

    private fun registerUnlockReceiver() {
        val filter = IntentFilter("com.wristkey.UNLOCK_ACTION")
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(unlockReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            registerReceiver(unlockReceiver, filter)
        }
    }

    private val unlockReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            val approved = intent?.getBooleanExtra("approved", false) ?: false
            if (approved) {
                val passwordKey = getPairingKey() ?: run {
                    Log.e(TAG, "No pairing key for unlock response")
                    sendUnlockResponse(null, "NO_PAIRING_KEY")
                    return
                }
                sendUnlockResponse(passwordKey)
            } else {
                sendUnlockResponse(null, "CANCEL")
            }
        }
    }

    private fun registerBluetoothStateReceiver() {
        val filter = IntentFilter(BluetoothAdapter.ACTION_STATE_CHANGED)
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU) {
            registerReceiver(bluetoothStateReceiver, filter, Context.RECEIVER_NOT_EXPORTED)
        } else {
            registerReceiver(bluetoothStateReceiver, filter)
        }
    }

    private val bluetoothStateReceiver = object : BroadcastReceiver() {
        override fun onReceive(context: Context?, intent: Intent?) {
            when (intent?.getIntExtra(BluetoothAdapter.EXTRA_STATE, BluetoothAdapter.ERROR)) {
                BluetoothAdapter.STATE_OFF -> { Log.w(TAG, "Bluetooth turned off"); stopAdvertising() }
                BluetoothAdapter.STATE_ON -> { Log.i(TAG, "Bluetooth turned on"); startAdvertising() }
            }
        }
    }

    override fun onDestroy() {
        super.onDestroy()
        stopAdvertising(); stopGattServer()
        try {
            unregisterReceiver(bluetoothStateReceiver)
        } catch (e: Exception) { Log.w(TAG, "unregisterReceiver failed", e) }
        try {
            unregisterReceiver(unlockReceiver)
        } catch (e: Exception) { Log.w(TAG, "unregisterReceiver failed", e) }
        wakeLock?.let { if (it.isHeld) { it.release(); Log.i(TAG, "WakeLock released") } }
    }
}