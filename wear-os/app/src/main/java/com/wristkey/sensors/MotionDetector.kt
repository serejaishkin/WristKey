package com.wristkey.sensors

import android.content.Context
import android.hardware.Sensor
import android.hardware.SensorEvent
import android.hardware.SensorEventListener
import android.hardware.SensorManager
import android.util.Log

/**
 * Detects if watch is on wrist and moving.
 *
 * Uses accelerometer to verify presence before signing unlock.
 * Anti-relay: if watch is stationary on table, reject.
 */
class MotionDetector(context: Context) : SensorEventListener {

    private val sensorManager = context.getSystemService(Context.SENSOR_SERVICE) as SensorManager
    private val accelerometer = sensorManager.getDefaultSensor(Sensor.TYPE_ACCELEROMETER)

    private var lastMotionTime = 0L
    private val motionThreshold = 2.0f // m/s^2

    var isMoving = false
        private set

    fun start() {
        accelerometer?.let {
            sensorManager.registerListener(this, it, SensorManager.SENSOR_DELAY_NORMAL)
            Log.i("WristKeyMotion", "Accelerometer registered")
        }
    }

    fun stop() {
        sensorManager.unregisterListener(this)
    }

    override fun onSensorChanged(event: SensorEvent?) {
        event?.let {
            val x = it.values[0]
            val y = it.values[1]
            val z = it.values[2]
            val magnitude = kotlin.math.sqrt(x * x + y * y + z * z)

            if (kotlin.math.abs(magnitude - SensorManager.GRAVITY_EARTH) > motionThreshold) {
                lastMotionTime = System.currentTimeMillis()
                isMoving = true
            } else if (System.currentTimeMillis() - lastMotionTime > 10000) {
                isMoving = false
            }
        }
    }

    override fun onAccuracyChanged(sensor: Sensor?, accuracy: Int) {}
}
