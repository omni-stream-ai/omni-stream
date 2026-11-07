package com.omnistreamai.app.push

import android.app.Activity
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import android.os.Bundle
import android.webkit.WebView
import androidx.core.app.NotificationCompat
import app.tauri.annotation.Command
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin
import com.google.firebase.messaging.FirebaseMessaging
import com.google.firebase.messaging.FirebaseMessagingService
import com.google.firebase.messaging.RemoteMessage
import java.net.HttpURLConnection
import java.net.URL
import org.json.JSONObject
import org.json.JSONArray

private const val CHANNEL = "omnistream_remote"
private const val PREFS = "omnistream_remote_push"
private val deliveryLock = Any()
private fun readKey(server: String, scope: String, instance: String) = JSONArray(listOf(server, scope, instance)).toString()

private fun preferences(context: Context) = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)

private fun channel(context: Context) {
    if (Build.VERSION.SDK_INT >= 26) {
        context.getSystemService(NotificationManager::class.java).createNotificationChannel(
            NotificationChannel(CHANNEL, "OmniStream", NotificationManager.IMPORTANCE_DEFAULT)
        )
    }
}

@TauriPlugin
class RemotePushPlugin(private val activity: Activity) : Plugin(activity) {
    override fun load(webView: WebView) {
        channel(activity)
        capture(activity.intent)
    }

    private fun capture(intent: Intent?) {
        val path = intent?.getStringExtra("path") ?: return
        val server = intent.getStringExtra("server") ?: return
        if (!path.startsWith("/chat/") && !path.startsWith("/slack?channel=")) return
        val data = JSObject().put("path", path).put("server", server)
        preferences(activity).edit().putString("click", data.toString()).apply()
        // Remove consumed intent extras so a recreation does not reopen an old notification.
        intent.removeExtra("path")
        intent.removeExtra("server")
        trigger("click", JSObject(data.toString()))
    }

    override fun onNewIntent(intent: Intent) { capture(intent) }

    @Command
    fun consumeClick(invoke: Invoke) {
        val prefs = preferences(activity)
        val value = prefs.getString("click", null)
        prefs.edit().remove("click").apply()
        invoke.resolve(value?.let { JSObject(it) })
    }

    @Command
    fun token(invoke: Invoke) {
        channel(activity)
        val messaging = FirebaseMessaging.getInstance()
        messaging.isAutoInitEnabled = true
        messaging.token.addOnCompleteListener { task ->
            if (task.isSuccessful) invoke.resolve(JSObject().apply { put("token", task.result) })
            else invoke.reject("Unable to obtain FCM token")
        }
    }

    @Command
    fun saveBinding(invoke: Invoke) {
        val binding = invoke.getArgs()
        val server = binding.optString("server")
        if (!server.startsWith("https://") && !server.startsWith("http://")) {
            invoke.reject("Invalid Core address")
            return
        }
        val prefs = preferences(activity)
        val bindings = JSONObject(prefs.getString("bindings", "{}") ?: "{}")
        bindings.put(server, JSONObject(binding.toString()))
        if (!prefs.edit().putString("bindings", bindings.toString()).commit()) {
            invoke.reject("Unable to save push binding")
            return
        }
        invoke.resolve()
    }

    @Command
    fun markRead(invoke: Invoke) {
        val input = invoke.getArgs()
        val server = input.optString("server")
        val scope = input.optString("scope")
        val instance = input.optString("instance")
        val cursor = input.optLong("cursor", -1)
        if (server.isEmpty() || scope.isEmpty() || instance.isEmpty() || cursor < 0) {
            invoke.reject("Invalid notification receipt")
            return
        }
        synchronized(deliveryLock) {
            val prefs = preferences(activity)
            val reads = JSONObject(prefs.getString("reads", "{}") ?: "{}")
            val key = readKey(server, scope, instance)
            val through = maxOf(cursor, reads.optLong(key, -1))
            reads.put(key, through)
            if (!prefs.edit().putString("reads", reads.toString()).commit()) {
                invoke.reject("Unable to save notification receipt")
                return
            }
            val manager = activity.getSystemService(NotificationManager::class.java)
            for (shown in manager.activeNotifications) {
                val data = shown.notification.extras
                if (data.getString("omnistream_server") == server && data.getString("omnistream_scope") == scope
                    && data.getString("omnistream_instance") == instance && data.getLong("omnistream_id", Long.MAX_VALUE) <= through) {
                    manager.cancel(shown.tag, shown.id)
                }
            }
        }
        invoke.resolve()
    }

    @Command
    fun removeBinding(invoke: Invoke) {
        val server = invoke.getArgs().optString("server")
        val prefs = preferences(activity)
        val bindings = JSONObject(prefs.getString("bindings", "{}") ?: "{}")
        bindings.remove(server)
        if (!prefs.edit().putString("bindings", bindings.toString()).commit()) {
            invoke.reject("Unable to remove push binding")
            return
        }
        if (bindings.length() == 0) FirebaseMessaging.getInstance().isAutoInitEnabled = false
        invoke.resolve()
    }
}

class OmniMessagingService : FirebaseMessagingService() {
    override fun onNewToken(token: String) {
        // FCM callbacks run on a worker thread. Refresh each device-limited binding once;
        // failures are exposed on next app launch and never retried in a background loop.
        val bindings = JSONObject(preferences(this).getString("bindings", "{}") ?: "{}")
        for (server in bindings.keys()) {
            val binding = bindings.getJSONObject(server)
            try {
                val body = JSONObject(binding.toString()).put("target", token).put("activate", false).apply { remove("clientToken") }
                val connection = URL("$server/push/devices/register").openConnection() as HttpURLConnection
                try {
                    connection.requestMethod = "POST"
                    connection.instanceFollowRedirects = false
                    connection.connectTimeout = 15000
                    connection.readTimeout = 15000
                    connection.doOutput = true
                    connection.setRequestProperty("Content-Type", "application/json")
                    binding.optString("clientToken").takeIf { it.isNotEmpty() }?.let { connection.setRequestProperty("Authorization", "Bearer " + it) }
                    connection.outputStream.use { it.write(body.toString().toByteArray(Charsets.UTF_8)) }
                    if (connection.responseCode !in 200..299) preferences(this).edit().putBoolean("refresh_failed:$server", true).apply()
                } finally { connection.disconnect() }
            } catch (_: Exception) {
                preferences(this).edit().putBoolean("refresh_failed:$server", true).apply()
            }
        }
    }

    override fun onMessageReceived(message: RemoteMessage) {
        synchronized(deliveryLock) { deliver(message) }
    }

    private fun deliver(message: RemoteMessage) {
        val data = message.data
        val server = data["server"] ?: return
        val scope = data["scope"] ?: return
        val instance = data["instance"] ?: return
        val id = data["id"]?.toLongOrNull() ?: return
        val reads = JSONObject(preferences(this).getString("reads", "{}") ?: "{}")
        if (reads.optLong(readKey(server, scope, instance), -1) >= id) return
        val bindings = JSONObject(preferences(this).getString("bindings", "{}") ?: "{}")
        if (bindings.optJSONObject(server)?.optString("id") != data["device"]) return
        channel(this)
        val intent = packageManager.getLaunchIntentForPackage(packageName) ?: return
        intent.addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP or Intent.FLAG_ACTIVITY_SINGLE_TOP)
        intent.putExtra("path", data["path"])
        intent.putExtra("server", server)
        val tag = data["tag"] ?: return
        val click = PendingIntent.getActivity(this, tag.hashCode(), intent, PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        val icon = resources.getIdentifier("ic_stat_omnistream", "drawable", packageName)
        val notification = NotificationCompat.Builder(this, CHANNEL)
            .setSmallIcon(icon).setContentTitle(data["title"]).setContentText(data["body"])
            .setContentIntent(click).setAutoCancel(true).addExtras(Bundle().apply {
                putString("omnistream_server", server); putString("omnistream_scope", scope)
                putString("omnistream_instance", instance); putLong("omnistream_id", id)
            }).build()
        try { getSystemService(NotificationManager::class.java).notify(tag, 0, notification) }
        catch (_: SecurityException) { /* Permission may have been revoked in system settings. */ }
    }
}
