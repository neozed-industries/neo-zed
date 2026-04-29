# Zed Browser Annotations Chromium Extension

This is a production-oriented Manifest V3 Chromium extension scaffold for sending browser annotations to Zed through native messaging.

## Development

1. Build or install the `browser_annotation_host` native messaging host separately.
2. Register the native host as `browser_annotation_host` in Chromium's native messaging host registry.
3. Prepare a clean unpacked extension directory:

   ```sh
   npm run prepare:unpacked
   ```

4. Open `chrome://extensions`, enable Developer mode, choose **Load unpacked**, and select:

   ```text
   /tmp/zed_browser_annotation_extension
   ```

5. Open an `http` or `https` page, click the extension action, select page text or use **Pick Element**, add a comment, and click **Send to Zed**. Zed opens the draft for review before the agent receives it.

Do not load a `.crx`, `.pem`, or the parent `browser_extensions` directory during local development. The prepare step copies only the manifest and runtime `src/` files into `/tmp/zed_browser_annotation_extension`, validates manifest references, and clears macOS quarantine/removable extended attributes from that prepared directory. To prepare a different directory, run `npm run prepare:unpacked -- --output /path/to/directory`.

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
npm run check:manifest
npm test
```
