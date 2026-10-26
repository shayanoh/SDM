//! The extension ships a copy of the site list for its very first run. It
//! must match the registry. Regenerate with
//! `UPDATE_COOKIE_DOMAINS=1 cargo test -p sdm-browser bundled_domains`.

use sdm_resolve::SiteRegistry;

const FILE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/../../../SDMChromeExtension/cookie-domains.json"
);

fn expected() -> String {
    let value = serde_json::json!({
        "version": SiteRegistry::cookie_domains_version(),
        "domains": SiteRegistry::cookie_domains(),
    });
    serde_json::to_string_pretty(&value).unwrap() + "\n"
}

#[test]
fn bundled_domains_match_the_registry() {
    if std::env::var_os("UPDATE_COOKIE_DOMAINS").is_some() {
        std::fs::write(FILE, expected()).unwrap();
    }
    let on_disk = std::fs::read_to_string(FILE)
        .expect("SDMChromeExtension/cookie-domains.json is missing; regenerate it");
    assert_eq!(
        on_disk,
        expected(),
        "the extension's bundled site list is out of date; regenerate it"
    );
}
