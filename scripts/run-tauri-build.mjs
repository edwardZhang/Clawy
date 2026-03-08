#!/usr/bin/env zx

import 'zx/globals';

const mode = (argv.mode || 'lite').trim().toLowerCase();
const configPath = argv.config ? String(argv.config) : null;
const bundles = argv.bundles ? String(argv.bundles) : null;
const debug = argv.debug === true;

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
await $({ env })`pnpm ${args}`;
