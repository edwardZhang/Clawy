#!/usr/bin/env zx

import 'zx/globals';

if (process.platform === 'win32') {
  usePowerShell();
}

const pnpmCommand = process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm';
const zxCommand = process.platform === 'win32' ? 'zx.cmd' : 'zx';

const mode = (process.env.CLAWY_PACKAGE_MODE || 'lite').trim().toLowerCase();
const validModes = new Set(['lite', 'full']);
if (!validModes.has(mode)) {
  throw new Error(`Unsupported CLAWY_PACKAGE_MODE: ${mode}`);
}

echo(chalk.cyan(`Preparing Tauri build resources in ${mode} mode...`));

await $`${pnpmCommand} run build:vite`;
await $`${zxCommand} scripts/bundle-openclaw-plugins.mjs`;
await $`${zxCommand} scripts/bundle-clawhub.mjs`;

if (mode === 'full') {
  await $`${pnpmCommand} run uv:download`;
  await $`${pnpmCommand} run node:prepare`;
  await $`${zxCommand} scripts/bundle-openclaw.mjs`;
} else {
  await fs.remove(path.join(process.cwd(), 'build', 'openclaw'));
  await fs.remove(path.join(process.cwd(), 'resources', 'bin'));
}

echo(chalk.green(`Tauri build resources ready (${mode}).`));
