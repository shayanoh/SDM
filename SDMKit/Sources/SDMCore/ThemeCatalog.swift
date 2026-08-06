import Foundation

/// Loads every bundled theme JSON file from `SDMCore`'s own resource
/// bundle. Spec §10.1: "adding a theme is adding a JSON file" — this is the
/// one place that enumerates them; nothing else changes to add a
/// seventeenth (Task 9 adds twelve more, all in the same
/// `Resources/Themes/` directory).
public enum ThemeCatalog {
    public static func builtInThemes() -> [Theme] {
        // `.process("Resources")` in Package.swift flattens the resource
        // tree into the built bundle's root rather than preserving the
        // `Resources/Themes/` nesting, so this looks up every `.json` at
        // the bundle root rather than passing a `subdirectory:`.
        guard
            let urls = Bundle.module.urls(forResourcesWithExtension: "json", subdirectory: nil)
        else { return [] }
        let decoder = JSONDecoder()
        return
            urls
            .compactMap { url -> Theme? in
                guard let data = try? Data(contentsOf: url) else { return nil }
                return try? decoder.decode(Theme.self, from: data)
            }
            .sorted { $0.name < $1.name }
    }
}
