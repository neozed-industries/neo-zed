(function () {
  "use strict";

  let active = false;
  let highlightedElement;
  let toastElement;

  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (!message || message.type !== "START_ANNOTATION_PICKER") {
      return false;
    }

    startPicker();
    sendResponse({ ok: true });
    return false;
  });

  function startPicker() {
    if (active) {
      showToast("Element picker is already active. Click a page element or press Escape.");
      return;
    }

    active = true;
    document.addEventListener("mouseover", handleMouseOver, true);
    document.addEventListener("mouseout", handleMouseOut, true);
    document.addEventListener("click", handleClick, true);
    document.addEventListener("keydown", handleKeyDown, true);
    showToast("Click an element to annotate. Press Escape to cancel.");
  }

  function stopPicker() {
    active = false;
    clearHighlight();
    document.removeEventListener("mouseover", handleMouseOver, true);
    document.removeEventListener("mouseout", handleMouseOut, true);
    document.removeEventListener("click", handleClick, true);
    document.removeEventListener("keydown", handleKeyDown, true);
  }

  function handleMouseOver(event) {
    if (!active || event.target === toastElement) {
      return;
    }

    clearHighlight();
    highlightedElement = event.target;
    highlightedElement.classList.add("zed-browser-annotation-highlight");
  }

  function handleMouseOut(event) {
    if (!active || event.target !== highlightedElement) {
      return;
    }

    clearHighlight();
  }

  function handleClick(event) {
    if (!active || event.target === toastElement) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();

    const target = event.target;
    const selectedText = window.getSelection().toString();
    const annotation = {
      url: window.location.href,
      title: document.title,
      selected_text: selectedText || target.innerText || target.textContent || undefined,
      selector: ZedBrowserAnnotationHelpers.selectorForElement(target),
    };

    stopPicker();
    showToast("Annotation target selected. Add a comment in the extension popup.");

    chrome.runtime.sendMessage({ type: "ANNOTATION_PICKED", annotation }, (response) => {
      if (chrome.runtime.lastError) {
        showToast(chrome.runtime.lastError.message);
        return;
      }

      if (!response || !response.ok) {
        showToast((response && response.error) || "Failed to store annotation target.");
      }
    });
  }

  function handleKeyDown(event) {
    if (!active || event.key !== "Escape") {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    stopPicker();
    showToast("Element picker canceled.");
  }

  function clearHighlight() {
    if (highlightedElement) {
      highlightedElement.classList.remove("zed-browser-annotation-highlight");
      highlightedElement = undefined;
    }
  }

  function showToast(message) {
    if (!toastElement) {
      toastElement = document.createElement("div");
      toastElement.className = "zed-browser-annotation-toast";
      document.documentElement.appendChild(toastElement);
    }

    toastElement.textContent = message;
    window.clearTimeout(showToast.timeoutId);
    showToast.timeoutId = window.setTimeout(() => {
      if (toastElement) {
        toastElement.remove();
        toastElement = undefined;
      }
    }, 3500);
  }
})();
