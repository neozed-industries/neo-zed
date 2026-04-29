(function (root) {
  "use strict";

  const MAX_TEXT_LENGTH = 20000;
  const THEME_COLOR_VARIABLES = {
    background: "--zed-browser-annotation-background",
    panel_background: "--zed-browser-annotation-panel-background",
    elevated_surface_background: "--zed-browser-annotation-elevated-surface-background",
    editor_background: "--zed-browser-annotation-editor-background",
    element_background: "--zed-browser-annotation-element-background",
    element_hover: "--zed-browser-annotation-element-hover",
    element_active: "--zed-browser-annotation-element-active",
    element_selected: "--zed-browser-annotation-element-selected",
    border: "--zed-browser-annotation-border",
    border_variant: "--zed-browser-annotation-border-variant",
    border_focused: "--zed-browser-annotation-border-focused",
    text: "--zed-browser-annotation-text",
    text_muted: "--zed-browser-annotation-text-muted",
    text_disabled: "--zed-browser-annotation-text-disabled",
    text_accent: "--zed-browser-annotation-text-accent",
    icon: "--zed-browser-annotation-icon",
    icon_muted: "--zed-browser-annotation-icon-muted",
    success: "--zed-browser-annotation-success",
    error: "--zed-browser-annotation-error",
    warning: "--zed-browser-annotation-warning",
  };

  function normalizeOptionalString(value) {
    if (typeof value !== "string") {
      return undefined;
    }

    const trimmed = value.trim();
    return trimmed.length === 0 ? undefined : trimmed;
  }

  function limitText(value) {
    const normalized = normalizeOptionalString(value);
    if (!normalized) {
      return undefined;
    }

    return normalized.length > MAX_TEXT_LENGTH
      ? normalized.slice(0, MAX_TEXT_LENGTH)
      : normalized;
  }

  function cssEscape(value) {
    if (root.CSS && typeof root.CSS.escape === "function") {
      return root.CSS.escape(value);
    }

    return String(value).replace(/[^a-zA-Z0-9_-]/g, function (character) {
      return "\\" + character;
    });
  }

  function selectorForElement(element) {
    if (!element || element.nodeType !== 1) {
      return undefined;
    }

    const parts = [];
    let current = element;

    while (current && current.nodeType === 1 && current !== current.ownerDocument.documentElement) {
      let part = current.localName;
      if (!part) {
        break;
      }

      part = part.toLowerCase();

      if (current.id) {
        parts.unshift(part + "#" + cssEscape(current.id));
        break;
      }

      const classNames = Array.from(current.classList || [])
        .filter(Boolean)
        .slice(0, 3)
        .map(function (className) {
          return "." + cssEscape(className);
        })
        .join("");

      part += classNames;

      const parent = current.parentElement;
      if (parent) {
        const sameTagSiblings = Array.from(parent.children).filter(function (sibling) {
          return sibling.localName === current.localName;
        });

        if (sameTagSiblings.length > 1) {
          part += ":nth-of-type(" + (sameTagSiblings.indexOf(current) + 1) + ")";
        }
      }

      parts.unshift(part);
      current = parent;
    }

    return parts.length === 0 ? undefined : parts.join(" > ");
  }

  function buildAnnotationPayload(input) {
    const payload = {
      id: normalizeOptionalString(input && input.id),
      url: normalizeOptionalString(input && input.url) || "",
      title: normalizeOptionalString(input && input.title),
      selected_text: limitText(input && input.selected_text),
      selector: normalizeOptionalString(input && input.selector),
      comment: limitText(input && input.comment),
      focus_url: normalizeOptionalString(input && input.focus_url),
    };

    Object.keys(payload).forEach(function (key) {
      if (key !== "url" && payload[key] === undefined) {
        delete payload[key];
      }
    });

    return payload;
  }

  function buildInsertRequest(payload, id) {
    return {
      jsonrpc: "2.0",
      id: id,
      method: "browserAnnotation.insert",
      params: buildAnnotationPayload(payload),
    };
  }

  function buildSyncRequest(annotations, submit, id) {
    return {
      jsonrpc: "2.0",
      id: id,
      method: "browserAnnotation.sync",
      params: {
        annotations: (annotations || []).map(buildAnnotationPayload),
        submit: Boolean(submit),
      },
    };
  }

  function buildThemeRequest(id) {
    return {
      jsonrpc: "2.0",
      id: id,
      method: "browserAnnotation.theme",
      params: {},
    };
  }

  function normalizeTheme(input) {
    if (!input || typeof input !== "object") {
      return undefined;
    }

    const appearance = input.appearance === "light" ? "light" : "dark";
    const colors = {};
    const inputColors = input.colors && typeof input.colors === "object" ? input.colors : {};

    for (const key of Object.keys(THEME_COLOR_VARIABLES)) {
      const color = normalizeThemeColor(inputColors[key]);
      if (color) {
        colors[key] = color;
      }
    }

    return { appearance, colors };
  }

  function normalizeThemeColor(value) {
    if (typeof value !== "string") {
      return undefined;
    }

    const trimmed = value.trim();
    return trimmed.length > 0 && trimmed.length <= 128 ? trimmed : undefined;
  }

  function applyTheme(theme, target) {
    const normalized = normalizeTheme(theme);
    if (!normalized || !target || !target.style) {
      return normalized;
    }

    target.style.setProperty("--zed-browser-annotation-color-scheme", normalized.appearance);
    for (const [key, variableName] of Object.entries(THEME_COLOR_VARIABLES)) {
      const color = normalized.colors[key];
      if (color) {
        target.style.setProperty(variableName, color);
      }
    }

    return normalized;
  }

  root.ZedBrowserAnnotationHelpers = {
    applyTheme,
    buildAnnotationPayload,
    buildInsertRequest,
    buildSyncRequest,
    buildThemeRequest,
    normalizeTheme,
    selectorForElement,
  };

  if (typeof module === "object" && module.exports) {
    module.exports = root.ZedBrowserAnnotationHelpers;
  }
})(typeof globalThis === "undefined" ? this : globalThis);
