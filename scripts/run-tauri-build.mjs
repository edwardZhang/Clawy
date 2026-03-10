#!/usr/bin/env zx

import 'zx/globals';
import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { configureMacosSigning } from './configure-macos-signing.mjs';

const mode = (argv.mode || 'lite').trim().toLowerCase();
const configPath = argv.config ? String(argv.config) : null;
const bundles = argv.bundles ? String(argv.bundles) : null;
const debug = argv.debug === true;
const signMode = argv.sign ? String(argv.sign) : null;
const notarize = argv.notarize === true ? true : null;
const repoRoot = path.resolve(fileURLToPath(new URL('..', import.meta.url)));

if (!['lite', 'full'].includes(mode)) {
  throw new Error(`Unsupported build mode: ${mode}`);
}

const env = {
  ...process.env,
  CLAWY_PACKAGE_MODE: mode,
};

if (mode === 'full') {
  env.CLAWY_FULL_MODE_RUNTIME = 'true';
}

const signing = await configureMacosSigning({
  env,
  requestedSignMode: signMode,
  requestedNotarize: notarize,
});

async function cleanupMacosBundleArtifacts() {
  if (process.platform !== 'darwin') {
    return;
  }

  const profile = debug ? 'debug' : 'release';
  const bundleRoot = path.join(repoRoot, 'src-tauri', 'target', profile, 'bundle');
  const candidateDirs = [
    path.join(bundleRoot, 'macos'),
    path.join(bundleRoot, 'dmg'),
  ];
  const removablePatterns = [
    /^rw\..*\.dmg$/,
    /\.app\.bak$/,
  ];
  const removed = [];

  for (const dir of candidateDirs) {
    let entries = [];
    try {
      entries = await fs.readdir(dir, { withFileTypes: true });
    } catch {
      continue;
    }

    for (const entry of entries) {
      if (!removablePatterns.some((pattern) => pattern.test(entry.name))) {
        continue;
      }

      const target = path.join(dir, entry.name);
      await fs.rm(target, { recursive: true, force: true });
      removed.push(path.relative(repoRoot, target));
    }
  }

  if (removed.length > 0) {
    echo(chalk.cyan(`Cleaned stale macOS bundle artifacts:\n- ${removed.join('\n- ')}`));
  }
}

const args = ['exec', 'tauri', 'build'];
if (configPath) {
  args.push('--config', configPath);
}
if (bundles) {
  args.push('--bundles', bundles);
}
if (debug) {
  args.push('--debug');
}

echo(chalk.cyan(`Running Tauri build in ${mode} mode...`));
echo(chalk.cyan(`macOS signing: ${signing.summary}`));
await cleanupMacosBundleArtifacts();
await $({ env })`pnpm ${args}`;
