const assert = require("node:assert/strict");
const helpers = require("../src/shared_core.js");

function element(localName, options = {}) {
  return {
    nodeType: 1,
    localName,
    id: options.id || "",
    classList: options.classList || [],
    children: options.children || [],
    parentElement: options.parentElement,
    ownerDocument: { documentElement: options.documentElement || {} },
  };
}

function linkChildren(parent, children) {
  parent.children = children;
  children.forEach((child) => {
    child.parentElement = parent;
    child.ownerDocument = parent.ownerDocument;
  });
}

assert.deepEqual(
  helpers.buildAnnotationPayload({
    url: " https://example.com ",
    title: " Example ",
    selected_text: " Selected text ",
    selector: " main ",
    comment: " Review this ",
    focus_url: " chrome-extension://extension/src/focus.html?tabId=1 ",
  }),
  {
    url: "https://example.com",
    title: "Example",
    selected_text: "Selected text",
    selector: "main",
    comment: "Review this",
    focus_url: "chrome-extension://extension/src/focus.html?tabId=1",
  },
);

assert.deepEqual(
  helpers.buildAnnotationPayload({
    url: "https://example.com",
    title: " ",
    selected_text: "",
    selector: undefined,
    comment: null,
  }),
  { url: "https://example.com" },
);

assert.deepEqual(
  helpers.buildInsertRequest({ url: "https://example.com" }, 7),
  {
    jsonrpc: "2.0",
    id: 7,
    method: "browserAnnotation.insert",
    params: { url: "https://example.com" },
  },
);

assert.deepEqual(helpers.buildThemeRequest(8), {
  jsonrpc: "2.0",
  id: 8,
  method: "browserAnnotation.theme",
  params: {},
});

assert.deepEqual(
  helpers.normalizeTheme({
    appearance: "light",
    colors: {
      panel_background: " rgba(255, 255, 255, 1) ",
      text: "rgba(20, 20, 20, 1)",
      unknown: "red",
    },
  }),
  {
    appearance: "light",
    colors: {
      panel_background: "rgba(255, 255, 255, 1)",
      text: "rgba(20, 20, 20, 1)",
    },
  },
);

const themeTarget = {
  values: {},
  style: {
    setProperty(name, value) {
      themeTarget.values[name] = value;
    },
  },
};
helpers.applyTheme(
  {
    appearance: "dark",
    colors: {
      panel_background: "rgba(30, 30, 30, 1)",
      text: "rgba(220, 220, 220, 1)",
    },
  },
  themeTarget,
);
assert.deepEqual(themeTarget.values, {
  "--zed-browser-annotation-color-scheme": "dark",
  "--zed-browser-annotation-panel-background": "rgba(30, 30, 30, 1)",
  "--zed-browser-annotation-text": "rgba(220, 220, 220, 1)",
});

const documentElement = element("html");
documentElement.ownerDocument = { documentElement };
const body = element("body", { documentElement });
const article = element("article", { classList: ["story"], documentElement });
const firstParagraph = element("p", { documentElement });
const secondParagraph = element("p", { classList: ["lede"], documentElement });

linkChildren(documentElement, [body]);
linkChildren(body, [article]);
linkChildren(article, [firstParagraph, secondParagraph]);

assert.equal(
  helpers.selectorForElement(secondParagraph),
  "body > article.story > p.lede:nth-of-type(2)",
);

const identified = element("section", { id: "main content" });
assert.equal(helpers.selectorForElement(identified), "section#main\\ content");

console.log("shared_core tests passed");
