import UserNotifications
import os.log

/// `UNUserNotificationCenter` on macOS (like iOS) silently drops a
/// notification's banner while the app is active/frontmost unless a
/// delegate is set and its `willPresent` explicitly opts back in — with no
/// delegate at all (the previous state here), every notification fired
/// while SDM was the frontmost app, which is most of the time anyone would
/// actually notice one, was posted successfully and then shown nowhere.
/// `NSObject` conformance is required by `UNUserNotificationCenterDelegate`.
@MainActor
final class NotificationManager: NSObject, UNUserNotificationCenterDelegate {
    private nonisolated static let log = Logger(subsystem: "com.shayanoh.SDM", category: "notifications")

    override init() {
        super.init()
        UNUserNotificationCenter.current().delegate = self
    }

    nonisolated func userNotificationCenter(
        _ center: UNUserNotificationCenter,
        willPresent notification: UNNotification,
        withCompletionHandler completionHandler:
            @escaping (UNNotificationPresentationOptions) ->
            Void
    ) {
        completionHandler([.banner, .sound, .list])
    }

    func requestAuthorization() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) {
            granted, error in
            if let error {
                Self.log.error("Notification authorization request failed: \(error)")
            } else if !granted {
                Self.log.notice(
                    "Notification authorization was denied — enable it in System Settings > Notifications > SDM."
                )
            }
        }
    }

    func notifyDownloadFinished(filename: String) {
        guard NotificationSettings.downloadFinishedEnabled else { return }
        post(title: "Download finished", body: filename)
    }

    func notifyPackageFinished(name: String) {
        guard NotificationSettings.packageFinishedEnabled else { return }
        post(title: "Package finished", body: name)
    }

    func notifyDownloadFailed(filename: String, reason: String) {
        guard NotificationSettings.downloadFailedEnabled else { return }
        post(title: "Download failed", body: "\(filename): \(reason)")
    }

    func notifyLinksGrabbed(count: Int) {
        guard NotificationSettings.linksGrabbedEnabled, count > 0 else { return }
        post(title: "\(count) links grabbed", body: "Waiting for confirmation in the Linkgrabber")
    }

    private func post(title: String, body: String) {
        let content = UNMutableNotificationContent()
        content.title = title
        content.body = body
        let request = UNNotificationRequest(
            identifier: UUID().uuidString, content: content, trigger: nil)
        UNUserNotificationCenter.current().add(request)
    }
}
