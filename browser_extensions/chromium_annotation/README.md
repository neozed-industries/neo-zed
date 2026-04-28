# Zed Browser Annotations Chromium Extension

This is a production-oriented Manifest V3 Chromium extension scaffold for sending browser annotations to Zed through native messaging.

## Development

1. Build or install the `browser_annotation_host` native messaging host separately.
2. Register the native host as `browser_annotation_host` in Chromium's native messaging host registry.
3. Open `chrome://extensions`, enable Developer mode, and load this directory as an unpacked extension.
4. Open an `http` or `https` page, click the extension action, select page text or use **Pick Element**, add a comment, and click **Send to Zed**.

The extension sends a JSON-RPC native message:

```json
{
  "jsonrpc": "2.0",
  "id": 1,
  "method": "browserAnnotation.insert",
  "params": {
    "url": "https://example.com",
    "title": "Example",
    "selected_text": "Selected text",
    "selector": "main article p:nth-of-type(2)",
    "comment": "Review this"
  }
}
```

## Tests

```sh
npm test
```
