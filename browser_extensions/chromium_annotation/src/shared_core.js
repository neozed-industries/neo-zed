(function (root) {
  "use strict";

  const MAX_TEXT_LENGTH = 20000;

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
      url: normalizeOptionalString(input && input.url) || "",
      title: normalizeOptionalString(input && input.title),
      selected_text: limitText(input && input.selected_text),
      selector: normalizeOptionalString(input && input.selector),
      comment: limitText(input && input.comment),
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

  root.ZedBrowserAnnotationHelpers = {
    buildAnnotationPayload,
    buildInsertRequest,
    selectorForElement,
  };

  if (typeof module === "object" && module.exports) {
    module.exports = root.ZedBrowserAnnotationHelpers;
  }
})(typeof globalThis === "undefined" ? this : globalThis);
