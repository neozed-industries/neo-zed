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
  }),
  {
    url: "https://example.com",
    title: "Example",
    selected_text: "Selected text",
    selector: "main",
    comment: "Review this",
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
