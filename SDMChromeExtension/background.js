import {
  DEBOUNCE_MINUTES,
  cookiesToSend,
  dedupeCookies,
  domainMatches,
  pickSiteList,
} from "./cookies.js";

const NATIVE_HOST = "com.shayanoh.sdm.chrome_extension";
const DUPLICATE_MS = 60000;
const STATUS_ALARM = "sdm-status";
const STATUS_INTERVAL_MINUTES = 60;
const SYNC_ALARM = "sdm-cookie-sync";

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

// MARK: - Cookie sync

// Which sites' cookies SDM needs. The host owns the list (so a new site in a
// later SDM release needs no extension update); the last one it sent is kept
// in storage, and a copy ships in the extension for the very first run.

async function bundledSiteList() {
  const response = await fetch(chrome.runtime.getURL("cookie-domains.json"));
  return await response.json();
}

async function storedSiteList() {
  const { siteList } = await chrome.storage.local.get("siteList");
  return siteList;
}

async function currentSiteList() {
  return pickSiteList(
    undefined,
    await storedSiteList(),
    await bundledSiteList(),
  );
}

async function rememberSiteList(list) {
  await chrome.storage.local.set({ siteList: list });
}

async function askHostForSiteList() {
  try {
    const reply = await chrome.runtime.sendNativeMessage(NATIVE_HOST, {
      type: "siteList",
    });
    if (reply?.success === true) {
      return { version: reply.version, domains: reply.domains };
    }
  } catch (error) {
    console.error("Failed to get the site list from SDM:", error);
  }
  return undefined;
}

let syncing = false;
let syncAgain = false;

// Sends a full snapshot of the listed sites' cookies. Never logs a cookie.
async function syncCookies() {
  if (syncing) {
    syncAgain = true;
    return;
  }
  syncing = true;
  try {
    do {
      syncAgain = false;
      const received = await askHostForSiteList();
      if (received) {
        await rememberSiteList(received);
      }
      const list = pickSiteList(
        received,
        await storedSiteList(),
        await bundledSiteList(),
      );
      const groups = await Promise.all(
        list.domains.map((domain) => chrome.cookies.getAll({ domain })),
      );
      const cookies = cookiesToSend(dedupeCookies(groups.flat()), list.domains);
      const reply = await chrome.runtime.sendNativeMessage(NATIVE_HOST, {
        type: "cookies",
        siteListVersion: list.version,
        cookies,
      });
      if (reply?.success === true && Array.isArray(reply.domains)) {
        await rememberSiteList({
          version: reply.siteListVersion,
          domains: reply.domains,
        });
        syncAgain = true;
      }
    } while (syncAgain);
  } catch (error) {
    console.error("Failed to send cookies to SDM:", error);
  } finally {
    syncing = false;
  }
}

// The service worker sleeps, so the debounce is an alarm, not a timer.
async function scheduleSync() {
  if (!(await chrome.alarms.get(SYNC_ALARM))) {
    await chrome.alarms.create(SYNC_ALARM, {
      delayInMinutes: DEBOUNCE_MINUTES,
    });
  }
}

chrome.cookies.onChanged.addListener(async (change) => {
  // Changes to other sites' cookies are ignored: not stored, not logged.
  const list = await currentSiteList();
  if (domainMatches(change.cookie.domain, list.domains)) {
    await scheduleSync();
  }
});

chrome.alarms.onAlarm.addListener(async (alarm) => {
  if (alarm.name === SYNC_ALARM) {
    await syncCookies();
  } else if (alarm.name === STATUS_ALARM) {
    await reportStatus();
    await syncCookies();
  }
});

async function startUp() {
  await reportStatus();
  await ensureStatusAlarm();
  await syncCookies();
}

chrome.runtime.onInstalled.addListener(startUp);

chrome.runtime.onStartup.addListener(startUp);
