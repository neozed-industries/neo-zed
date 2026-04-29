(function () {
  "use strict";

  let commentMode = false;
  let highlightedElement;
  let toastElement;
  let focusPollIntervalId;
  let focusPollInFlight = false;
  let theme;
  const markers = new Map();

  chrome.runtime.onMessage.addListener((message, _sender, sendResponse) => {
    if (!message || typeof message.type !== "string") {
      return false;
    }

    if (message.type === "PING_CONTENT_SCRIPT") {
      sendResponse({ ok: true });
      return false;
    }

    if (message.type === "SET_COMMENT_MODE") {
      applyTheme(message.theme);
      setCommentMode(Boolean(message.enabled), message.comments || []);
      sendResponse({ ok: true });
      return false;
    }

    if (message.type === "APPLY_THEME") {
      applyTheme(message.theme);
      sendResponse({ ok: true });
      return false;
    }

    if (message.type === "REMOVE_COMMENT_MARKER") {
      removeCommentMarker(message.comment_id);
      sendResponse({ ok: true });
      return false;
    }

    if (message.type === "CLEAR_COMMENT_MARKERS") {
      clearCommentMarkers();
      sendResponse({ ok: true });
      return false;
    }

    return false;
  });

  window.addEventListener("zed-browser-annotation-focus-comment", handleFocusCommentEvent);

  function handleFocusCommentEvent(event) {
    const commentId = typeof event.detail === "string" ? event.detail : undefined;
    if (!commentId) {
      return;
    }

    const marker = markers.get(commentId);
    if (!marker) {
      showToast("Comment is not visible on this page.");
      return;
    }

    focusCommentMarker(marker);
  }

  function applyTheme(nextTheme) {
    theme = ZedBrowserAnnotationHelpers.applyTheme(nextTheme, document.documentElement) || theme;
  }

  function setCommentMode(enabled, comments) {
    commentMode = enabled;

    if (commentMode) {
      document.addEventListener("mouseover", handleMouseOver, true);
      document.addEventListener("mouseout", handleMouseOut, true);
      document.addEventListener("click", handleClick, true);
      document.addEventListener("keydown", handleKeyDown, true);
      showToast("Comment mode on. Select text if needed, then click elements to comment.");
    } else {
      clearHighlight();
      document.removeEventListener("mouseover", handleMouseOver, true);
      document.removeEventListener("mouseout", handleMouseOut, true);
      document.removeEventListener("click", handleClick, true);
      document.removeEventListener("keydown", handleKeyDown, true);
      showToast("Comment mode off.");
    }

    renderExistingComments(comments);
  }

  function handleMouseOver(event) {
    if (!commentMode || isExtensionOverlayElement(event.target)) {
      return;
    }

    clearHighlight();
    highlightedElement = event.target;
    highlightedElement.classList.add("zed-browser-annotation-highlight");
  }

  function handleMouseOut(event) {
    if (!commentMode || event.target !== highlightedElement) {
      return;
    }

    clearHighlight();
  }

  function handleClick(event) {
    if (!commentMode || isExtensionOverlayElement(event.target)) {
      return;
    }

    event.preventDefault();
    event.stopPropagation();

    if (collapseExpandedMarkers()) {
      return;
    }

    const target = event.target;
    const selectedText = window.getSelection().toString();
    const annotation = {
      url: window.location.href,
      title: document.title,
      selected_text: selectedText || undefined,
      selector: ZedBrowserAnnotationHelpers.selectorForElement(target),
    };

    chrome.runtime.sendMessage({ type: "ADD_COMMENT", annotation }, (response) => {
      if (chrome.runtime.lastError) {
        showToast(chrome.runtime.lastError.message);
        return;
      }

      if (!response || !response.ok) {
        showToast((response && response.error) || "Failed to create comment.");
        return;
      }

      const comments = (response.session && response.session.comments) || [];
      const comment = comments[comments.length - 1];
      if (comment) {
        showCommentMarker(comment, target, true);
      }
    });
  }

  function handleKeyDown(event) {
    if (!commentMode || event.key !== "Escape") {
      return;
    }

    event.preventDefault();
    event.stopPropagation();
    chrome.runtime.sendMessage({ type: "SET_COMMENT_MODE", enabled: false }, (response) => {
      if (chrome.runtime.lastError) {
        showToast(chrome.runtime.lastError.message);
        return;
      }

      if (!response || !response.ok) {
        showToast((response && response.error) || "Failed to turn comment mode off.");
      }
    });
  }

  function clearHighlight() {
    if (highlightedElement) {
      highlightedElement.classList.remove("zed-browser-annotation-highlight");
      highlightedElement = undefined;
    }
  }

  function renderExistingComments(comments) {
    for (const comment of comments || []) {
      if (!comment || !comment.id) {
        continue;
      }

      const marker = markers.get(comment.id);
      if (marker) {
        marker.comment = comment;
        renderCommentBadge(marker, false);
        positionCommentBadge(marker);
        continue;
      }

      const element = elementForAnnotation(comment.annotation);
      if (element) {
        showCommentMarker(comment, element, false);
      }
    }
  }

  function showCommentMarker(comment, element, focusComment) {
    if (!comment || !comment.id || !element || element.nodeType !== 1) {
      return;
    }

    applyTheme(theme);
    removeCommentMarker(comment.id);

    element.classList.add("zed-browser-annotation-target");

    const badge = document.createElement("div");
    badge.className = "zed-browser-annotation-badge";
    document.documentElement.appendChild(badge);

    const marker = { comment, element, badge, expanded: Boolean(focusComment) };
    markers.set(comment.id, marker);

    badge.addEventListener("click", (event) => {
      const targetElement = event.target instanceof Element ? event.target : undefined;
      if (
        targetElement &&
        (targetElement.closest(".zed-browser-annotation-remove") || targetElement.closest("textarea"))
      ) {
        return;
      }

      event.preventDefault();
      event.stopPropagation();
      setMarkerExpanded(marker, !marker.expanded, true);
    });

    renderCommentBadge(marker, focusComment);
    positionCommentBadgeOnNextFrame(marker);
    startFocusPolling();
    window.addEventListener("scroll", positionAllBadges, true);
    window.addEventListener("resize", positionAllBadges, true);
  }

  function renderCommentBadge(marker, focusComment) {
    const annotation = marker.comment.annotation || {};
    const target =
      annotation.selected_text ||
      annotation.selector ||
      annotation.title ||
      annotation.url ||
      "Selected element";

    marker.badge.textContent = "";
    marker.badge.classList.toggle("is-expanded", marker.expanded);
    marker.badge.classList.toggle("is-collapsed", !marker.expanded);

    const headerElement = document.createElement("div");
    headerElement.className = "zed-browser-annotation-badge-header";

    const labelElement = document.createElement("button");
    labelElement.type = "button";
    labelElement.className = "zed-browser-annotation-badge-label";
    labelElement.textContent = `Draft comment ${Array.from(markers.keys()).indexOf(marker.comment.id) + 1}`;
    labelElement.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      setMarkerExpanded(marker, !marker.expanded, true);
    });

    const removeButton = document.createElement("button");
    removeButton.type = "button";
    removeButton.className = "zed-browser-annotation-remove";
    removeButton.textContent = "x";
    removeButton.title = "Remove comment";
    removeButton.setAttribute("aria-label", "Remove comment");
    removeButton.addEventListener("click", (event) => {
      event.preventDefault();
      event.stopPropagation();
      chrome.runtime.sendMessage({
        type: "REMOVE_COMMENT",
        comment_id: marker.comment.id,
      });
      removeCommentMarker(marker.comment.id);
    });

    headerElement.append(labelElement, removeButton);

    const detailElement = document.createElement("div");
    detailElement.className = "zed-browser-annotation-badge-detail";
    detailElement.textContent = marker.expanded ? target : annotation.comment || target;

    marker.badge.append(headerElement, detailElement);

    if (!marker.expanded) {
      return;
    }

    collapseOtherMarkers(marker);
    const commentElement = document.createElement("textarea");
    commentElement.className = "zed-browser-annotation-comment";
    commentElement.rows = 3;
    commentElement.placeholder = "Comment for Zed";
    commentElement.value = annotation.comment || "";
    commentElement.addEventListener("input", () => {
      marker.comment = {
        ...marker.comment,
        annotation: {
          ...annotation,
          comment: commentElement.value,
        },
      };
      chrome.runtime.sendMessage({
        type: "UPDATE_COMMENT",
        comment_id: marker.comment.id,
        annotation: marker.comment.annotation,
      });
    });

    marker.badge.append(commentElement);

    if (focusComment) {
      commentElement.focus();
    }
  }

  function setMarkerExpanded(marker, expanded, focusComment) {
    marker.expanded = expanded;
    renderCommentBadge(marker, focusComment);
    positionAllBadges();
  }

  function focusCommentMarker(marker) {
    marker.element.scrollIntoView({ block: "center", inline: "nearest", behavior: "smooth" });
    setMarkerExpanded(marker, true, true);
  }

  function collapseOtherMarkers(activeMarker) {
    for (const marker of markers.values()) {
      if (marker === activeMarker || !marker.expanded) {
        continue;
      }

      marker.expanded = false;
      renderCommentBadge(marker, false);
      positionCommentBadge(marker);
    }
  }

  function collapseExpandedMarkers() {
    let collapsed = false;
    for (const marker of markers.values()) {
      if (!marker.expanded) {
        continue;
      }

      marker.expanded = false;
      renderCommentBadge(marker, false);
      positionCommentBadge(marker);
      collapsed = true;
    }

    return collapsed;
  }

  function removeCommentMarker(commentId) {
    const marker = markers.get(commentId);
    if (!marker) {
      return;
    }

    marker.badge.remove();
    markers.delete(commentId);

    if (!hasMarkerForElement(marker.element)) {
      marker.element.classList.remove("zed-browser-annotation-target");
    }

    if (markers.size === 0) {
      stopFocusPolling();
      window.removeEventListener("scroll", positionAllBadges, true);
      window.removeEventListener("resize", positionAllBadges, true);
    }
  }

  function clearCommentMarkers() {
    for (const commentId of Array.from(markers.keys())) {
      removeCommentMarker(commentId);
    }
  }

  function hasMarkerForElement(element) {
    for (const marker of markers.values()) {
      if (marker.element === element) {
        return true;
      }
    }

    return false;
  }

  function startFocusPolling() {
    if (focusPollIntervalId) {
      return;
    }

    focusPollIntervalId = window.setInterval(pollFocusRequest, 700);
    pollFocusRequest();
  }

  function stopFocusPolling() {
    if (!focusPollIntervalId) {
      return;
    }

    window.clearInterval(focusPollIntervalId);
    focusPollIntervalId = undefined;
    focusPollInFlight = false;
  }

  function pollFocusRequest() {
    if (focusPollInFlight || markers.size === 0) {
      return;
    }

    focusPollInFlight = true;
    chrome.runtime.sendMessage({ type: "POLL_FOCUS_REQUEST" }, (response) => {
      focusPollInFlight = false;
      if (chrome.runtime.lastError || !response || !response.ok || !response.request) {
        return;
      }

      const marker = markers.get(response.request.id);
      if (!marker) {
        return;
      }

      focusCommentMarker(marker);
      chrome.runtime.sendMessage({
        type: "ACK_FOCUS_REQUEST",
        comment_id: response.request.id,
      });
    });
  }

  function positionAllBadges() {
    for (const marker of markers.values()) {
      positionCommentBadge(marker);
    }
  }

  function positionCommentBadgeOnNextFrame(marker) {
    window.requestAnimationFrame(() => {
      if (markers.get(marker.comment.id) === marker) {
        positionCommentBadge(marker);
      }
    });
  }

  function positionCommentBadge(marker) {
    const rect = marker.element.getBoundingClientRect();
    const margin = 8;
    const left = Math.min(
      Math.max(rect.left, margin),
      window.innerWidth - marker.badge.offsetWidth - margin,
    );
    const topCandidate = rect.bottom + margin;
    const top =
      topCandidate + marker.badge.offsetHeight + margin <= window.innerHeight
        ? topCandidate
        : Math.max(margin, rect.top - marker.badge.offsetHeight - margin);

    marker.badge.style.left = `${left}px`;
    marker.badge.style.top = `${top}px`;
  }

  function elementForAnnotation(annotation) {
    if (!annotation || !annotation.selector) {
      return undefined;
    }

    try {
      return document.querySelector(annotation.selector);
    } catch (_error) {
      return undefined;
    }
  }

  function isExtensionOverlayElement(element) {
    if (element === toastElement) {
      return true;
    }

    for (const marker of markers.values()) {
      if (marker.badge === element || marker.badge.contains(element)) {
        return true;
      }
    }

    return false;
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
