import Foundation
import SDMCore

/// Pure translation of a package's grabber rows into engine
/// `DownloadItem`s. HTTP links become one-component items; a `.resolved`
/// media row becomes a 1–2-component item. Rows with no usable format are
/// held back. Parent spec §6.3.
public enum MediaHandoff {
    public static func build(
        httpLinks: [ProbedLink], mediaRows: [MediaRow]
    ) -> (items: [DownloadItem], heldBackCount: Int) {
        var items: [DownloadItem] = []
        var heldBack = 0

        for link in httpLinks {
            let filename =
                link.effectiveFilename.isEmpty ? "download" : link.effectiveFilename
            items.append(
                DownloadItem(
                    url: link.originalURL, filename: filename,
                    totalBytes: link.contentLength.flatMap { $0 > 0 ? $0 : nil },
                    metadata: ReleaseTags.extract(from: filename)))
        }

        for row in mediaRows {
            guard row.state == .resolved, let media = row.media, let choice = row.choice else {
                heldBack += 1
                continue
            }
            let output = row.displayFilename
            let stem = MediaRow.sanitize(media.title) + " [\(media.videoID)]"
            let mediaInfo = MediaMetadata.describe(choice: choice, media: media)
            var components: [FileComponent] = []

            // HLS/DASH-only: one non-resumable component that yt-dlp
            // downloads wholesale against the page URL. Spec §6.4.
            if let selector = choice.wholesaleSelector {
                components = [
                    FileComponent(
                        url: row.sourceURL,
                        partFilename:
                            "\(stem).\(choice.outputContainer.fileExtension)",
                        totalBytes: choice.estimatedBytes,
                        origin: .wholesale(formatSelector: selector),
                        isResumable: false)
                ]
                components[0].partFilename = output
                items.append(
                    DownloadItem(
                        components: components, outputFilename: output,
                        sourceURL: row.sourceURL, assembly: .none, state: .queued,
                        metadata: mediaInfo))
                continue
            }

            if let video = choice.video {
                components.append(
                    FileComponent(
                        url: video.url,
                        partFilename: "\(stem).f\(video.id).\(video.container.fileExtension)",
                        totalBytes: video.filesizeEffective,
                        origin: .resolved(formatID: video.id)))
            }
            if let audio = choice.audio {
                components.append(
                    FileComponent(
                        url: audio.url,
                        partFilename: "\(stem).f\(audio.id).\(audio.container.fileExtension)",
                        totalBytes: audio.filesizeEffective,
                        origin: .resolved(formatID: audio.id)))
            }
            guard !components.isEmpty else {
                heldBack += 1
                continue
            }
            // A single progressive/audio-only stream needs no assembly — name
            // its part file the final name from the start so nothing has to
            // rename it. Only a mux keeps `.fNNN` names (two parts share the
            // folder until ffmpeg combines them).
            if components.count == 1 {
                components[0].partFilename = output
            }
            items.append(
                DownloadItem(
                    components: components, outputFilename: output,
                    // The grabbed YouTube URL, not the googlevideo stream —
                    // this is what the details panel shows.
                    sourceURL: row.sourceURL,
                    assembly: components.count == 2 ? .mux : .none, state: .queued,
                    metadata: mediaInfo))
        }

        return (items, heldBack)
    }
}
