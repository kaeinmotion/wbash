#!/usr/bin/env node
const { spawnSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');
const https = require('https');

const VERSION = 'v0.2.7';
const REPO = 'kaeinmotion/wxec';

function getTarget() {
  const platform = os.platform();
  const arch = os.arch();
  const archMap = { x64: 'x86_64', arm64: 'aarch64' };
  const targetArch = archMap[arch];
  if (!targetArch) {
    console.error(`Unsupported architecture: ${arch}`);
    process.exit(1);
  }
  if (platform === 'linux') return `${targetArch}-unknown-linux-gnu`;
  if (platform === 'darwin') return `${targetArch}-apple-darwin`;
  console.error(`Unsupported platform: ${platform}`);
  process.exit(1);
}

function getCacheDir() {
  const base = process.env.XDG_CACHE_HOME || path.join(os.homedir(), '.cache');
  const dir = path.join(base, 'wxec', VERSION);
  fs.mkdirSync(dir, { recursive: true });
  return dir;
}

function downloadBinary(target, destPath) {
  const url = `https://github.com/${REPO}/releases/download/${VERSION}/wxec-${target}.tar.gz`;
  const tmpTar = destPath + '.tar.gz';

  return new Promise((resolve, reject) => {
    function fetch(currentUrl) {
      https.get(currentUrl, res => {
        if (res.statusCode >= 300 && res.statusCode < 400 && res.headers.location) {
          fetch(res.headers.location);
          return;
        }
        if (res.statusCode !== 200) {
          reject(new Error(`Failed to download wxec binary: HTTP ${res.statusCode}`));
          return;
        }
        const fileStream = fs.createWriteStream(tmpTar);
        res.pipe(fileStream);
        fileStream.on('finish', () => {
          fileStream.close(() => {
            try {
              spawnSync('tar', ['-xzf', tmpTar, '-C', path.dirname(destPath)], { stdio: 'inherit' });
              fs.unlinkSync(tmpTar);
              fs.chmodSync(destPath, 0o755);
              resolve();
            } catch (err) {
              reject(err);
            }
          });
        });
      }).on('error', reject);
    }
    fetch(url);
  });
}

async function main() {
  const target = getTarget();
  const cacheDir = getCacheDir();
  const binaryPath = path.join(cacheDir, 'wxec');

  if (!fs.existsSync(binaryPath)) {
    process.stderr.write(`wxec: fetching prebuilt binary (${target})...\n`);
    try {
      await downloadBinary(target, binaryPath);
    } catch (e) {
      console.error(`Error installing wxec: ${e.message}`);
      process.exit(1);
    }
  }

  const result = spawnSync(binaryPath, process.argv.slice(2), {
    stdio: 'inherit',
    env: process.env,
  });

  process.exit(result.status ?? 0);
}

main().catch(err => {
  console.error(err);
  process.exit(1);
});
