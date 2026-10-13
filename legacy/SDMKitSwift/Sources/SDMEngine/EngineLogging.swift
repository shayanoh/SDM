import Foundation

#if SDM_ENGINE_LOGGING
    import os.log

    let engineLog = Logger(subsystem: "com.shayanoh.SDM", category: "engine")

    /// Short, grep-friendly tag for a download item: `[<id8> <filename>]`.
    /// `id8` disambiguates items that happen to share a filename.
    func itemTag(_ itemID: UUID, filename: String) -> String {
        "[\(itemID.uuidString.prefix(8)) \(filename)]"
    }

    func workerTag(_ itemID: UUID, filename: String, worker: Int) -> String {
        "\(itemTag(itemID, filename: filename)) [w\(worker)]"
    }

    func formatted(_ duration: Duration) -> String {
        let components = duration.components
        let ms = Double(components.seconds) * 1000 + Double(components.attoseconds) / 1e15
        return "\(Int(ms))ms"
    }
#endif
