(function () {
  "use strict";

  const pageTitle = document.getElementById("page-title");
  const selector = document.getElementById("selector");
  const selectedText = document.getElementById("selected-text");
  const comment = document.getElementById("comment");
  const status = document.getElementById("status");
  const pick = document.getElementById("pick");
  const send = document.getElementById("send");
  const ping = document.getElementById("ping");

  let currentAnnotation = {};

  document.addEventListener("DOMContentLoaded", refreshAnnotation);
  pick.addEventListener("click", startPicker);
  send.addEventListener("click", sendAnnotation);
  ping.addEventListener("click", pingHost);

  async function refreshAnnotation() {
    setStatus("Loading current tab...", "pending");
    const response = await sendRuntimeMessage({ type: "GET_CURRENT_ANNOTATION" });
    if (!response.ok) {
      setStatus(response.error, "error");
      return;
    }

    currentAnnotation = response.annotation || {};
    pageTitle.textContent = currentAnnotation.title || currentAnnotation.url || "Current tab";
    selector.textContent = currentAnnotation.selector || "None selected";
    selectedText.value = currentAnnotation.selected_text || "";
    setStatus("Ready.", "success");
  }

  async function startPicker() {
    setBusy(true);
    setStatus("Starting element picker...", "pending");

    const response = await sendRuntimeMessage({ type: "START_PICKER" });
    setBusy(false);

    if (!response.ok) {
      setStatus(response.error, "error");
      return;
    }

    setStatus("Picker active. Click a page element.", "success");
  }

  async function sendAnnotation() {
    setBusy(true);
    setStatus("Sending annotation to Zed...", "pending");

    const annotation = ZedBrowserAnnotationHelpers.buildAnnotationPayload({
      ...currentAnnotation,
      selected_text: selectedText.value,
      comment: comment.value,
    });

    const response = await sendRuntimeMessage({ type: "SEND_ANNOTATION", annotation });
    setBusy(false);

    if (!response.ok) {
      setStatus(response.error, "error");
      return;
    }

    currentAnnotation = {};
    selectedText.value = "";
    comment.value = "";
    selector.textContent = "None selected";
    setStatus("Sent to Zed.", "success");
  }

  async function pingHost() {
    setBusy(true);
    setStatus("Checking native host...", "pending");

    const response = await sendRuntimeMessage({ type: "PING_HOST" });
    setBusy(false);

    if (!response.ok) {
      setStatus(response.error, "error");
      return;
    }

    setStatus("Native host is reachable.", "success");
  }

  function sendRuntimeMessage(message) {
    return new Promise((resolve) => {
      chrome.runtime.sendMessage(message, (response) => {
        if (chrome.runtime.lastError) {
          resolve({ ok: false, error: chrome.runtime.lastError.message });
          return;
        }

        resolve(response || { ok: false, error: "Extension background did not respond." });
      });
    });
  }

  function setBusy(isBusy) {
    pick.disabled = isBusy;
    send.disabled = isBusy;
    ping.disabled = isBusy;
  }

  function setStatus(message, kind) {
    status.textContent = message;
    status.dataset.kind = kind;
  }
})();
