#!/usr/bin/env node

import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { assertVersionsAligned } from './versioning.mjs';

const repoRoot = path.resolve(fileURLToPath(new URL('..', import.meta.url)));

function parseArgs(argv) {
  const result = {
    channel: 'stable',
    artifact: '',
    baseUrl: 'https://clawy-releases.oss-cn-shenzhen.aliyuncs.com',
    output: path.join(repoRoot, 'release-info.json'),
    merge: '',
    changelog: '',
    releaseDate: new Date().toISOString(),
  };

  for (let index = 0; index < argv.length; index += 1) {
    const arg = argv[index];
    const next = argv[index + 1];
    switch (arg) {
      case '--':
        break;
      case '--channel':
        result.channel = next ?? result.channel;
        index += 1;
        break;
      case '--artifact':
        result.artifact = next ?? '';
        index += 1;
        break;
      case '--base-url':
        result.baseUrl = next ?? result.baseUrl;
        index += 1;
        break;
      case '--output':
        result.output = next ?? result.output;
        index += 1;
        break;
      case '--merge':
        result.merge = next ?? '';
        index += 1;
        break;
      case '--changelog':
        result.changelog = next ?? '';
        index += 1;
        break;
      case '--release-date':
        result.releaseDate = next ?? result.releaseDate;
        index += 1;
        break;
      case '--help':
      case '-h':
        printHelp();
        process.exit(0);
      default:
        throw new Error(`Unknown argument: ${arg}`);
    }
  }

  return result;
}

function printHelp() {
  console.log(`Usage:
  node scripts/generate-update-manifest.mjs [options]

Options:
  --channel <stable|beta|dev>   Update channel, default stable
  --artifact <path>             Built artifact path. If omitted, auto-detect current platform artifact
  --base-url <url>              Public base URL, default https://clawy-releases.oss-cn-shenzhen.aliyuncs.com
  --output <path>               Output manifest path, default ./release-info.json
  --merge <path-or-url>         Merge into an existing manifest with the same version
  --changelog <url>             Optional release notes URL
  --release-date <iso>          Optional release date, default now
`);
}

function channelDirectory(channel) {
  switch (String(channel).trim().toLowerCase()) {
    case 'beta':
      return 'beta';
    case 'dev':
      return 'alpha';
    default:
      return 'latest';
  }
}

function currentTarget() {
  const platform = process.platform;
  const arch = process.arch === 'arm64' ? 'arm64' : 'x64';

  if (platform === 'darwin') return { platform: 'mac', arch };
  if (platform === 'win32') return { platform: 'win', arch };
  if (platform === 'linux') return { platform: 'linux', arch };
  throw new Error(`Unsupported platform: ${platform}`);
}

async function firstExisting(paths) {
  for (const candidate of paths) {
    try {
      await fs.access(candidate);
      return candidate;
    } catch {
      // ignore
    }
  }
  return '';
}

async function autoDetectArtifact(target) {
  const bundleRoot = path.join(repoRoot, 'src-tauri', 'target', 'release', 'bundle');

  if (target.platform === 'mac') {
    return firstExisting([
      path.join(bundleRoot, 'dmg', `Clawy_${await assertVersionsAligned()}_${target.arch === 'arm64' ? 'aarch64' : 'x64'}.dmg`),
    ]);
  }

  if (target.platform === 'win') {
    return firstExisting([
      path.join(bundleRoot, 'nsis', `Clawy_${await assertVersionsAligned()}_${target.arch === 'arm64' ? 'arm64-setup' : 'x64-setup'}.exe`),
      path.join(bundleRoot, 'msi', `Clawy_${await assertVersionsAligned()}_${target.arch === 'arm64' ? 'arm64_en-US' : 'x64_en-US'}.msi`),
    ]);
  }

  return firstExisting([
    path.join(bundleRoot, 'appimage', `Clawy_${await assertVersionsAligned()}_${target.arch}.AppImage`),
    path.join(bundleRoot, 'deb', `clawy_${await assertVersionsAligned()}_${target.arch === 'arm64' ? 'arm64' : 'amd64'}.deb`),
    path.join(bundleRoot, 'rpm', `clawy-${await assertVersionsAligned()}-1.${target.arch === 'arm64' ? 'aarch64' : 'x86_64'}.rpm`),
  ]);
}

async function loadJsonFromPathOrUrl(value) {
  if (!value) return null;
  if (/^https?:\/\//.test(value)) {
    const response = await fetch(value);
    if (!response.ok) {
      throw new Error(`Failed to fetch manifest: ${value} (${response.status})`);
    }
    return response.json();
  }

  const contents = await fs.readFile(path.resolve(value), 'utf8');
  return JSON.parse(contents);
}

function emptyManifest(version, channel, releaseDate, changelog) {
  return {
    version,
    channel,
    releaseDate,
    changelog: changelog || undefined,
    downloads: {
      mac: {},
      win: {},
      linux: {},
    },
  };
}

function applyArtifact(manifest, target, artifactUrl, artifactPath) {
  const extension = path.extname(artifactPath).toLowerCase();

  if (target.platform === 'mac') {
    manifest.downloads.mac[target.arch] = artifactUrl;
    return;
  }

  if (target.platform === 'win') {
    manifest.downloads.win[target.arch] = artifactUrl;
    return;
  }

  if (extension === '.deb') {
    manifest.downloads.linux[target.arch === 'arm64' ? 'deb_arm64' : 'deb_amd64'] = artifactUrl;
    return;
  }
  if (extension === '.rpm') {
    manifest.downloads.linux.rpm_x64 = artifactUrl;
    return;
  }

  manifest.downloads.linux[target.arch === 'arm64' ? 'appimage_arm64' : 'appimage_x64'] = artifactUrl;
}

const options = parseArgs(process.argv.slice(2));
const version = await assertVersionsAligned();
const channel = channelDirectory(options.channel);
const target = currentTarget();
const artifactPath = options.artifact
  ? path.resolve(options.artifact)
  : await autoDetectArtifact(target);

if (!artifactPath) {
  throw new Error('Could not auto-detect a packaged artifact. Pass --artifact explicitly.');
}

const artifactName = path.basename(artifactPath);
const artifactUrl = `${options.baseUrl.replace(/\/+$/, '')}/${channel}/${artifactName}`;

let manifest = emptyManifest(version, channel, options.releaseDate, options.changelog);
if (options.merge) {
  const existing = await loadJsonFromPathOrUrl(options.merge);
  if (existing?.version && existing.version !== version) {
    throw new Error(
      `Existing manifest version ${existing.version} does not match local version ${version}.`,
    );
  }
  manifest = {
    ...manifest,
    ...existing,
    version,
    channel,
    releaseDate: options.releaseDate,
    changelog: options.changelog || existing?.changelog,
    downloads: {
      mac: { ...(existing?.downloads?.mac ?? {}) },
      win: { ...(existing?.downloads?.win ?? {}) },
      linux: { ...(existing?.downloads?.linux ?? {}) },
    },
  };
}

applyArtifact(manifest, target, artifactUrl, artifactPath);
await fs.writeFile(
  path.resolve(options.output),
  `${JSON.stringify(manifest, null, 2)}\n`,
  'utf8',
);

console.log(`Generated manifest for ${target.platform}/${target.arch}`);
console.log(`Artifact: ${artifactPath}`);
console.log(`URL: ${artifactUrl}`);
console.log(`Manifest: ${path.resolve(options.output)}`);
