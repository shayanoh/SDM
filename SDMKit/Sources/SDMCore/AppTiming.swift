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
    /// rate by `EngineController`'s loop; a slower or faster refresh is a
    /// one-line change here.
    public static let ticksPerSecond: Int = 5

    /// How often `EngineController` republishes a fresh snapshot to
    /// SwiftUI, independent of `ticksPerSecond` — the engine still ticks
    /// (and keeps its speed math, checkpointing, and backoff timing) at the
    /// full rate regardless. Kept lower on purpose: reassigning the
    /// published snapshot invalidates every `List` that reads it, and doing
    /// that as often as `ticksPerSecond` visibly interrupts an in-flight
    /// drag-and-drop reorder — AppKit resets a table's drag session when its
    /// data source reloads mid-drag.
    public static let uiRefreshesPerSecond: Int = 2
}
