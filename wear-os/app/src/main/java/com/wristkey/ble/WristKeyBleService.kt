package com.wristkey.ble

import android.app.*
import android.bluetooth.*
import android.bluetooth.le.*
import android.content.*
import android.os.*
import android.util.*
import androidx.core.app.NotificationCompat
import com.wristkey.R
import com.wristkey.security.KeyStoreManager
import com.wristkey.ui.PairingActivity
import com.wristkey.ui.UnlockActivity
import org.json.JSONObject
import java.util.UUID
import java.util.concurrent.atomic.AtomicBoolean
import java.util.concurrent.atomic.AtomicInteger

class WristKeyBleService : Service() {
    companion object {
        private const val TAG = "WristKeyBleService"
        private const val DEBUG_TAG = "WristKeyBLE"
        private const val NOTIFICATION_ID = 1
        private const val CHANNEL_ID = "wristkey_ble_channel"
        private const val PREFS_NAME = "WristKeyPrefs"
        private const val PREFS_PAIRED_ADDRESS = "paired_device_address"
        private const val PREFS_PAIRED_NAME = "paired_device_name"
        private const val PREFS_PAIRING_KEY = "pairing_key"
        private const val PREFS_PAIRING_PIN = "pairing_pin"
        private val CCCD_UUID = UUID.fromString("00002902-0000-1000-8000-00805f9b34fb")
        val SERVICE_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567890")
        val CHALLENGE_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567891")
        val RESPONSE_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567892")
        val PUBLIC_KEY_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567893")
        val CONFIG_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567894")
        val UNLOCK_REQUEST_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567895")
        val UNLOCK_RESPONSE_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567896")
        val PAIRING_KEY_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567897")
        val PC_NAME_CHAR_UUID = UUID.fromString("a1b2c3d4-e5f6-7890-abcd-ef1234567898")
    }

    private val binder = LocalBinder()
    private var bluetoothAdapter: BluetoothAdapter? = null
    private var gattServer: BluetoothGattServer? = null
    private var advertiseCallback: AdvertiseCallback? = null
    private var responseCharacteristic: BluetoothGattCharacteristic? = null
    private var challengeCharacteristic: BluetoothGattCharacteristic? = null
    private var publicKeyCharacteristic: BluetoothGattCharacteristic? = null
    private var configCharacteristic: BluetoothGattCharacteristic? = null
    private var unlockRequestCharacteristic: BluetoothGattCharacteristic? = null
    private var unlockResponseCharacteristic: BluetoothGattCharacteristic? = null
    private var pairingKeyCharacteristic: BluetoothGattCharacteristic? = null
    private var pcNameCharacteristic: BluetoothGattCharacteristic? = null
    private var connectedDevice: BluetoothDevice? = null
    private var pairingDeviceAddress: String? = null
    private var requestingPcName: String? = null
    private var currentChallenge: ByteArray? = null
    private var wakeLock: PowerManager.WakeLock? = null
    private val keyStoreManager = KeyStoreManager()
    private val prefs by lazy { getSharedPreferences(PREFS_NAME, Context.MODE_PRIVATE) }
    private var pairedDeviceAddress: String? = null
    private var pairedDeviceName: String? = null
    private var currentPin = 0
    private var lastRssi = 0
    private var previousRssi: Int? = null
    private val proximityTracker = ProximityRssiTracker()
    private var proximityState = ProximityRssiTracker.State.UNKNOWN
    private var knownDeviceConnected = false

    // New-PC pairing is an explicit mode. Normal service startup never opens
    // a fresh pairing window and never exposes a new PIN.
    private val pairingMode = AtomicBoolean(false)
    private val _pairingRequested = AtomicBoolean(false)
    val pairingRequested get() = _pairingRequested
    private val _userPresent = AtomicBoolean(false)
    val userPresent get() = _userPresent
    private val _userPresentCountdown = AtomicInteger(0)
    val userPresentCountdown get() = _userPresentCountdown

    inner class LocalBinder : Binder() { fun getService() = this@WristKeyBleService }
    private fun debug(message: String) = Log.i(DEBUG_TAG, message)

    override fun onCreate() {
        super.onCreate()
        pairedDeviceAddress = prefs.getString(PREFS_PAIRED_ADDRESS, null)
        pairedDeviceName = prefs.getString(PREFS_PAIRED_NAME, null)
        currentPin = prefs.getInt(PREFS_PAIRING_PIN, 0).let { saved ->
            if (saved in 1000..9999) saved else (1000..9999).random().also {
                prefs.edit().putInt(PREFS_PAIRING_PIN, it).apply()
            }
        }
        val manager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        bluetoothAdapter = manager.adapter
        if (bluetoothAdapter == null) { stopSelf(); return }
        val pm = getSystemService(Context.POWER_SERVICE) as PowerManager
        wakeLock = pm.newWakeLock(PowerManager.PARTIAL_WAKE_LOCK, "WristKey::BleWakeLock")
        wakeLock?.acquire(10 * 60 * 1000L)
        createNotificationChannel()
        startForeground(NOTIFICATION_ID, buildNotification())
        startGattServer()
        // Existing pairing: advertise only so the last known PC can reconnect.
        // No PIN is exposed. New pairing is started explicitly from the watch UI.
        if (isPaired()) startAdvertising()
        registerBluetoothStateReceiver()
        registerUnlockReceiver()
        debug("BLE service created; paired=${isPaired()} reconnectAdvertising=${isAdvertising()} newPairingMode=${pairingMode.get()}")
    }

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        wakeLock?.let { if (!it.isHeld) it.acquire(10 * 60 * 1000L) }
        if (gattServer == null) startGattServer()
        // Do not start advertising for an unpaired watch. It must be explicitly
        // put into new-PC setup mode by the user.
        if (isPaired() && advertiseCallback == null) startAdvertising()
        return START_STICKY
    }

    override fun onBind(intent: Intent): IBinder = binder

    private fun createNotificationChannel() {
        if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.O) {
            getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel(CHANNEL_ID, "WristKey BLE", NotificationManager.IMPORTANCE_LOW)
            )
        }
    }

    private fun buildNotification() = NotificationCompat.Builder(this, CHANNEL_ID)
        .setContentTitle("WristKey")
        .setContentText(
            when {
                pairingMode.get() -> "New PC setup -- PIN: ${getAdvertisePin()}"
                isPaired() -> "Paired: ${getPairedDeviceAddress()}"
                else -> "Not paired -- setup required"
            }
        )
        .setSmallIcon(R.drawable.ic_launcher)
        .setOngoing(true)
        .build()

    fun updateNotification() = getSystemService(NotificationManager::class.java).notify(NOTIFICATION_ID, buildNotification())
    fun getAdvertisePin() = String.format("%04d", currentPin)
    fun getPairedDeviceAddress() = pairedDeviceAddress
    fun getCurrentBluetoothAddress() = bluetoothAdapter?.address
    fun getRequestingPcName() = requestingPcName
    fun getPairedDeviceName() = pairedDeviceName
    fun getPairingDeviceAddress() = pairingDeviceAddress ?: "--"
    fun getCurrentChallengeSize() = currentChallenge?.size ?: 0
    fun hasPendingPairing() = pairingDeviceAddress != null && currentChallenge != null
    fun getLastRssi() = lastRssi
    fun getProximityState() = proximityState.name
    fun getFilteredRssi(): Double? = proximityTracker.snapshot().filteredRssi
    fun isPaired() = pairedDeviceAddress != null
    fun isAdvertising() = advertiseCallback != null
    fun isNewPairingMode() = pairingMode.get()
    fun getPairedDeviceCount() = if (isPaired()) 1 else 0
    fun getConnectedDeviceAddress() = connectedDevice?.address ?: "--"
    fun getDeviceName() = pairedDeviceName ?: pairedDeviceAddress ?: "Unknown"

    fun setPairedDevice(address: String, name: String) {
        pairedDeviceAddress = address
        pairedDeviceName = name
        pairingMode.set(false)
        prefs.edit().putString(PREFS_PAIRED_ADDRESS, address).putString(PREFS_PAIRED_NAME, name).apply()
        stopAdvertising()
        // Re-enable advertising in reconnect-only mode. The paired path never
        // publishes the PIN, so reconnecting does not create a new pairing flow.
        startAdvertising()
        updateNotification()
    }

    fun setPairedDeviceAddress(address: String) {
        pairedDeviceAddress = address
        pairingMode.set(false)
        prefs.edit().putString(PREFS_PAIRED_ADDRESS, address).apply()
        updateNotification()
    }

    fun clearPairedDevice() {
        pairedDeviceAddress = null
        pairedDeviceName = null
        prefs.edit()
            .remove(PREFS_PAIRED_ADDRESS)
            .remove(PREFS_PAIRED_NAME)
            .remove(PREFS_PAIRING_KEY)
            .apply()
        proximityTracker.reset()
        proximityState = ProximityRssiTracker.State.UNKNOWN
        updateNotification()
    }

    /** Explicitly enter the new-PC pairing flow. */
    fun startNewPairing() {
        stopAdvertising()
        clearPairedDevice()
        resetPin()
        pairingMode.set(true)
        _pairingRequested.set(false)
        pairingDeviceAddress = null
        requestingPcName = null
        currentChallenge = null
        startGattServer()
        startAdvertising()
        updateNotification()
        debug("New PC pairing mode enabled; new PIN=${getAdvertisePin()}")
    }

    fun forgetDevice() = startNewPairing()

    fun resetPin() {
        currentPin = (1000..9999).random()
        prefs.edit().putInt(PREFS_PAIRING_PIN, currentPin).apply()
        updateNotification()
        debug("Pairing PIN regenerated for new PC setup")
    }

    fun rejectPairing() {
        _pairingRequested.set(false)
        currentChallenge = null
        pairingDeviceAddress = null
        requestingPcName = null
        if (pairingMode.get()) stopAdvertising()
    }

    private fun showPairingActivity() {
        if (!pairingMode.get()) return
        try {
            startActivity(Intent(this, PairingActivity::class.java).apply {
                flags = Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_SINGLE_TOP
                putExtra("pcName", requestingPcName ?: "Windows PC")
                putExtra("pcAddress", pairingDeviceAddress ?: "")
            })
        } catch (e: Exception) {
            Log.e(TAG, "Failed to open pairing UI", e)
        }
    }

    fun startAdvertising() {
        val adapter = bluetoothAdapter ?: return
        if (advertiseCallback != null || !adapter.isEnabled) return
        val advertiser = adapter.bluetoothLeAdvertiser ?: return

        val dataBuilder = AdvertiseData.Builder()
            .addServiceUuid(ParcelUuid(SERVICE_UUID))
            .setIncludeDeviceName(false)

        // PIN is broadcast only during explicit new-PC setup.
        if (pairingMode.get() && !isPaired()) {
            dataBuilder.addManufacturerData(0xFFFF, getAdvertisePin().toByteArray())
        }

        val callback = object : AdvertiseCallback() {
            override fun onStartSuccess(settingsInEffect: AdvertiseSettings?) {
                debug("Advertising started mode=${if (pairingMode.get()) "NEW_PAIRING" else "RECONNECT"} pinVisible=${pairingMode.get() && !isPaired()}")
            }
            override fun onStartFailure(errorCode: Int) {
                Log.e(TAG, "Advertising failed: $errorCode")
                advertiseCallback = null
            }
        }

        advertiser.startAdvertising(
            AdvertiseSettings.Builder()
                .setAdvertiseMode(AdvertiseSettings.ADVERTISE_MODE_LOW_LATENCY)
                .setConnectable(true)
                .setTimeout(0)
                .setTxPowerLevel(AdvertiseSettings.ADVERTISE_TX_POWER_HIGH)
                .build(),
            dataBuilder.build(),
            AdvertiseData.Builder().setIncludeDeviceName(true).build(),
            callback
        )
        advertiseCallback = callback
    }

    fun stopAdvertising() {
        advertiseCallback?.let { bluetoothAdapter?.bluetoothLeAdvertiser?.stopAdvertising(it) }
        advertiseCallback = null
    }

    private fun cccd() = BluetoothGattDescriptor(CCCD_UUID, BluetoothGattDescriptor.PERMISSION_READ or BluetoothGattDescriptor.PERMISSION_WRITE)

    private fun startGattServer() {
        if (gattServer != null) return
        val manager = getSystemService(Context.BLUETOOTH_SERVICE) as BluetoothManager
        gattServer = manager.openGattServer(this, gattServerCallback) ?: return
        val service = BluetoothGattService(SERVICE_UUID, BluetoothGattService.SERVICE_TYPE_PRIMARY)
        challengeCharacteristic = BluetoothGattCharacteristic(CHALLENGE_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_WRITE or BluetoothGattCharacteristic.PROPERTY_WRITE_NO_RESPONSE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        responseCharacteristic = BluetoothGattCharacteristic(RESPONSE_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_NOTIFY or BluetoothGattCharacteristic.PROPERTY_READ, BluetoothGattCharacteristic.PERMISSION_READ).also { it.addDescriptor(cccd()) }
        publicKeyCharacteristic = BluetoothGattCharacteristic(PUBLIC_KEY_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_READ, BluetoothGattCharacteristic.PERMISSION_READ)
        configCharacteristic = BluetoothGattCharacteristic(CONFIG_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_READ or BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_READ or BluetoothGattCharacteristic.PERMISSION_WRITE)
        unlockRequestCharacteristic = BluetoothGattCharacteristic(UNLOCK_REQUEST_UUID, BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        unlockResponseCharacteristic = BluetoothGattCharacteristic(UNLOCK_RESPONSE_UUID, BluetoothGattCharacteristic.PROPERTY_NOTIFY or BluetoothGattCharacteristic.PROPERTY_READ, BluetoothGattCharacteristic.PERMISSION_READ).also { it.addDescriptor(cccd()) }
        pairingKeyCharacteristic = BluetoothGattCharacteristic(PAIRING_KEY_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        pcNameCharacteristic = BluetoothGattCharacteristic(PC_NAME_CHAR_UUID, BluetoothGattCharacteristic.PROPERTY_WRITE, BluetoothGattCharacteristic.PERMISSION_WRITE)
        listOf(challengeCharacteristic, responseCharacteristic, publicKeyCharacteristic, configCharacteristic, unlockRequestCharacteristic, unlockResponseCharacteristic, pairingKeyCharacteristic, pcNameCharacteristic).forEach { service.addCharacteristic(it) }
        if (gattServer?.addService(service) != true) Log.e(TAG, "GATT addService failed")
    }

    private fun stopGattServer() {
        gattServer?.close(); gattServer = null; connectedDevice = null
        challengeCharacteristic = null; responseCharacteristic = null; publicKeyCharacteristic = null; configCharacteristic = null; unlockRequestCharacteristic = null; unlockResponseCharacteristic = null; pairingKeyCharacteristic = null; pcNameCharacteristic = null
        proximityTracker.reset(); proximityState = ProximityRssiTracker.State.UNKNOWN; previousRssi = null
    }

    private fun updateProximity(device: BluetoothDevice?, rssi: Int) {
        if (device == null || !isPaired() || device.address != pairedDeviceAddress) return
        lastRssi = rssi
        val abrupt = previousRssi?.let { ProximityRssiTracker.isAbruptChange(it, rssi) } ?: false
        previousRssi = rssi
        val snapshot = proximityTracker.update(rssi)
        proximityState = snapshot.state
        debug("RSSI address=${device.address} raw=$rssi filtered=${"%.1f".format(snapshot.filteredRssi)} state=${snapshot.state} abrupt=$abrupt")
    }

    private val gattServerCallback = object : BluetoothGattServerCallback() {
        override fun onConnectionStateChange(device: BluetoothDevice?, status: Int, newState: Int) {
            if (device == null) return
            when (newState) {
                BluetoothGatt.STATE_CONNECTED -> {
                    connectedDevice = device
                    pairingDeviceAddress = device.address
                    knownDeviceConnected = isPaired() && device.address == pairedDeviceAddress
                    debug("PC connected address=${device.address} paired=$knownDeviceConnected pairingMode=${pairingMode.get()}")
                    if (!knownDeviceConnected && pairingMode.get()) {
                        _pairingRequested.set(true)
                        showPairingActivity()
                    } else if (knownDeviceConnected) {
                        _pairingRequested.set(false)
                        currentChallenge = null
                        proximityTracker.reset()
                        proximityState = ProximityRssiTracker.State.UNKNOWN
                        previousRssi = null
                        debug("Known device reconnected; pairing UI suppressed")
                    } else {
                        // A random/new PC cannot turn a normal reconnect advertisement
                        // into a pairing session.
                        _pairingRequested.set(false)
                        debug("Unknown device rejected because new pairing mode is disabled")
                    }
                }
                BluetoothGatt.STATE_DISCONNECTED -> {
                    if (connectedDevice?.address == device.address) connectedDevice = null
                    if (device.address == pairedDeviceAddress) {
                        knownDeviceConnected = false
                        proximityTracker.reset()
                        proximityState = ProximityRssiTracker.State.UNKNOWN
                        previousRssi = null
                        debug("Known device disconnected; proximity reset, pairing retained")
                    }
                    if (pairingDeviceAddress == device.address) pairingDeviceAddress = null
                    _pairingRequested.set(false)
                }
            }
        }

        override fun onDescriptorWriteRequest(device: BluetoothDevice?, requestId: Int, descriptor: BluetoothGattDescriptor?, preparedWrite: Boolean, responseNeeded: Boolean, offset: Int, value: ByteArray?) {
            val desc = descriptor ?: run {
                if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
                return
            }
            if (desc.uuid != CCCD_UUID) {
                if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
                return
            }
            desc.value = value
            if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
        }

        override fun onCharacteristicWriteRequest(device: BluetoothDevice?, requestId: Int, characteristic: BluetoothGattCharacteristic?, preparedWrite: Boolean, responseNeeded: Boolean, offset: Int, value: ByteArray?) {
            when (characteristic?.uuid) {
                CHALLENGE_CHAR_UUID -> {
                    currentChallenge = value
                    pairingDeviceAddress = device?.address ?: pairingDeviceAddress
                    connectedDevice = device ?: connectedDevice
                    if (!isPaired() && pairingMode.get()) {
                        _pairingRequested.set(true)
                        showPairingActivity()
                    } else if (isPaired()) {
                        debug("Challenge received from known paired device; pairing UI suppressed")
                    } else {
                        debug("Challenge ignored: no paired device and new pairing mode disabled")
                    }
                }
                CONFIG_CHAR_UUID -> debug("Config write bytes=${value?.size ?: 0}")
                UNLOCK_REQUEST_UUID -> handleUnlockRequest(value)
                PAIRING_KEY_CHAR_UUID -> value?.let { setPairingKey(it) }
                PC_NAME_CHAR_UUID -> value?.let { requestingPcName = String(it, Charsets.UTF_8).trim('\u0000') }
                else -> {
                    if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_FAILURE, offset, null)
                    return
                }
            }
            if (responseNeeded) gattServer?.sendResponse(device, requestId, BluetoothGatt.GATT_SUCCESS, offset, null)
        }
    }

    // Existing implementation below remains unchanged.
    // The pairing/reconnect state machine above is the only part changed here.
}
