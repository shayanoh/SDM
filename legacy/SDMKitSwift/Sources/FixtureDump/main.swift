import Foundation
import SDMCore
import SDMEngine
import SDMResolve

// FixtureDump: writes sample persistence files using the real Swift encoders so the
// Rust core can prove it reads and writes the same on-disk JSON. Usage:
//   swift run FixtureDump <output-directory>
// Everything is fixed (UUIDs, strings, numbers) so a re-run is byte-stable.

func fail(_ message: String, code: Int32) -> Never {
    FileHandle.standardError.write(Data("FixtureDump: \(message)\n".utf8))
    exit(code)
}

let arguments = CommandLine.arguments
guard arguments.count == 2 else { fail("usage: FixtureDump <output-directory>", code: 2) }
let outDir = URL(fileURLWithPath: arguments[1], isDirectory: true)
var isDir: ObjCBool = false
guard FileManager.default.fileExists(atPath: outDir.path, isDirectory: &isDir), isDir.boolValue,
    FileManager.default.isWritableFile(atPath: outDir.path)
else { fail("\(outDir.path) is not an existing, writable directory", code: 2) }

func uuid(_ n: Int) -> UUID {
    let tail = String(format: "%012d", n)
    return UUID(uuidString: "00000000-0000-4000-8000-\(tail)")!
}

func url(_ s: String) -> URL {
    guard let u = URL(string: s) else { fail("bad URL literal \(s)", code: 1) }
    return u
}

/// Re-serializes production-encoded JSON with sorted keys so the file is byte-stable
/// even when the production encoder's key order is not.
///
/// With `sortArrays`, arrays are also ordered by their serialized text; used only for values
/// that contain a Swift `Set` (random order per process), where the order carries no meaning.
func canonical(_ data: Data, sortArrays: Bool = false) throws -> Data {
    var object = try JSONSerialization.jsonObject(with: data, options: [.fragmentsAllowed])
    if sortArrays { object = try sortedArrays(object) }
    var out = try JSONSerialization.data(
        withJSONObject: object,
        options: [.sortedKeys, .prettyPrinted, .withoutEscapingSlashes, .fragmentsAllowed])
    out.append(0x0A)
    return out
}

func sortedArrays(_ value: Any) throws -> Any {
    if let array = value as? [Any] {
        let mapped = try array.map(sortedArrays)
        let keyed = try mapped.map { element -> (String, Any) in
            let text = try JSONSerialization.data(
                withJSONObject: element, options: [.sortedKeys, .fragmentsAllowed])
            return (String(decoding: text, as: UTF8.self), element)
        }
        return keyed.sorted { $0.0 < $1.0 }.map { $0.1 }
    }
    if let dict = value as? [String: Any] { return try dict.mapValues(sortedArrays) }
    return value
}

func write(_ name: String, _ data: Data) throws {
    try data.write(to: outDir.appendingPathComponent(name), options: .atomic)
    print("wrote \(name) (\(data.count) bytes)")
}

func scratch(_ name: String) -> URL { outDir.appendingPathComponent(".scratch-\(name)") }

// MARK: - state_rich.json

func range(_ s: Int64, _ e: Int64) -> ByteRange { ByteRange(start: s, end: e) }

/// A one-component HTTP item with a fixed component id. (The flat convenience initializer
/// would mint a random component UUID, which would make the dump unstable.)
func httpItem(
    id: UUID, componentID: UUID, url: URL, filename: String, totalBytes: Int64? = nil,
    completed: RangeSet = RangeSet(), state: ItemState = .queued, isEnabled: Bool = true,
    isResumable: Bool? = nil, priority: Priority? = nil, position: Int = 0,
    validator: String? = nil, metadata: String? = nil
) -> DownloadItem {
    DownloadItem(
        id: id,
        components: [
            FileComponent(
                id: componentID, url: url, partFilename: filename, totalBytes: totalBytes,
                completed: completed, validator: validator, origin: .http,
                isResumable: isResumable)
        ],
        outputFilename: filename, state: state, isEnabled: isEnabled, priority: priority,
        position: position, metadata: metadata)
}

let httpStopped = httpItem(
    id: uuid(101), componentID: uuid(1011), url: url("https://example.com/files/a.zip"),
    filename: "a.zip",
    totalBytes: nil, state: .stopped, isEnabled: false, isResumable: nil,
    priority: .lowest, position: 0)

let httpQueued = httpItem(
    id: uuid(102), componentID: uuid(1021), url: url("https://example.com/files/b.iso"),
    filename: "b.iso",
    totalBytes: 4_294_967_296, state: .queued, isEnabled: true, isResumable: true,
    priority: .low, position: 1, validator: "\"etag-b\"",
    metadata: "1080p · WEB-DL · H.264")

let httpRunning = httpItem(
    id: uuid(103), componentID: uuid(1031), url: url("https://example.com/files/c.bin"),
    filename: "c.bin",
    totalBytes: 10_000_000_000,
    completed: RangeSet([
        range(0, 1_000_000), range(2_000_000, 3_500_000), range(5_000_000_000, 9_999_999_999),
    ]),
    state: .running, isEnabled: true, isResumable: false, priority: .normal, position: 2,
    validator: "Wed, 21 Oct 2015 07:28:00 GMT")

let httpCompleted = httpItem(
    id: uuid(104), componentID: uuid(1041), url: url("https://example.com/files/d.pdf"),
    filename: "d.pdf",
    totalBytes: 2048, completed: RangeSet([range(0, 2048)]),
    state: .completed, isEnabled: true, isResumable: true, priority: .high, position: 3)

let packageHTTP = DownloadPackage(
    id: uuid(1), name: "Mixed HTTP", items: [httpStopped, httpQueued, httpRunning, httpCompleted],
    priority: .normal, position: 0)

let muxed = DownloadItem(
    id: uuid(201),
    components: [
        FileComponent(
            id: uuid(2011), url: url("https://rr1.googlevideo.com/videoplayback?id=v"),
            partFilename: "Clip.f137.mp4", totalBytes: 50_000_000,
            completed: RangeSet([range(0, 25_000_000)]), validator: "v-etag",
            origin: .resolved(formatID: "137"), isResumable: true),
        FileComponent(
            id: uuid(2012), url: url("https://rr1.googlevideo.com/videoplayback?id=a"),
            partFilename: "Clip.f251.webm", totalBytes: 5_000_000,
            completed: RangeSet([range(0, 5_000_000)]),
            origin: .resolved(formatID: "251"), isResumable: nil),
    ],
    outputFilename: "Clip.mp4", sourceURL: url("https://www.youtube.com/watch?v=abc123"),
    assembly: .mux, state: .running, isEnabled: true, priority: .highest, position: 0,
    metadata: "1080p · AV1 · mp4 · youtube")

let packageMedia = DownloadPackage(
    id: uuid(2), name: "YouTube", items: [muxed], priority: .high, position: 1)

let wholesale = DownloadItem(
    id: uuid(301),
    components: [
        FileComponent(
            id: uuid(3011), url: url("https://cdn.example.net/master.m3u8"),
            partFilename: "Stream.mp4", origin: .wholesale(formatSelector: "bv*+ba/b"),
            isResumable: true)
    ],
    outputFilename: "Stream.mp4", sourceURL: url("https://vimeo.com/123456"),
    state: .queued, isEnabled: true, priority: nil, position: 0,
    metadata: "HLS · 720p")

let failedItem = DownloadItem(
    id: uuid(302),
    components: [
        FileComponent(
            id: uuid(3021), url: url("https://example.com/gone.bin"),
            partFilename: "gone.bin", totalBytes: 1000, completed: RangeSet([range(0, 10)]),
            isResumable: true, lastError: ComponentError("HTTP 404"))
    ],
    outputFilename: "gone.bin", state: .failed(reason: "HTTP 404"), isEnabled: true,
    priority: .normal, position: 1)

// Foundation's URL(string:) percent-encodes the whole string once it sees a non-ASCII
// character, so the "%20" below is written as "%2520". That is what the app really stores.
let urlOdd = httpItem(
    id: uuid(303), componentID: uuid(3031),
    url: url("https://EXAMPLE.com:443/a%20b/ü?x=&y=1#frag"),
    filename: "ü file.bin", state: .stopped, isEnabled: true, position: 2)
let urlSpace = httpItem(
    id: uuid(304), componentID: uuid(3041),
    url: url("https://EXAMPLE.com:443/a%20b/c?x=&y=1#frag"),
    filename: "space.bin", position: 3)
let urlTrailingDot = httpItem(
    id: uuid(305), componentID: uuid(3051), url: url("https://example.com./p?"),
    filename: "dot.bin", position: 4)

let packageOdd = DownloadPackage(
    id: uuid(3), name: "Odd ones",
    items: [wholesale, failedItem, urlOdd, urlSpace, urlTrailingDot],
    priority: .lowest, position: 2, note: "3 of 320 videos")

let richState = PersistedState(packages: [packageHTTP, packageMedia, packageOdd])

// MARK: - state_minimal.json

let minimalState = PersistedState(packages: [
    DownloadPackage(
        id: uuid(10), name: "P",
        items: [
            httpItem(
                id: uuid(11), componentID: uuid(111), url: url("https://example.com/m.bin"),
                filename: "m.bin")
        ])
])

/// Saves through the production `JSONStateStore` (flush) and returns the bytes it wrote.
func productionStateBytes(_ state: PersistedState, tag: String) async throws -> Data {
    let file = scratch(tag)
    try? FileManager.default.removeItem(at: file)
    defer { try? FileManager.default.removeItem(at: file) }
    let store = JSONStateStore(fileURL: file)
    await store.save(state)
    await store.flush()
    return try Data(contentsOf: file)
}

try await write("state_rich.json", canonical(productionStateBytes(richState, tag: "rich")))
try await write("state_minimal.json", canonical(productionStateBytes(minimalState, tag: "min")))

// MARK: - state_legacy_flat.json (cannot come from the encoder; decoded to prove acceptance)

let legacyLiteral = """
    {
      "formatVersion": 1,
      "packages": [
        {
          "id": "00000000-0000-4000-8000-000000000020",
          "name": "Legacy",
          "priority": 2,
          "position": 0,
          "items": [
            {
              "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427",
              "url": "https://x/f.bin",
              "filename": "f.bin",
              "totalBytes": 2048,
              "completed": { "ranges": [ { "start": 0, "end": 1024 } ] },
              "validator": "\\"legacy-etag\\"",
              "state": { "stopped": {} },
              "isEnabled": true,
              "isResumable": true,
              "priority": 3,
              "position": 3
            },
            {
              "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA428",
              "url": "https://x/g.bin",
              "filename": "g.bin",
              "state": { "failed": { "reason": "boom" } },
              "isEnabled": false,
              "position": 0
            }
          ]
        }
      ]
    }
    """
do {
    let decoded = try JSONDecoder().decode(PersistedState.self, from: Data(legacyLiteral.utf8))
    guard decoded.formatVersion == 1, decoded.packages.count == 1,
        decoded.packages[0].items.count == 2,
        decoded.packages[0].items.allSatisfy({
            $0.components.count == 1 && $0.components[0].origin == .http
        }),
        decoded.packages[0].items[0].totalBytes == 2048,
        decoded.packages[0].items[1].isEnabled == false
    else { fail("legacy fixture decoded to an unexpected value", code: 1) }
    // Also prove the production store loads it (and normalizes to v2).
    let file = scratch("legacy")
    try Data(legacyLiteral.utf8).write(to: file)
    defer { try? FileManager.default.removeItem(at: file) }
    let loaded = await JSONStateStore(fileURL: file).load()
    guard loaded.packages.count == 1, loaded.formatVersion == PersistedState.currentFormatVersion
    else { fail("JSONStateStore did not load the legacy fixture", code: 1) }
} catch {
    fail("legacy fixture does not decode with the real JSONDecoder: \(error)", code: 1)
}
try write("state_legacy_flat.json", canonical(Data(legacyLiteral.utf8)))

// MARK: - sidecars

func sidecarBytes(_ sidecar: ResumeSidecar, tag: String) throws -> Data {
    let file = scratch(tag)
    defer { try? FileManager.default.removeItem(at: file) }
    try sidecar.save(to: file)
    return try Data(contentsOf: file)
}

let richSidecar = ResumeSidecar(
    sourceURL: url("https://download.example.org/pub/big.iso"),
    resolvedURL: url("https://mirror2.example.org/big.iso?token=abc"),
    totalBytes: 8_589_934_592, validator: "\"etag-xyz\"",
    completed: RangeSet([range(0, 1_048_576), range(4_294_967_296, 5_000_000_000)]),
    durableCompleted: RangeSet([range(0, 524_288)]),
    bootSessionID: "11111111-2222-4333-8444-555555555555")
let minimalSidecar = ResumeSidecar(
    sourceURL: url("https://example.com/m.bin"), resolvedURL: nil, totalBytes: 1,
    validator: nil, completed: RangeSet())
try write("sidecar_rich.json", canonical(sidecarBytes(richSidecar, tag: "sc1")))
try write("sidecar_minimal.json", canonical(sidecarBytes(minimalSidecar, tag: "sc2")))

// MARK: - binaries_manifest.json
// BinariesManifest's memberwise init is internal, so build it by decoding, then encode with
// the exact settings ManagedBinaries.saveManifest uses.
let manifestLiteral = """
    {"ytDlpVersion":"2026.09.01.123456","ytDlpChannel":"nightly","lastCheckTick":4242,
     "ffmpegVersion":"7.1","qjsVersion":"2025-09-13","lastError":"offline: timed out"}
    """
let manifest = try JSONDecoder().decode(BinariesManifest.self, from: Data(manifestLiteral.utf8))
let manifestEncoder = JSONEncoder()
manifestEncoder.outputFormatting = [.prettyPrinted, .sortedKeys]
var manifestData = try manifestEncoder.encode(manifest)
manifestData.append(0x0A)
try write("binaries_manifest.json", manifestData)

// MARK: - media

let formats: [MediaFormat] = [
    MediaFormat(
        id: "137", kind: .videoOnly, height: 1080, width: 1920, vcodec: .h264, acodec: nil,
        container: .mp4, filesize: 50_000_000, filesizeApprox: nil, tbr: 4500.5,
        url: url("https://rr1.googlevideo.com/videoplayback?id=137")),
    MediaFormat(
        id: "399", kind: .videoOnly, height: 1080, width: 1920, vcodec: .av1, acodec: nil,
        container: .mp4, filesize: nil, filesizeApprox: 30_000_000, tbr: 2500.25,
        url: url("https://rr1.googlevideo.com/videoplayback?id=399")),
    MediaFormat(
        id: "248", kind: .videoOnly, height: 1080, width: 1920, vcodec: .vp9, acodec: nil,
        container: .webm, filesize: 40_000_000, filesizeApprox: 41_000_000, tbr: 3000,
        url: url("https://rr1.googlevideo.com/videoplayback?id=248")),
    MediaFormat(
        id: "251", kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: .opus,
        container: .webm, filesize: 5_000_000, filesizeApprox: nil, tbr: 130,
        url: url("https://rr1.googlevideo.com/videoplayback?id=251")),
    MediaFormat(
        id: "140", kind: .audioOnly, height: nil, width: nil, vcodec: nil, acodec: .aac,
        container: .m4a, filesize: nil, filesizeApprox: nil, tbr: nil,
        url: url("https://rr1.googlevideo.com/videoplayback?id=140")),
    MediaFormat(
        id: "18", kind: .progressive, height: 360, width: 640, vcodec: .h264, acodec: .aac,
        container: .mp4, filesize: 12_345_678, filesizeApprox: nil, tbr: 500,
        url: url("https://rr1.googlevideo.com/videoplayback?id=18")),
    MediaFormat(
        id: "hls-720", kind: .progressive, height: 720, width: 1280, vcodec: .other("hevc"),
        acodec: .other("ac-3"), container: .other("mkv"), filesize: nil, filesizeApprox: nil,
        tbr: 2000, url: url("https://cdn.example.net/master.m3u8"), delivery: .hls),
    MediaFormat(
        id: "dash-1", kind: .videoOnly, height: 480, width: 854, vcodec: .other("vp8"),
        acodec: nil, container: .other("ogv"), filesize: nil, filesizeApprox: nil, tbr: nil,
        url: url("https://cdn.example.net/manifest.mpd"), delivery: .dash),
]

let media = ResolvedMedia(
    extractor: "youtube", videoID: "abc123", title: "Clip: a \"title\" / ü",
    durationSeconds: 212.5, formats: formats,
    sourceURL: url("https://www.youtube.com/watch?v=abc123"))

let choice = FormatChoice(
    video: formats[0], audio: formats[3], outputContainer: .mp4, estimatedBytes: 55_000_000,
    wholesaleSelector: nil)
let wholesaleChoice = FormatChoice(
    video: formats[6], audio: nil, outputContainer: .other("mkv"), estimatedBytes: nil,
    wholesaleSelector: "bv*+ba/b")

let plainEncoder = JSONEncoder()
try write("resolved_media.json", canonical(plainEncoder.encode(media)))
try write("media_formats.json", canonical(plainEncoder.encode(formats)))
try write("format_choices.json", canonical(plainEncoder.encode([choice, wholesaleChoice])))

let prefs = QualityPreferences.default
try write("quality_preferences.json", canonical(plainEncoder.encode(prefs), sortArrays: true))

// MARK: - themes

let themesSource = URL(fileURLWithPath: #filePath)
    .deletingLastPathComponent().deletingLastPathComponent()
    .appendingPathComponent("SDMCore/Resources/Themes", isDirectory: true)
let themesOut = outDir.appendingPathComponent("themes", isDirectory: true)
try FileManager.default.createDirectory(at: themesOut, withIntermediateDirectories: true)
let themeFiles = try FileManager.default.contentsOfDirectory(atPath: themesSource.path)
    .filter { $0.hasSuffix(".json") }.sorted()
guard !themeFiles.isEmpty else { fail("no theme files found at \(themesSource.path)", code: 1) }
for name in themeFiles {
    try Data(contentsOf: themesSource.appendingPathComponent(name))
        .write(to: themesOut.appendingPathComponent(name), options: .atomic)
}
print("copied \(themeFiles.count) themes")
