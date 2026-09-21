#!/usr/bin/env node
/**
 * Amber Shield CLI — npx launcher
 * Downloads the native Tauri binary for the current platform.
 * Usage: npx amber-shield-cli [args]
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
  const isArm = arch === "arm64";

  if (platform === "win32") return { target: "x86_64-pc-windows-msvc", ext: ".exe", name: "amber-shield-lite.exe" };
  if (platform === "darwin") return { target: isArm ? "aarch64-apple-darwin" : "x86_64-apple-darwin", ext: "", name: "amber-shield-lite" };
  if (platform === "linux") return { target: "x86_64-unknown-linux-gnu", ext: "", name: "amber-shield-lite" };
  throw new Error(`Unsupported platform: ${platform}/${arch}`);
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
  const binPath = path.join(INSTALL_DIR, p.name);

  if (fs.existsSync(binPath)) {
    const args = process.argv.slice(2).map(a => `"${a}"`).join(" ");
    try {
      execSync(`"${binPath}" ${args}`, { stdio: "inherit", shell: true });
    } catch (e) { process.exit(e.status || 1); }
    return;
  }

  console.log(`[amber-shield] Downloading ${p.name} v${VERSION}...`);
  fs.mkdirSync(INSTALL_DIR, { recursive: true });

  const url = `https://github.com/${REPO}/releases/download/v${VERSION}/${p.name}`;
  await download(url, binPath);
  fs.chmodSync(binPath, 0o755);

  console.log(`[amber-shield] Installed to ${binPath}`);
  const args = process.argv.slice(2).map(a => `"${a}"`).join(" ");
  try {
    execSync(`"${binPath}" ${args}`, { stdio: "inherit", shell: true });
  } catch (e) { process.exit(e.status || 1); }
}

main().catch((e) => { console.error("[amber-shield]", e.message); process.exit(1); });
