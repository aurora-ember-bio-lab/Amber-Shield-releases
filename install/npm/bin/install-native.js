#!/usr/bin/env node
/**
 * Amber Shield — postinstall script
 * Downloads the native binary for the current platform.
 */

const { execSync } = require("child_process");
const fs = require("fs");
const path = require("path");
const https = require("https");
const os = require("os");

const REPO = "aurora-ember-bio-lab/Amber-Shield-releases";
const VERSION = "0.1.0";
const INSTALL_DIR = path.join(os.homedir(), ".amber-shield", "bin");

function getPlatform() {
  const platform = os.platform();
  const arch = os.arch();
  if (platform === "win32") return { ext: ".exe", name: "amber-shield-lite.exe" };
  if (platform === "darwin") return { ext: "", name: "amber-shield-lite" };
  if (platform === "linux") return { ext: "", name: "amber-shield-lite" };
  return null;
}

function download(url, dest) {
  return new Promise((resolve, reject) => {
    const follow = (url) => {
      https.get(url, (res) => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          return follow(res.headers.location);
        }
        if (res.statusCode !== 200) return reject(new Error(`HTTP ${res.statusCode}`));
        const file = fs.createWriteStream(dest);
        res.pipe(file);
        file.on("finish", () => { file.close(); resolve(); });
      }).on("error", reject);
    };
    follow(url);
  });
}

async function main() {
  const p = getPlatform();
  if (!p) { console.log("[amber-shield] Skipping native binary for this platform."); return; }

  const binPath = path.join(INSTALL_DIR, p.name);
  if (fs.existsSync(binPath)) { console.log("[amber-shield] Binary already installed."); return; }

  console.log(`[amber-shield] Downloading native binary v${VERSION}...`);
  fs.mkdirSync(INSTALL_DIR, { recursive: true });

  const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${p.name}`;
  try {
    await download(url, binPath);
    fs.chmodSync(binPath, 0o755);
    console.log(`[amber-shield] Installed to ${binPath}`);
  } catch (e) {
    console.log(`[amber-shield] Could not download native binary: ${e.message}`);
    console.log("[amber-shield] The CLI will run in demo mode. Install the desktop app for full features.");
  }
}

main().catch(() => {});
