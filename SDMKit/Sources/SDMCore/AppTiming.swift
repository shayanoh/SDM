import Foundation

/// Single source of truth for the engine's heartbeat rate. Every mechanism
/// that counts ticks to measure real time — checkpoint staleness, the
/// hysteresis window, the persistence debounce, retry backoff, and speed-
/// sampler history length — derives its tick count from this rather than
/// assuming 1 tick == 1 second.
///
/// Raising this makes the UI redraw more often and every sparkline/graph
/// visibly smoother; it does not change how many *seconds* any of the above
/// waits, since every consumer multiplies its "N seconds" by this value
/// rather than hardcoding a tick count.
public enum AppTiming {
    /// Heartbeat ticks per second. `DownloadEngine.tick()` is driven at this
    /// rate by `EngineController`'s loop, and every SwiftUI snapshot publish
    /// rides the same rate — a slower or faster refresh is a one-line
    /// change here. (A dedicated in-flight drag/reorder can still interrupt
    /// a `List`-backed `NSTableView` if its data source reloads mid-drag;
    /// `EngineController` handles that separately by pausing publishing
    /// while a mouse button is held down, not by throttling this rate.)
    public static let ticksPerSecond: Int = 5
}
