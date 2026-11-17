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
import androidx.core.app.RemoteInput
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
        // While sharing or in SOS, the location goes on in the background too.
        val sharing = intent?.getBooleanExtra(SHARING, false) == true &&
            ContextCompat.checkSelfPermission(this, android.Manifest.permission.ACCESS_FINE_LOCATION) ==
            android.content.pm.PackageManager.PERMISSION_GRANTED
        val types = ServiceInfo.FOREGROUND_SERVICE_TYPE_CONNECTED_DEVICE or
            (if (sharing) ServiceInfo.FOREGROUND_SERVICE_TYPE_LOCATION else 0)
        val started = runCatching {
            ServiceCompat.startForeground(this, Notifications.SERVICE_ID, Notifications.service(this, name), types)
        }
        if (started.isFailure) {
            stopSelf()
            return START_NOT_STICKY
        }
        return START_STICKY
    }

    companion object {
        private const val NAME = "name"
        private const val SHARING = "sharing"

        fun start(context: Context, name: String, sharing: Boolean = false) {
            val i = Intent(context, RadioService::class.java).putExtra(NAME, name).putExtra(SHARING, sharing)
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
    private const val SOS = "sos"
    const val EXTRA_CHAT = "chat"
    const val EXTRA_SOS = "sos"

    fun channels(context: Context) {
        val m = context.getSystemService(NotificationManager::class.java) ?: return
        m.createNotificationChannel(
            NotificationChannel(RADIO, context.getString(R.string.channel_radio), NotificationManager.IMPORTANCE_LOW),
        )
        m.createNotificationChannel(
            NotificationChannel(MESSAGES, context.getString(R.string.channel_messages), NotificationManager.IMPORTANCE_HIGH),
        )
        m.createNotificationChannel(
            NotificationChannel(SOS, context.getString(R.string.channel_sos), NotificationManager.IMPORTANCE_HIGH).apply {
                enableVibration(true)
                setBypassDnd(true)
            },
        )
    }

    /** An SOS heard: on its own channel, as an alarm. */
    fun sos(context: Context, sender: String, name: String, noGps: Boolean) {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            return
        }
        val n = NotificationCompat.Builder(context, SOS)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(context.getString(R.string.sos_notification_title, name))
            .setContentText(context.getString(if (noGps) R.string.sos_notification_no_gps else R.string.sos_notification_text))
            .setPriority(NotificationCompat.PRIORITY_MAX)
            .setCategory(NotificationCompat.CATEGORY_ALARM)
            .setAutoCancel(true)
            .setContentIntent(
                PendingIntent.getActivity(
                    context, ("sos" + sender).hashCode(),
                    Intent(context, MainActivity::class.java)
                        .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP or Intent.FLAG_ACTIVITY_CLEAR_TOP)
                        .putExtra(EXTRA_SOS, sender),
                    PendingIntent.FLAG_IMMUTABLE or PendingIntent.FLAG_UPDATE_CURRENT,
                ),
            )
            .build()
        NotificationManagerCompat.from(context).notify(("sos" + sender).hashCode(), n)
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

    /** A received message, unless notifications are not allowed; `actions`: Reply and Mark as
     * read (ADR-0034), which the app lock leaves out. */
    fun message(context: Context, chat: ChatId, title: String, text: String, actions: Boolean = false) {
        if (ContextCompat.checkSelfPermission(context, Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) {
            return
        }
        val b = NotificationCompat.Builder(context, MESSAGES)
            .setSmallIcon(R.drawable.ic_notification)
            .setContentTitle(title)
            .setContentText(text)
            .setStyle(NotificationCompat.BigTextStyle().bigText(text))
            .setAutoCancel(true)
            .setContentIntent(open(context, chat))
            .setCategory(NotificationCompat.CATEGORY_MESSAGE)
        if (actions) {
            val input = RemoteInput.Builder(NotificationActions.KEY_TEXT).setLabel(context.getString(R.string.message_placeholder)).build()
            b.addAction(
                NotificationCompat.Action.Builder(
                    R.drawable.ic_notification, context.getString(R.string.reply),
                    // Mutable: the system fills in the typed text. The intent is explicit, to a
                    // receiver no other app can reach.
                    action(context, chat, NotificationActions.REPLY, PendingIntent.FLAG_MUTABLE),
                )
                    .addRemoteInput(input)
                    .setAllowGeneratedReplies(false)
                    .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_REPLY)
                    .setAuthenticationRequired(true)
                    .build(),
            )
            b.addAction(
                NotificationCompat.Action.Builder(
                    R.drawable.ic_notification, context.getString(R.string.mark_as_read),
                    action(context, chat, NotificationActions.READ, PendingIntent.FLAG_IMMUTABLE),
                )
                    .setSemanticAction(NotificationCompat.Action.SEMANTIC_ACTION_MARK_AS_READ)
                    .setShowsUserInterface(false)
                    .setAuthenticationRequired(true)
                    .build(),
            )
        }
        NotificationManagerCompat.from(context).notify(chat.hex.hashCode(), b.build())
    }

    private fun action(context: Context, chat: ChatId, what: String, mutability: Int): PendingIntent =
        PendingIntent.getBroadcast(
            context, (what + chat.route).hashCode(),
            Intent(context, NotificationActions::class.java).setAction(what).putExtra(EXTRA_CHAT, chat.route),
            mutability or PendingIntent.FLAG_UPDATE_CURRENT,
        )

    fun clear(context: Context, chat: ChatId) {
        NotificationManagerCompat.from(context).cancel(chat.hex.hashCode())
    }
}

/**
 * Reply and Mark as read from a message's notification (ADR-0034). Not exported: only the
 * notifications' own intents reach it, and still everything in them is checked as if another
 * app had written it (review of 2026-11-15, SR-25): the action, the conversation, the text.
 */
class NotificationActions : android.content.BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        val chat = intent.getStringExtra(Notifications.EXTRA_CHAT)?.let(ChatId::fromRoute) ?: return
        val model = (context.applicationContext as? com.tduhamel.lichen.LichenApplication)?.model ?: return
        when (intent.action) {
            REPLY -> {
                val text = RemoteInput.getResultsFromIntent(intent)?.getCharSequence(KEY_TEXT)?.toString()?.trim()
                if (text.isNullOrEmpty() || text.length > TEXT_MAX) {
                    Notifications.clear(context, chat)
                    return
                }
                model.replyFromNotification(chat, text)
            }
            READ -> model.markReadFromNotification(chat)
        }
    }

    companion object {
        const val REPLY = "com.tduhamel.lichen.REPLY"
        const val READ = "com.tduhamel.lichen.READ"
        const val KEY_TEXT = "text"

        /** What a text typed in a notification may hold at most: the core refuses what does
         * not fit a message anyway. */
        private const val TEXT_MAX = 4_096
    }
}
