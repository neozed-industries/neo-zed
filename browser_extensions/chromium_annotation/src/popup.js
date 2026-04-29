(function () {
  "use strict";

  const pageTitle = document.getElementById("page-title");
  const commentMode = document.getElementById("comment-mode");
  const commentCount = document.getElementById("comment-count");
  const comments = document.getElementById("comments");
  const clear = document.getElementById("clear");
  const send = document.getElementById("send");
  const status = document.getElementById("status");

  let session = { commentMode: false, comments: [] };
  let busy = false;

  document.addEventListener("DOMContentLoaded", refreshSession);
  commentMode.addEventListener("click", setCommentMode);
  clear.addEventListener("click", clearComments);
  send.addEventListener("click", sendComments);

  async function refreshSession() {
    setStatus("Loading...", "pending");
    const response = await sendRuntimeMessage({ type: "GET_ANNOTATION_SESSION" });
    if (!response.ok) {
      setStatus(response.error, "error");
      return;
    }

    session = response.session || session;
    renderSession();
    setStatus(session.commentMode ? "Comment mode is on." : "Comment mode is off.", "success");
  }

  async function setCommentMode() {
    const enabled = commentMode.getAttribute("aria-checked") !== "true";
    setBusy(true);
    setStatus(enabled ? "Turning comment mode on..." : "Turning comment mode off...", "pending");

    const response = await sendRuntimeMessage({
      type: "SET_COMMENT_MODE",
      enabled,
    });
    setBusy(false);

    if (!response.ok) {
      setToggleState(Boolean(session.commentMode));
      setStatus(response.error, "error");
      return;
    }

    session = response.session || session;
    renderSession();
    setStatus(session.commentMode ? "Comment mode is on." : "Comment mode is off.", "success");
  }

  async function clearComments() {
    setBusy(true);
    setStatus("Clearing comments...", "pending");

    const response = await sendRuntimeMessage({ type: "CLEAR_COMMENTS" });
    setBusy(false);

    if (!response.ok) {
      setStatus(response.error, "error");
      return;
    }

    session = response.session || session;
    renderSession();
    setStatus("Cleared comments.", "success");
  }

  async function sendComments() {
    setBusy(true);
    setStatus("Sending comments to Zed...", "pending");

    const response = await sendRuntimeMessage({ type: "SEND_COMMENTS" });
    setBusy(false);

    if (!response.ok) {
      setStatus(response.error, "error");
      return;
    }

    session = { ...session, comments: [] };
    renderSession();
    setStatus(`Sent ${response.result.sent} comments to Zed for review.`, "success");
  }

  function renderSession() {
    ZedBrowserAnnotationHelpers.applyTheme(session.theme, document.documentElement);

    const commentTotal = session.comments.length;
    pageTitle.textContent = session.pageTitle || session.pageUrl || "Current tab";
    setToggleState(Boolean(session.commentMode));
    commentCount.textContent = commentTotal === 1 ? "1 comment" : `${commentTotal} comments`;
    clear.disabled = busy || commentTotal === 0;
    send.disabled = busy || commentTotal === 0;

    comments.textContent = "";
    if (commentTotal === 0) {
      const empty = document.createElement("p");
      empty.className = "empty";
      empty.textContent = "No draft comments yet.";
      comments.append(empty);
      return;
    }

    for (const comment of session.comments) {
      comments.append(renderComment(comment));
    }
  }

  function renderComment(comment) {
    const annotation = comment.annotation || {};
    const item = document.createElement("article");
    item.className = "comment";

    const text = document.createElement("p");
    text.className = "comment-text";
    text.textContent = annotation.comment || "No comment text";

    const target = document.createElement("p");
    target.className = "comment-target";
    target.textContent = annotation.selected_text || annotation.selector || annotation.url || "Selected element";

    const remove = document.createElement("button");
    remove.type = "button";
    remove.textContent = "Remove";
    remove.addEventListener("click", () => removeComment(comment.id));

    item.append(text, target, remove);
    return item;
  }

  async function removeComment(commentId) {
    setBusy(true);
    setStatus("Removing comment...", "pending");

    const response = await sendRuntimeMessage({
      type: "REMOVE_COMMENT",
      comment_id: commentId,
    });
    setBusy(false);

    if (!response.ok) {
      setStatus(response.error, "error");
      return;
    }

    session = response.session || session;
    renderSession();
    setStatus("Removed comment.", "success");
  }

  function setBusy(isBusy) {
    busy = isBusy;
    commentMode.disabled = busy;
    clear.disabled = busy || session.comments.length === 0;
    send.disabled = busy || session.comments.length === 0;
  }

  function setToggleState(enabled) {
    commentMode.setAttribute("aria-checked", enabled ? "true" : "false");
  }

  function setStatus(message, kind) {
    status.textContent = message;
    status.dataset.kind = kind;
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
})();
