//! Pins that what the shipping Swift app writes loads, and that what Rust
//! writes back is what Swift would have written. Fixtures are Swift-written
//! (`fixtures/swift/`); comparisons are semantic (`serde_json::Value`, key
//! order ignored).

use std::collections::HashSet;

use sdm_core::{
    DownloadItem, DownloadPackage, FormatChoice, MediaFormat, QualityPreferences, ResolvedMedia,
    SourceUrl,
};
use serde_json::{Value, json};
use url::Url;

fn fixture(name: &str) -> Value {
    let path = format!("{}/../../fixtures/swift/{name}", env!("CARGO_MANIFEST_DIR"));
    serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap()
}

fn packages_round_trip(name: &str) {
    let packages = fixture(name)["packages"].clone();
    let decoded: Vec<DownloadPackage> = serde_json::from_value(packages.clone()).unwrap();
    assert!(!decoded.is_empty());
    assert_eq!(serde_json::to_value(&decoded).unwrap(), packages);
}

#[test]
fn state_rich_round_trips_semantically() {
    packages_round_trip("state_rich.json");
}

#[test]
fn state_minimal_round_trips_semantically() {
    packages_round_trip("state_minimal.json");
    // Every optional field really is absent, and stays absent.
    let packages: Vec<DownloadPackage> =
        serde_json::from_value(fixture("state_minimal.json")["packages"].clone()).unwrap();
    let item = &packages[0].items[0];
    assert_eq!(item.priority, None);
    assert_eq!(item.metadata, None);
    assert_eq!(packages[0].note, None);
    assert_eq!(item.components[0].total_bytes, None);
    assert_eq!(item.components[0].last_error, None);
}

#[test]
fn state_legacy_flat_decodes_and_reserializes_to_the_nested_shape() {
    let packages_json = fixture("state_legacy_flat.json")["packages"].clone();
    let packages: Vec<DownloadPackage> = serde_json::from_value(packages_json).unwrap();
    let mut written = serde_json::to_value(&packages).unwrap();

    // The legacy shape has no component id: Swift mints a fresh UUID on
    // decode. Check it is a well-formed uppercase UUID, then drop it.
    for item in written[0]["items"].as_array_mut().unwrap() {
        let component = &mut item["components"][0];
        let id = component["id"].as_str().unwrap().to_owned();
        assert_eq!(id, id.to_uppercase());
        assert!(uuid::Uuid::parse_str(&id).is_ok());
        component.as_object_mut().unwrap().remove("id");
    }

    let expected = json!([{
        "id": "00000000-0000-4000-8000-000000000020",
        "name": "Legacy",
        "position": 0,
        "priority": 2,
        "items": [
            {
                "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA427",
                "components": [{
                    "url": "https://x/f.bin",
                    "partFilename": "f.bin",
                    "totalBytes": 2048,
                    "completed": {"ranges": [{"start": 0, "end": 1024}]},
                    "validator": "\"legacy-etag\"",
                    "origin": {"kind": "http"},
                    "isResumable": true
                }],
                "outputFilename": "f.bin",
                "sourceURL": "https://x/f.bin",
                "assembly": "none",
                "state": {"stopped": {}},
                "isEnabled": true,
                "priority": 3,
                "position": 3
            },
            {
                "id": "1B4E28BA-2FA1-11D2-883F-0016D3CCA428",
                "components": [{
                    "url": "https://x/g.bin",
                    "partFilename": "g.bin",
                    "completed": {"ranges": []},
                    "origin": {"kind": "http"}
                }],
                "outputFilename": "g.bin",
                "sourceURL": "https://x/g.bin",
                "assembly": "none",
                "state": {"failed": {"reason": "boom"}},
                "isEnabled": false,
                "position": 0
            }
        ]
    }]);
    assert_eq!(written, expected);
}

#[test]
fn resolved_media_round_trips() {
    let value = fixture("resolved_media.json");
    let decoded: ResolvedMedia = serde_json::from_value(value.clone()).unwrap();
    assert!(decoded.source_url.is_some());
    assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
}

#[test]
fn media_formats_round_trip() {
    let value = fixture("media_formats.json");
    let decoded: Vec<MediaFormat> = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.len(), 8);
    assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
}

#[test]
fn format_choices_round_trip() {
    let value = fixture("format_choices.json");
    let decoded: Vec<FormatChoice> = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded.len(), 2);
    assert!(!decoded[0].is_wholesale());
    assert!(decoded[1].is_wholesale());
    assert_eq!(serde_json::to_value(&decoded).unwrap(), value);
}

fn sorted_array(v: &Value) -> Vec<String> {
    let mut items: Vec<String> = v
        .as_array()
        .unwrap()
        .iter()
        .map(|e| e.to_string())
        .collect();
    items.sort();
    items
}

#[test]
fn quality_preferences_round_trip_compares_sets_unordered() {
    let value = fixture("quality_preferences.json");
    let decoded: QualityPreferences = serde_json::from_value(value.clone()).unwrap();
    assert_eq!(decoded, QualityPreferences::default());
    let written = serde_json::to_value(&decoded).unwrap();
    assert_eq!(written["maxHeight"], value["maxHeight"]);
    for key in ["videoCodecs", "containers", "audioCodecs"] {
        // Swift `Set`s encode in arbitrary order; compare as sets.
        assert_eq!(sorted_array(&written[key]), sorted_array(&value[key]));
        let unique: HashSet<String> = sorted_array(&written[key]).into_iter().collect();
        assert_eq!(unique.len(), written[key].as_array().unwrap().len());
    }
}

#[test]
fn source_url_preserves_the_original_string() {
    for s in [
        "https://EXAMPLE.com:443/a%20b/ü?x=&y=1#frag",
        "http://host./path",
        "https://h/a b",
        "https://h/?",
    ] {
        let url = SourceUrl::new(s).unwrap();
        assert_eq!(url.as_str(), s);
        assert_eq!(serde_json::to_value(&url).unwrap(), Value::String(s.into()));
        let back: SourceUrl = serde_json::from_value(Value::String(s.into())).unwrap();
        assert_eq!(back, url);
        assert_eq!(url, url.clone());

        // Differs from the `url` crate's normalized form wherever that
        // form is a different string.
        let normalized = Url::parse(s).unwrap();
        assert_eq!(url.as_url(), Some(&normalized));
        if normalized.as_str() != s {
            assert_ne!(url, SourceUrl::from_url(&normalized));
        }
    }
    // At least the first and third really are altered by normalization.
    assert_ne!(
        Url::parse("https://EXAMPLE.com:443/a%20b/ü?x=&y=1#frag")
            .unwrap()
            .as_str(),
        "https://EXAMPLE.com:443/a%20b/ü?x=&y=1#frag"
    );
    assert_ne!(
        Url::parse("https://h/a b").unwrap().as_str(),
        "https://h/a b"
    );
}

#[test]
fn source_url_hashes_by_original_string() {
    let a = SourceUrl::new("https://EXAMPLE.com:443/x").unwrap();
    let b = SourceUrl::new("https://example.com/x").unwrap();
    assert_ne!(a, b);
    let set: HashSet<SourceUrl> = [a.clone(), b, a.clone()].into_iter().collect();
    assert_eq!(set.len(), 2);
    assert!(set.contains(&a));
}

#[test]
fn source_url_rejects_empty_and_unparseable_construction_without_panicking() {
    assert!(SourceUrl::new("").is_err());
    assert!(SourceUrl::new("not a url").is_err());
    assert!(SourceUrl::new("a/b").is_err());
}

#[test]
fn source_url_deserialization_keeps_odd_strings_like_swift() {
    // Foundation's `URL` accepts a relative reference, so Swift's decoder
    // loads it; one odd URL must not fail the whole file.
    let odd: SourceUrl = serde_json::from_value(json!("a/b")).unwrap();
    assert_eq!(odd.as_str(), "a/b");
    assert_eq!(odd.as_url(), None);
    assert_eq!(serde_json::to_value(&odd).unwrap(), json!("a/b"));
    // An empty string is where Swift's `URL` decoder throws.
    assert!(serde_json::from_value::<SourceUrl>(json!("")).is_err());
    assert!(serde_json::from_value::<SourceUrl>(json!(5)).is_err());
}

#[test]
fn every_url_in_state_rich_survives_a_round_trip_byte_for_byte() {
    let fixture = fixture("state_rich.json");
    let mut strings: Vec<String> = Vec::new();
    for package in fixture["packages"].as_array().unwrap() {
        for item in package["items"].as_array().unwrap() {
            strings.push(item["sourceURL"].as_str().unwrap().into());
            for c in item["components"].as_array().unwrap() {
                strings.push(c["url"].as_str().unwrap().into());
            }
        }
    }
    assert_eq!(strings.len(), 21);
    for s in &strings {
        let url: SourceUrl = serde_json::from_value(Value::String(s.clone())).unwrap();
        let out = serde_json::to_string(&url).unwrap();
        assert_eq!(out, serde_json::to_string(s).unwrap());
    }
    // And through the item model.
    let packages: Vec<DownloadPackage> =
        serde_json::from_value(fixture["packages"].clone()).unwrap();
    let decoded: Vec<&DownloadItem> = packages.iter().flat_map(|p| &p.items).collect();
    for item in decoded {
        let again: DownloadItem =
            serde_json::from_value(serde_json::to_value(item).unwrap()).unwrap();
        assert_eq!(item.source_url.as_str(), again.source_url.as_str());
    }
    // The odd ones are present as stored (including Foundation's %2520).
    for odd in [
        "https://EXAMPLE.com:443/a%2520b/%C3%BC?x=&y=1#frag",
        "https://EXAMPLE.com:443/a%20b/c?x=&y=1#frag",
        "https://example.com./p?",
    ] {
        assert!(strings.iter().any(|s| s == odd), "{odd}");
    }
}
