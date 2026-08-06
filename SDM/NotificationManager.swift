import UserNotifications

@MainActor
final class NotificationManager {
    func requestAuthorization() {
        UNUserNotificationCenter.current().requestAuthorization(options: [.alert, .sound]) { _, _ in
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
