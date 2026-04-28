importScripts("shared_core.js");

const NATIVE_HOST_NAME = "browser_annotation_host";
const STATE_KEY = "currentAnnotation";

let nextRequestId = 1;

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || typeof message.type !== "string") {
    return false;
  }

  if (message.type === "ANNOTATION_PICKED") {
    persistAnnotation(message.annotation, sender.tab)
      .then((annotation) => sendResponse({ ok: true, annotation }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "GET_CURRENT_ANNOTATION") {
    getCurrentAnnotation()
      .then((annotation) => sendResponse({ ok: true, annotation }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "START_PICKER") {
    startPicker()
      .then((result) => sendResponse({ ok: true, result }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "SEND_ANNOTATION") {
    sendAnnotation(message.annotation)
      .then((result) => sendResponse({ ok: true, result }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "PING_HOST") {
    pingNativeHost()
      .then((result) => sendResponse({ ok: true, result }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  return false;
});

async function startPicker() {
  const tab = await getActiveTab();
  if (!tab || !tab.id) {
    throw new Error("No active tab is available.");
  }

  await chrome.tabs.sendMessage(tab.id, { type: "START_ANNOTATION_PICKER" });
  return { tabId: tab.id };
}

async function persistAnnotation(annotation, tab) {
  const payload = ZedBrowserAnnotationHelpers.buildAnnotationPayload({
    url: annotation && annotation.url,
    title: annotation && annotation.title,
    selected_text: annotation && annotation.selected_text,
    selector: annotation && annotation.selector,
    comment: annotation && annotation.comment,
  });

  if (!payload.url && tab && tab.url) {
    payload.url = tab.url;
  }

  if (!payload.title && tab && tab.title) {
    payload.title = tab.title;
  }

  await chrome.storage.session.set({ [STATE_KEY]: payload });
  return payload;
}

async function getCurrentAnnotation() {
  const stored = await chrome.storage.session.get(STATE_KEY);
  if (stored[STATE_KEY]) {
    return stored[STATE_KEY];
  }

  const tab = await getActiveTab();
  return ZedBrowserAnnotationHelpers.buildAnnotationPayload({
    url: tab && tab.url,
    title: tab && tab.title,
  });
}

async function sendAnnotation(annotation) {
  const currentAnnotation = await getCurrentAnnotation();
  const payload = ZedBrowserAnnotationHelpers.buildAnnotationPayload({
    ...currentAnnotation,
    ...annotation,
  });

  if (!payload.url) {
    throw new Error("Annotation URL is required.");
  }

  const request = ZedBrowserAnnotationHelpers.buildInsertRequest(payload, nextRequestId++);
  const response = await chrome.runtime.sendNativeMessage(NATIVE_HOST_NAME, request);

  if (!response) {
    throw new Error("Native host returned no response.");
  }

  if (response.error) {
    throw new Error(response.error.message || "Native host rejected the annotation.");
  }

  await chrome.storage.session.remove(STATE_KEY);
  return response.result || { ok: true };
}

async function pingNativeHost() {
  const response = await chrome.runtime.sendNativeMessage(NATIVE_HOST_NAME, {
    jsonrpc: "2.0",
    id: nextRequestId++,
    method: "browserAnnotation.ping",
  });

  if (!response) {
    throw new Error("Native host returned no response.");
  }

  if (response.error) {
    throw new Error(response.error.message || "Native host ping failed.");
  }

  return response.result || { ok: true };
}

async function getActiveTab() {
  const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
  return tabs[0];
}
