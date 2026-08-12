const NATIVE_HOST = "com.shayanoh.sdm.chrome_extension";
const DUPLICATE_MS = 60000;
const STATUS_ALARM = "sdm-status";
const STATUS_INTERVAL_MINUTES = 60;

let lastUrl = "";
let lastUrlTime = 0;

chrome.downloads.onCreated.addListener(async (download) => {
  console.log("Download detected:", download);

  const { captureDownloads = true } =
    await chrome.storage.local.get("captureDownloads");

  if (!captureDownloads) {
    console.log("Not capturing downloads");
    return;
  }

  // We only want normal HTTP(S) downloads.
  let finalUrl = download.finalUrl;
  if (!finalUrl) {
    finalUrl = download.url;
  }
  if (!finalUrl.startsWith("http://") && !finalUrl.startsWith("https://")) {
    return;
  }

  let now = Date.now();
  if (finalUrl == lastUrl) {
    if (now - lastUrlTime < DUPLICATE_MS) {
      console.log("Duplicate URL within " + DUPLICATE_MS + "ms, ignoring...");
      return;
    }
  }

  try {
    let response = await chrome.runtime.sendNativeMessage(NATIVE_HOST, {
      type: "download",
      url: finalUrl,
      referrer: download.referrer ?? null,
    });

    if (response?.success === true) {
      await chrome.downloads.cancel(download.id);
      lastUrl = finalUrl;
      lastUrlTime = Date.now();
      console.log("Sent to SDM and cancelled:", download.url);
    } else {
      console.error("Native host rejected download:", response);
    }
  } catch (error) {
    console.error("Failed to send download to SDM:", error);
  }
});

async function reportStatus() {
  try {
    const manifest = chrome.runtime.getManifest();

    await chrome.runtime.sendNativeMessage(NATIVE_HOST, {
      type: "extensionStatus",
      version: manifest.version,
    });
  } catch (error) {
    console.error("Failed to report extension status:", error);
  }
}

async function ensureStatusAlarm() {
  const alarm = await chrome.alarms.get(STATUS_ALARM);

  if (!alarm) {
    await chrome.alarms.create(STATUS_ALARM, {
      delayInMinutes: STATUS_INTERVAL_MINUTES,
      periodInMinutes: STATUS_INTERVAL_MINUTES,
    });
  }
}

chrome.runtime.onInstalled.addListener(async () => {
  await reportStatus();
  await ensureStatusAlarm();
});

chrome.runtime.onStartup.addListener(async () => {
  await reportStatus();
  await ensureStatusAlarm();
});
