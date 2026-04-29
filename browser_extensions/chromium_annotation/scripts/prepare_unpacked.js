#!/usr/bin/env node

const childProcess = require("node:child_process");
const fs = require("node:fs");
const os = require("node:os");
const path = require("node:path");

const extensionRoot = path.resolve(__dirname, "..");
const browserExtensionsRoot = path.resolve(extensionRoot, "..");
const repositoryRoot = path.resolve(browserExtensionsRoot, "..");
const defaultUnpackedRoot = path.join("/tmp", "zed_browser_annotation_extension");
const preparedMarkerFile = ".zed-browser-annotation-unpacked";
const cliArguments = process.argv.slice(2);
const checkOnly = cliArguments.includes("--check");
const unpackedRoot = parseOutputPath();

function parseOutputPath() {
  const outputIndex = cliArguments.indexOf("--output");
  if (outputIndex === -1) {
    return defaultUnpackedRoot;
  }

  const outputPath = cliArguments[outputIndex + 1];
  if (!outputPath || outputPath.startsWith("--")) {
    throw new Error("--output requires a directory path.");
  }

  return path.resolve(outputPath);
}

function readManifest() {
  const manifestPath = path.join(extensionRoot, "manifest.json");
  try {
    return JSON.parse(fs.readFileSync(manifestPath, "utf8"));
  } catch (error) {
    throw new Error(`Failed to parse ${manifestPath}: ${error.message}`);
  }
}

function referencedFiles(manifest) {
  const files = new Set(["manifest.json"]);

  if (manifest.action && manifest.action.default_popup) {
    files.add(manifest.action.default_popup);
  }

  if (manifest.background && manifest.background.service_worker) {
    files.add(manifest.background.service_worker);
  }

  for (const contentScript of manifest.content_scripts || []) {
    for (const script of contentScript.js || []) {
      files.add(script);
    }

    for (const stylesheet of contentScript.css || []) {
      files.add(stylesheet);
    }
  }

  if (Array.isArray(manifest.web_accessible_resources)) {
    for (const resourceGroup of manifest.web_accessible_resources) {
      for (const resource of resourceGroup.resources || []) {
        files.add(resource);
      }
    }
  }

  return files;
}

function validateManifest(manifest) {
  if (manifest.manifest_version !== 3) {
    throw new Error("manifest_version must be 3.");
  }

  if (!manifest.name || !manifest.version) {
    throw new Error("manifest must include name and version.");
  }

  for (const relativePath of referencedFiles(manifest)) {
    const sourcePath = path.resolve(extensionRoot, relativePath);
    const relativeToRoot = path.relative(extensionRoot, sourcePath);
    if (relativeToRoot.startsWith("..") || path.isAbsolute(relativeToRoot)) {
      throw new Error(`Manifest references a path outside the extension root: ${relativePath}`);
    }

    if (!fs.existsSync(sourcePath)) {
      throw new Error(`Manifest references a missing file: ${relativePath}`);
    }
  }
}

function copyExtensionFiles() {
  validateOutputPath();
  validateReplaceableOutputDirectory();
  fs.rmSync(unpackedRoot, { recursive: true, force: true });
  fs.mkdirSync(unpackedRoot, { recursive: true });
  fs.copyFileSync(path.join(extensionRoot, "manifest.json"), path.join(unpackedRoot, "manifest.json"));
  fs.cpSync(path.join(extensionRoot, "src"), path.join(unpackedRoot, "src"), { recursive: true });
  fs.writeFileSync(path.join(unpackedRoot, preparedMarkerFile), "Zed browser annotation unpacked extension\n");
}

function validateOutputPath() {
  const resolvedOutput = path.resolve(unpackedRoot);
  const parsedOutput = path.parse(resolvedOutput);
  const forbiddenPaths = [
    parsedOutput.root,
    path.resolve(os.tmpdir()),
    extensionRoot,
    browserExtensionsRoot,
    repositoryRoot,
  ];

  for (const forbiddenPath of forbiddenPaths) {
    if (samePath(resolvedOutput, forbiddenPath) || isAncestorOf(resolvedOutput, forbiddenPath)) {
      throw new Error(`Refusing to prepare unpacked extension into unsafe directory: ${resolvedOutput}`);
    }
  }
}

function validateReplaceableOutputDirectory() {
  if (!fs.existsSync(unpackedRoot)) {
    return;
  }

  const stat = fs.statSync(unpackedRoot);
  if (!stat.isDirectory()) {
    throw new Error(`Refusing to replace non-directory output path: ${unpackedRoot}`);
  }

  const entries = fs.readdirSync(unpackedRoot);
  if (entries.length === 0 || entries.includes(preparedMarkerFile)) {
    return;
  }

  const knownPreparedEntries = new Set(["manifest.json", "src"]);
  if (entries.every((entry) => knownPreparedEntries.has(entry))) {
    return;
  }

  throw new Error(
    `Refusing to replace ${unpackedRoot} because it does not look like a prepared extension directory.`,
  );
}

function samePath(left, right) {
  return path.relative(left, right) === "";
}

function isAncestorOf(candidateAncestor, child) {
  const relative = path.relative(candidateAncestor, child);
  return relative !== "" && !relative.startsWith("..") && !path.isAbsolute(relative);
}

function clearMacExtendedAttributes() {
  if (process.platform !== "darwin") {
    return;
  }

  const result = childProcess.spawnSync("xattr", ["-cr", unpackedRoot], {
    encoding: "utf8",
  });

  if (result.error && result.error.code === "ENOENT") {
    console.warn("xattr was not found; skipping macOS extended attribute cleanup.");
    return;
  }

  if (result.status !== 0) {
    const message = (result.stderr || result.stdout || "xattr failed").trim();
    throw new Error(`Failed to clear macOS extended attributes from ${unpackedRoot}: ${message}`);
  }

  for (const filePath of pathsUnder(unpackedRoot)) {
    for (const attributeName of macExtendedAttributes(filePath)) {
      const deleteResult = childProcess.spawnSync("xattr", ["-d", attributeName, filePath], {
        encoding: "utf8",
      });

      if (deleteResult.status !== 0 && !deleteResult.stderr.includes("No such xattr")) {
        const message = (deleteResult.stderr || deleteResult.stdout || "xattr failed").trim();
        throw new Error(`Failed to remove ${attributeName} from ${filePath}: ${message}`);
      }
    }
  }
}

function assertNoPackagedArtifacts() {
  const blockedExtensions = new Set([".crx", ".pem", ".zip"]);
  const pending = [unpackedRoot];

  while (pending.length > 0) {
    const currentPath = pending.pop();
    for (const entry of fs.readdirSync(currentPath, { withFileTypes: true })) {
      const entryPath = path.join(currentPath, entry.name);
      if (entry.isDirectory()) {
        pending.push(entryPath);
      } else if (blockedExtensions.has(path.extname(entry.name))) {
        throw new Error(`Prepared extension contains a packaged artifact: ${entryPath}`);
      }
    }
  }
}

function warnAboutNearbyPackagedArtifacts() {
  const packagedArtifacts = [
    path.join(extensionRoot, "..", "chromium_annotation.crx"),
    path.join(extensionRoot, "..", "chromium_annotation.pem"),
  ].filter((artifactPath) => fs.existsSync(artifactPath));

  if (packagedArtifacts.length === 0) {
    return;
  }

  console.warn("Packaged extension artifacts still exist near the source tree:");
  for (const artifactPath of packagedArtifacts) {
    console.warn(`  ${artifactPath}`);
  }
  console.warn(`Use Chrome's Load unpacked button with this exact directory: ${unpackedRoot}`);
}

function assertNoMacQuarantine() {
  if (process.platform !== "darwin") {
    return;
  }

  for (const filePath of pathsUnder(unpackedRoot)) {
    if (macExtendedAttributes(filePath).includes("com.apple.quarantine")) {
      throw new Error(`Prepared extension is still quarantined: ${filePath}`);
    }
  }
}

function macExtendedAttributes(filePath) {
  const result = childProcess.spawnSync("xattr", [filePath], {
    encoding: "utf8",
  });

  if (result.status !== 0) {
    const message = (result.stderr || result.stdout || "xattr failed").trim();
    throw new Error(`Failed to inspect macOS extended attributes on ${filePath}: ${message}`);
  }

  return result.stdout
    .split("\n")
    .map((attributeName) => attributeName.trim())
    .filter(Boolean);
}

function pathsUnder(root) {
  const paths = [root];
  const pending = [root];

  while (pending.length > 0) {
    const currentPath = pending.pop();
    for (const entry of fs.readdirSync(currentPath, { withFileTypes: true })) {
      const entryPath = path.join(currentPath, entry.name);
      paths.push(entryPath);

      if (entry.isDirectory()) {
        pending.push(entryPath);
      }
    }
  }

  return paths;
}

function main() {
  const manifest = readManifest();
  validateManifest(manifest);

  if (checkOnly) {
    console.log("Manifest and referenced files are valid.");
    return;
  }

  copyExtensionFiles();
  clearMacExtendedAttributes();
  assertNoPackagedArtifacts();
  assertNoMacQuarantine();
  warnAboutNearbyPackagedArtifacts();
  console.log(`Prepared unpacked extension: ${unpackedRoot}`);
}

main();
