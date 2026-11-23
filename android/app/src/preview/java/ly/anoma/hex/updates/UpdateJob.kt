package ly.anoma.hex.updates

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.app.job.JobInfo
import android.app.job.JobParameters
import android.app.job.JobScheduler
import android.app.job.JobService
import android.content.ComponentName
import android.content.Context
import android.content.Intent
import android.os.PersistableBundle
import android.os.SystemClock
import androidx.annotation.RequiresApi
import ly.anoma.hex.MainActivity
import ly.anoma.hex.R
import java.util.concurrent.atomic.AtomicBoolean

/** Android 14+: the update download as a user-initiated data transfer, scheduled from the person's tap on
 * Update, so it keeps going when they leave Hex Preview (a plain download is cut off seconds later). Android
 * lists it among running apps; its notification shows only if Hex Preview may post notifications, which it
 * never asks for. Downloading and checking stay in [Updates] and [Channel]. */
class UpdateJob : JobService() {
    private var stop: AtomicBoolean? = null

    @RequiresApi(34)
    override fun onStartJob(params: JobParameters): Boolean {
        val r = params.extras.getString("release")?.let(Release::parse) ?: return false
        val c = applicationContext
        setNotification(params, DOWNLOADING, downloading(c, r, 0), JOB_END_NOTIFICATION_POLICY_REMOVE)
        val stop = AtomicBoolean(false).also { this.stop = it }
        var shownAt = 0L
        val progress = { percent: Int ->
            val now = SystemClock.uptimeMillis()
            if (now - shownAt >= 1000) {
                shownAt = now
                setNotification(params, DOWNLOADING, downloading(c, r, percent), JOB_END_NOTIFICATION_POLICY_REMOVE)
            }
        }
        Updates.fetch(c, r, stop::get, progress) { file ->
            // Out of sight, the person installs it from the Home card when they come back; never by itself.
            if (file != null && Updates.ready(c, r, file)) {
                c.getSystemService(NotificationManager::class.java).notify(READY, notification(c)
                    .setContentTitle(c.getString(R.string.updates_notification_ready, r.version))
                    .setContentText(c.getString(R.string.updates_notification_open)).setAutoCancel(true).build())
            }
            jobFinished(params, false)
        }
        return true
    }

    /** Android stopped it: the download ends at its next read, its partial file is deleted, and the Home card
     * says it stopped, with Try again. Not rescheduled: a download starts only from a tap. */
    override fun onStopJob(params: JobParameters): Boolean {
        stop?.set(true)
        return false
    }

    companion object {
        private const val JOB = 1
        private const val DOWNLOADING = 1
        const val READY = 2

        /** False when Android won't take it, so the caller downloads in its own process instead. */
        @RequiresApi(34)
        internal fun schedule(c: Context, r: Release): Boolean = runCatching {
            val job = JobInfo.Builder(JOB, ComponentName(c, UpdateJob::class.java))
                .setUserInitiated(true)
                .setRequiredNetworkType(JobInfo.NETWORK_TYPE_ANY)
                .setEstimatedNetworkBytes(r.size, 0)
                .setExtras(PersistableBundle().apply { putString("release", r.json()) })
                .build()
            c.getSystemService(JobScheduler::class.java).schedule(job) == JobScheduler.RESULT_SUCCESS
        }.getOrDefault(false)

        private fun downloading(c: Context, r: Release, percent: Int) = notification(c)
            .setContentTitle(c.getString(R.string.updates_notification_downloading, r.version))
            .setProgress(100, percent, false).setOngoing(true).setOnlyAlertOnce(true)
            .setCategory(Notification.CATEGORY_PROGRESS).build()

        /** Low importance: no sound or pop-up. Tapping it opens Hex Preview. */
        private fun notification(c: Context): Notification.Builder {
            c.getSystemService(NotificationManager::class.java).createNotificationChannel(
                NotificationChannel("updates", c.getString(R.string.updates_channel), NotificationManager.IMPORTANCE_LOW))
            val open = Intent(c, MainActivity::class.java).addFlags(Intent.FLAG_ACTIVITY_NEW_TASK or Intent.FLAG_ACTIVITY_CLEAR_TOP)
            return Notification.Builder(c, "updates").setSmallIcon(R.drawable.hex_mark)
                .setContentIntent(PendingIntent.getActivity(c, 0, open, PendingIntent.FLAG_IMMUTABLE))
        }
    }
}
