#!/usr/bin/env zx

import 'zx/globals';

const mode = (process.env.CLAWY_PACKAGE_MODE || 'lite').trim().toLowerCase();
const validModes = new Set(['lite', 'full']);
if (!validModes.has(mode)) {
  throw new Error(`Unsupported CLAWY_PACKAGE_MODE: ${mode}`);
}

echo(chalk.cyan(`Preparing Tauri build resources in ${mode} mode...`));

await $`pnpm run build:vite`;
await $`zx scripts/bundle-openclaw-plugins.mjs`;
await $`zx scripts/bundle-clawhub.mjs`;

if (mode === 'full') {
  await $`pnpm run uv:download`;
  await $`pnpm run node:prepare`;
  await $`zx scripts/bundle-openclaw.mjs`;
} else {
  await fs.remove(path.join(process.cwd(), 'build', 'openclaw'));
  await fs.remove(path.join(process.cwd(), 'resources', 'bin'));
}

echo(chalk.green(`Tauri build resources ready (${mode}).`));
