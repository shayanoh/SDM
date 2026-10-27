// The version this code reports to SDM. Chrome keeps running the old script
// after an update until the extension is reloaded, while
// `chrome.runtime.getManifest()` already returns the new manifest, so the
// manifest cannot say which code is actually running.
//
// KEEP IN SYNC with "version" in manifest.json (`version.test.js` fails when
// they differ). Bump both together.
export const EXTENSION_VERSION = "2.0.2";
