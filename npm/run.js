#!/usr/bin/env node
"use strict";

const { spawnSync } = require("child_process");
const { join } = require("path");
const { existsSync } = require("fs");

const ext = process.platform === "win32" ? ".exe" : "";
const binName = `ruddydoc${ext}`;
const binPath = join(__dirname, "bin", binName);

if (!existsSync(binPath)) {
  console.error("ruddydoc binary not found.");
  console.error("Try running: npm install");
  console.error("Or install manually with: cargo install ruddydoc");
  process.exit(1);
}

const result = spawnSync(binPath, process.argv.slice(2), {
  stdio: "inherit",
  windowsHide: true,
});

if (result.error) {
  console.error(`Failed to execute ruddydoc: ${result.error.message}`);
  process.exit(1);
}

process.exit(result.status || 0);
