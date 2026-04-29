importScripts("shared_core.js");

const NATIVE_HOST_NAME = "browser_annotation_host";
const STATE_KEY = "annotationSession";

let nextRequestId = 1;

chrome.runtime.onMessage.addListener((message, sender, sendResponse) => {
  if (!message || typeof message.type !== "string") {
    return false;
  }

  if (message.type === "GET_ANNOTATION_SESSION") {
    getAnnotationSession({ refreshTheme: true })
      .then((session) => sendResponse({ ok: true, session }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "SET_COMMENT_MODE") {
    setCommentMode(Boolean(message.enabled))
      .then((session) => sendResponse({ ok: true, session }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "ADD_COMMENT") {
    addComment(message.annotation, sender.tab)
      .then((session) => sendResponse({ ok: true, session }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "UPDATE_COMMENT") {
    updateComment(message.comment_id, message.annotation)
      .then((session) => sendResponse({ ok: true, session }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "REMOVE_COMMENT") {
    removeComment(message.comment_id)
      .then((session) => sendResponse({ ok: true, session }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "CLEAR_COMMENTS") {
    clearComments()
      .then((session) => sendResponse({ ok: true, session }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "SEND_COMMENTS") {
    sendComments()
      .then((result) => sendResponse({ ok: true, result }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "POLL_FOCUS_REQUEST") {
    pollFocusRequest(message.url)
      .then((request) => sendResponse({ ok: true, request }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  if (message.type === "ACK_FOCUS_REQUEST") {
    ackFocusRequest(message.comment_id, message.focus_tab === false ? undefined : sender.tab)
      .then((result) => sendResponse({ ok: true, result }))
      .catch((error) => sendResponse({ ok: false, error: error.message }));
    return true;
  }

  return false;
});

async function getAnnotationSession(options = {}) {
  const stored = await chrome.storage.session.get(STATE_KEY);
  const tab = await getActiveTab();
  let session = normalizeSession(stored[STATE_KEY], tab);

  if (options.refreshTheme) {
    const theme = await getZedTheme().catch(() => session.theme);
    session = normalizeSession({ ...session, theme }, tab);
    await saveSession(session);
    await sendToSessionTab(session, { type: "APPLY_THEME", theme: session.theme });
  }

  return session;
}

async function setCommentMode(enabled) {
  const tab = await getActiveTab();
  if (enabled) {
    await ensureContentScript(tab);
  }

  const session = await getAnnotationSession({ refreshTheme: true });
  const nextSession = normalizeSession(
    {
      ...session,
      commentMode: enabled,
      tabId: tab && tab.id,
      windowId: tab && tab.windowId,
      pageUrl: tab && tab.url,
      pageTitle: tab && tab.title,
    },
    tab,
  );

  await saveSession(nextSession);
  await sendToSessionTab(nextSession, {
    type: "SET_COMMENT_MODE",
    enabled,
    comments: nextSession.comments,
    theme: nextSession.theme,
  });
  return nextSession;
}

async function addComment(annotation, tab) {
  const session = await getAnnotationSession();
  const targetTab = tab || (await getActiveTab());
  const payload = buildPayload(annotation, targetTab);
  const commentId = `comment-${Date.now()}-${Math.random().toString(16).slice(2)}`;
  payload.id = commentId;
  const comment = {
    id: commentId,
    annotation: payload,
  };
  const nextSession = normalizeSession(
    {
      ...session,
      commentMode: true,
      tabId: targetTab && targetTab.id,
      windowId: targetTab && targetTab.windowId,
      pageUrl: payload.url,
      pageTitle: payload.title,
      comments: [...session.comments, comment],
    },
    targetTab,
  );

  await saveSession(nextSession);
  await syncZedDraft(nextSession, false);
  return nextSession;
}

async function updateComment(commentId, annotation) {
  const session = await getAnnotationSession();
  const nextSession = {
    ...session,
    comments: session.comments.map((comment) => {
      if (comment.id !== commentId) {
        return comment;
      }

      return {
        ...comment,
        annotation: ZedBrowserAnnotationHelpers.buildAnnotationPayload({
          ...comment.annotation,
          ...annotation,
        }),
      };
    }),
  };

  await saveSession(nextSession);
  await syncZedDraft(nextSession, false);
  return nextSession;
}

async function removeComment(commentId) {
  const session = await getAnnotationSession();
  const nextSession = {
    ...session,
    comments: session.comments.filter((comment) => comment.id !== commentId),
  };

  await saveSession(nextSession);
  await sendToSessionTab(nextSession, {
    type: "REMOVE_COMMENT_MARKER",
    comment_id: commentId,
  });
  await syncZedDraft(nextSession, false);
  return nextSession;
}

async function clearComments() {
  const session = await getAnnotationSession();
  const nextSession = { ...session, comments: [] };
  await saveSession(nextSession);
  await sendToSessionTab(session, { type: "CLEAR_COMMENT_MARKERS" });
  await syncZedDraft(nextSession, false);
  return nextSession;
}

async function sendComments() {
  const session = await getAnnotationSession();
  const comments = session.comments.filter((comment) => comment.annotation && comment.annotation.url);
  if (comments.length === 0) {
    throw new Error("No comments to send.");
  }

  await syncZedDraft({ ...session, comments }, false);

  const nextSession = { ...session, comments: [] };
  await saveSession(nextSession);
  await sendToSessionTab(session, { type: "CLEAR_COMMENT_MARKERS" });
  return { sent: comments.length };
}

async function syncZedDraft(session, submit) {
  const annotations = (session.comments || [])
    .map((comment) => comment.annotation)
    .filter((annotation) => annotation && annotation.url);
  const request = ZedBrowserAnnotationHelpers.buildSyncRequest(annotations, submit, nextRequestId++);
  const response = await chrome.runtime.sendNativeMessage(NATIVE_HOST_NAME, request);

  if (!response) {
    throw new Error("Native host returned no response.");
  }

  if (response.error) {
    throw new Error(response.error.message || "Native host rejected the annotation sync.");
  }

  return response.result || { ok: true };
}

async function getZedTheme() {
  const response = await chrome.runtime.sendNativeMessage(
    NATIVE_HOST_NAME,
    ZedBrowserAnnotationHelpers.buildThemeRequest(nextRequestId++),
  );

  if (!response) {
    throw new Error("Native host returned no response.");
  }

  if (response.error) {
    throw new Error(response.error.message || "Native host rejected the theme request.");
  }

  const theme = ZedBrowserAnnotationHelpers.normalizeTheme(response.result && response.result.theme);
  if (!theme) {
    throw new Error("Native host returned no Zed theme.");
  }

  return theme;
}

async function pollFocusRequest(pageUrl) {
  const params = {};
  if (typeof pageUrl === "string") {
    params.url = pageUrl;
  }

  const response = await chrome.runtime.sendNativeMessage(NATIVE_HOST_NAME, {
    jsonrpc: "2.0",
    id: nextRequestId++,
    method: "browserAnnotation.pollFocus",
    params,
  });

  if (!response || response.error) {
    return undefined;
  }

  return response.result && response.result.request;
}

async function ackFocusRequest(commentId, tab) {
  if (!commentId) {
    throw new Error("Focus request id is required.");
  }

  await chrome.runtime.sendNativeMessage(NATIVE_HOST_NAME, {
    jsonrpc: "2.0",
    id: nextRequestId++,
    method: "browserAnnotation.ackFocus",
    params: { id: commentId },
  });

  if (tab && typeof tab.windowId === "number") {
    await chrome.windows.update(tab.windowId, { focused: true });
  }

  if (tab && typeof tab.id === "number") {
    await chrome.tabs.update(tab.id, { active: true });
  }

  return { ok: true };
}

async function ensureContentScript(tab) {
  if (!isScriptableTab(tab)) {
    throw new Error("Comment mode only works on regular http:// and https:// pages.");
  }

  try {
    await chrome.tabs.sendMessage(tab.id, { type: "PING_CONTENT_SCRIPT" });
    return;
  } catch (_error) {
  }

  await chrome.scripting.insertCSS({
    target: { tabId: tab.id },
    files: ["src/content_script.css"],
  });
  await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    files: ["src/shared_core.js"],
  });
  await chrome.scripting.executeScript({
    target: { tabId: tab.id },
    files: ["src/content_script.js"],
  });
}

async function sendToSessionTab(session, message) {
  if (!session || typeof session.tabId !== "number") {
    return;
  }

  try {
    await chrome.tabs.sendMessage(session.tabId, message);
  } catch (_error) {
  }
}

async function saveSession(session) {
  await chrome.storage.session.set({ [STATE_KEY]: session });
}

async function getActiveTab() {
  const tabs = await chrome.tabs.query({ active: true, currentWindow: true });
  return tabs[0];
}

function normalizeSession(value, tab) {
  const legacyComment = value && value.annotation ? [{ id: "legacy-comment", annotation: value.annotation }] : [];
  return {
    commentMode: Boolean(value && value.commentMode),
    comments: Array.isArray(value && value.comments) ? value.comments : legacyComment,
    tabId: typeof (value && value.tabId) === "number" ? value.tabId : tab && tab.id,
    windowId: typeof (value && value.windowId) === "number" ? value.windowId : tab && tab.windowId,
    pageUrl: (value && value.pageUrl) || (tab && tab.url) || "",
    pageTitle: (value && value.pageTitle) || (tab && tab.title),
    theme: ZedBrowserAnnotationHelpers.normalizeTheme(value && value.theme),
  };
}

function buildPayload(annotation, tab) {
  const payload = ZedBrowserAnnotationHelpers.buildAnnotationPayload({
    url: annotation && annotation.url,
    title: annotation && annotation.title,
    selected_text: annotation && annotation.selected_text,
    selector: annotation && annotation.selector,
    comment: annotation && annotation.comment,
    focus_url: annotation && annotation.focus_url,
  });

  if (!payload.url && tab && tab.url) {
    payload.url = tab.url;
  }

  if (!payload.title && tab && tab.title) {
    payload.title = tab.title;
  }

  if (!payload.focus_url) {
    payload.focus_url = focusUrlForTab(tab, payload.url);
  }

  return payload;
}

function focusUrlForTab(tab, fallbackUrl) {
  if (!tab || typeof tab.id !== "number") {
    return undefined;
  }

  const focusUrl = new URL(chrome.runtime.getURL("src/focus.html"));
  focusUrl.searchParams.set("tabId", String(tab.id));

  if (typeof tab.windowId === "number") {
    focusUrl.searchParams.set("windowId", String(tab.windowId));
  }

  if (fallbackUrl) {
    focusUrl.searchParams.set("url", fallbackUrl);
  }

  return focusUrl.toString();
}

function isScriptableTab(tab) {
  return Boolean(
    tab &&
      typeof tab.id === "number" &&
      typeof tab.url === "string" &&
      (tab.url.startsWith("http://") || tab.url.startsWith("https://")),
  );
}
