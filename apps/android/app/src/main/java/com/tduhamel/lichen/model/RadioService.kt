package com.tduhamel.lichen.model

import android.Manifest
import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.Service
import android.content.Context
import android.content.Intent
import android.content.pm.PackageManager
import android.content.pm.ServiceInfo
import android.os.IBinder
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat
import androidx.core.app.ServiceCompat
import androidx.core.content.ContextCompat
import com.tduhamel.lichen.MainActivity
import com.tduhamel.lichen.R

/**
 * Keeps the process, and so the radio link, alive with the screen off (ADR-0006: receive while
 * connected), as iOS's Bluetooth background mode does. Runs while a radio is connected.
 */
class RadioService : Service() {
    override fun onBind(intent: Intent?): IBinder? = null

    override fun onStartCommand(intent: Intent?, flags: Int, startId: Int): Int {
        val name = intent?.getStringExtra(NAME) ?: "MeshCore"
        ServiceCompat.startForeground(
            this, Notifications.SERVICE_ID, Notifications.service(this, name),
            ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE,
        )
        return START_STICKY
    }

    companion object {
        private const val NAME = "name"

        fun start(context: Context, name: String) {
            val i = Intent(context, RadioService::class.java).putExtra(NAME, name)
            runCatching { ContextCompat.startForegroundService(context, i) }
        }

        fun stop(context: Context) {
            context.stopService(Intent(context, RadioService::class.java))
        }
    }
}

/** The radio link's ongoing notification and the received messages' ones. */
object Notifications {
    const val SERVICE_ID = 1
    private const val RADIO = "radio"
    private const val MESSAGES = "messages"
    const val EXTRA_CHAT = "chat"

    fun channels(context: Context) {
        val m = context.getSystemService(NotificationManager::class.java) ?: return
        m.createNotificationChannel(
            NotificationChannel(RADIO, context.getString(R.string.channel_radio), NotificationManager.IMPORTANCE_LOW),
        )
        m.createNotificationChannel(
            NotificationChannel(MESSAGES, context.getString(R.string.channel_messages), NotificationManager.IMPORTANCE_HIGH),
        )
    }

    private fun open(context: Context, chat: ChatId?): PendingIntent {
        val i = Intent(context, MainActivity::class.java)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
        chat?.let { i.putExtra(EXTRA_CHAT, it.route) }
        return PendingIntent.getActivity(
            context, chat?.route?.hashCode() ?: 0, i, PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
        )
    }

    fun service(context: Context, name: String): Notification =
        NotificationCompat.Builder(context, RADIO)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.radio_connected, name))
            .setContentText(context.getString(R.string.service_notification))
            .setOngoing(true)
            .setSilent(true)
            .setContentIntent(open(context, null))
            .build()

    /** A received message, unless notifications are not allowed. */
    fun message(context: Context, chat: ChatId, title: String, text: String) {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            return
        }
        val n = NotificationCompat.Builder(context, MESSAGES)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setAutoCancel(true)
            .setContentIntent(open(context, chat))
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
            .build()
        NotificationManagerCompat.from(context).notify(chat.hex.hashCode(), n)
    }

    fun clear(context: Context, chat: ChatId) {
        NotificationManagerCompat.from(context).cancel(chat.hex.hashCode())
    }
}
