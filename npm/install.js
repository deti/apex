#!/usr/bin/env node
"use strict";

const { execSync } = require("child_process");
const fs = require("fs");
const https = require("https");
const os = require("os");
const path = require("path");
const { createWriteStream } = require("fs");
const { pipeline } = require("stream/promises");
const { createGunzip } = require("zlib");
const tar = require("tar"); // peer/optional — fallback below

const REPO = "allexdav2/apex";
const BINARY = "apex";
const VERSION = require("./package.json").version;

function getTarget() {
  const platform = os.platform();
  const arch = os.arch();

  const osMap = { darwin: "apple-darwin", linux: "unknown-linux-gnu" };
  const archMap = { x64: "x86_64", arm64: "aarch64" };

  const targetOs = osMap[platform];
  const targetArch = archMap[arch];

  if (!targetOs || !targetArch) {
    throw new Error(`Unsupported platform: ${platform}-${arch}`);
  }
  return `${targetArch}-${targetOs}`;
}

function download(url) {
  return new Promise((resolve, reject) => {
    https.get(url, (res) => {
      if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
        return download(res.headers.location).then(resolve, reject);
      }
      if (res.statusCode !== 200) {
        return reject(new Error(`Download failed: HTTP ${res.statusCode}`));
      }
      resolve(res);
    }).on("error", reject);
  });
}

async function main() {
  const target = getTarget();
  const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${BINARY}-${target}.tar.gz`;
  const binDir = path.join(__dirname, "bin");
  const binPath = path.join(binDir, BINARY);

  if (fs.existsSync(binPath)) {
    return; // already installed
  }

  console.log(`Downloading ${BINARY} v${VERSION} (${target})...`);

  const tmpDir = fs.mkdtempSync(path.join(os.tmpdir(), "apex-"));
  const tarPath = path.join(tmpDir, "archive.tar.gz");

  try {
    const stream = await download(url);
    await pipeline(stream, createWriteStream(tarPath));

    // Extract using tar command (avoids npm tar dependency)
    fs.mkdirSync(binDir, { recursive: true });
    execSync(`tar xzf "${tarPath}" -C "${binDir}"`, { stdio: "ignore" });
    fs.chmodSync(binPath, 0o755);

    console.log(`Installed ${BINARY} to ${binPath}`);
  } finally {
    fs.rmSync(tmpDir, { recursive: true, force: true });
  }
}

main().catch((err) => {
  console.error(`Failed to install ${BINARY}: ${err.message}`);
  process.exit(1);
});
