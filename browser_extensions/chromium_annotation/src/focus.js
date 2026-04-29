(async function () {
  "use strict";

  const params = new URLSearchParams(window.location.search);
  const tabId = parseIntegerParam(params.get("tabId"));
  const windowId = parseIntegerParam(params.get("windowId"));
  const fallbackUrl = params.get("url");
  const status = document.getElementById("status");

  try {
    const focused = await focusOriginalTab(tabId, windowId, fallbackUrl);
    if (!focused) {
      throw new Error("The annotated tab is no longer available.");
    }

    status.textContent = "Opened annotated tab.";
    window.setTimeout(() => window.close(), 150);
  } catch (error) {
    status.textContent = error.message;
  }

  async function focusOriginalTab(tabId, windowId, fallbackUrl) {
    if (typeof tabId === "number") {
      try {
        if (typeof windowId === "number") {
          await chrome.windows.update(windowId, { focused: true });
        }

        await chrome.tabs.update(tabId, { active: true });
        return true;
      } catch (_error) {
      }
    }

    if (!fallbackUrl) {
      return false;
    }

    const tabs = await chrome.tabs.query({});
    const tab =
      tabs.find((candidate) => candidate.url === fallbackUrl) ||
      tabs.find((candidate) => stripHash(candidate.url) === stripHash(fallbackUrl));

    if (!tab) {
      await chrome.tabs.create({ url: fallbackUrl });
      return true;
    }

    if (typeof tab.windowId === "number") {
      await chrome.windows.update(tab.windowId, { focused: true });
    }
    await chrome.tabs.update(tab.id, { active: true });
    return true;
  }

  function parseIntegerParam(value) {
    if (!value) {
      return undefined;
    }

    const parsed = Number.parseInt(value, 10);
    return Number.isFinite(parsed) ? parsed : undefined;
  }

  function stripHash(url) {
    try {
      const parsed = new URL(url);
      parsed.hash = "";
      return parsed.toString();
    } catch (_error) {
      return url;
    }
  }
})();
