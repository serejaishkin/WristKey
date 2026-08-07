package com.wristkey.app

import android.app.Application
import android.util.Log

class WristKeyApp : Application() {
    override fun onCreate() {
        super.onCreate()
        Log.i("WristKey", "Application started")
    }
}
