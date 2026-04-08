#!/usr/bin/env node
"use strict";

const { existsSync, mkdirSync, createWriteStream, chmodSync, unlinkSync } = require("fs");
const { join } = require("path");
const { get } = require("https");
const { createGunzip } = require("zlib");

const VERSION = require("./package.json").version;
const REPO = "chapeaux/ruddydoc";

const PLATFORM_MAP = {
  "darwin-x64": "x86_64-apple-darwin",
  "darwin-arm64": "aarch64-apple-darwin",
  "linux-x64": "x86_64-unknown-linux-gnu",
  "linux-arm64": "aarch64-unknown-linux-gnu",
  "win32-x64": "x86_64-pc-windows-msvc",
  "win32-arm64": "aarch64-pc-windows-msvc",
};

const key = `${process.platform}-${process.arch}`;
const target = PLATFORM_MAP[key];
if (!target) {
  console.error(`Unsupported platform: ${key}`);
  console.error("Supported platforms:", Object.keys(PLATFORM_MAP).join(", "));
  console.error("You can install manually with: cargo install ruddydoc");
  process.exit(1);
}

const ext = process.platform === "win32" ? ".exe" : "";
const binName = `ruddydoc${ext}`;
const binDir = join(__dirname, "bin");
const binPath = join(binDir, binName);

// Skip if binary already exists
if (existsSync(binPath)) {
  console.log("ruddydoc binary already installed");
  process.exit(0);
}

const archiveExt = process.platform === "win32" ? ".zip" : ".tar.gz";
const url = `https://github.com/${REPO}/releases/download/v${VERSION}/ruddydoc-v${VERSION}-${target}${archiveExt}`;

function download(url, dest) {
  return new Promise((resolve, reject) => {
    get(url, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return download(res.headers.location, dest).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`Download failed: HTTP ${res.statusCode} for ${url}`));
      }

      mkdirSync(binDir, { recursive: true });
      const file = createWriteStream(dest);
      res.pipe(file);
      file.on("finish", () => {
        file.close();
        resolve();
      });
      file.on("error", reject);
    }).on("error", reject);
  });
}

async function extractTarGz(archivePath, destDir) {
  const { execSync } = require("child_process");
  try {
    execSync(`tar -xzf "${archivePath}" -C "${destDir}"`, { stdio: "inherit" });
  } catch (err) {
    throw new Error(`Failed to extract tar.gz: ${err.message}`);
  }
}

async function extractZip(archivePath, destDir) {
  const { execSync } = require("child_process");
  try {
    execSync(`powershell -Command "Expand-Archive -Path '${archivePath}' -DestinationPath '${destDir}' -Force"`, { stdio: "inherit" });
  } catch (err) {
    throw new Error(`Failed to extract zip: ${err.message}`);
  }
}

async function main() {
  const tmpPath = join(binDir, `ruddydoc-download${archiveExt}`);

  console.log(`Downloading ruddydoc v${VERSION} for ${target}...`);
  console.log(`URL: ${url}`);

  try {
    await download(url, tmpPath);
  } catch (err) {
    console.error(`Failed to download: ${err.message}`);
    console.error("You can install manually with: cargo install ruddydoc");
    process.exit(1);
  }

  console.log("Extracting binary...");
  try {
    if (archiveExt === ".tar.gz") {
      await extractTarGz(tmpPath, binDir);
    } else {
      await extractZip(tmpPath, binDir);
    }
  } catch (err) {
    console.error(`Failed to extract: ${err.message}`);
    process.exit(1);
  }

  // Clean up archive
  try {
    unlinkSync(tmpPath);
  } catch (err) {
    // Ignore cleanup errors
  }

  // Make binary executable on Unix
  if (process.platform !== "win32") {
    try {
      chmodSync(binPath, 0o755);
    } catch (err) {
      console.error(`Warning: Failed to make binary executable: ${err.message}`);
    }
  }

  console.log(`Successfully installed ruddydoc to ${binPath}`);
}

main().catch((err) => {
  console.error(`Installation failed: ${err.message}`);
  console.error("You can install manually with: cargo install ruddydoc");
  process.exit(1);
});
