const DEFAULTS = {
  captureDownloads: true,
};

const captureDownloads = document.getElementById("captureDownloads");
async function loadSettings() {
  const settings = await chrome.storage.local.get(DEFAULTS);

  captureDownloads.checked = settings.captureDownloads;
}

captureDownloads.addEventListener("change", async () => {
  await chrome.storage.local.set({
    captureDownloads: captureDownloads.checked,
  });
});

loadSettings();
