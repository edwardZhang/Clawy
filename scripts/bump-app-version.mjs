#!/usr/bin/env node

import {
  assertVersionsAligned,
  formatVersionState,
  readVersionState,
  resolveNextVersion,
  writeVersion,
} from './versioning.mjs';

function printHelp() {
  console.log(`Usage:
  node scripts/bump-app-version.mjs [patch|minor|major]
  node scripts/bump-app-version.mjs --set <version>
  node scripts/bump-app-version.mjs --print

Examples:
  node scripts/bump-app-version.mjs patch
  node scripts/bump-app-version.mjs minor
  node scripts/bump-app-version.mjs --set 0.3.12
`);
}

const args = process.argv.slice(2);

if (args.includes('--help') || args.includes('-h')) {
  printHelp();
  process.exit(0);
}

if (args.includes('--print')) {
  const state = await readVersionState();
  console.log(formatVersionState(state));
  process.exit(0);
}

const setIndex = args.indexOf('--set');
const target = setIndex >= 0 ? args[setIndex + 1] : args[0] || 'patch';

if (!target) {
  console.error('Missing version after --set');
  process.exit(1);
}

const currentVersion = await assertVersionsAligned();
const nextVersion = resolveNextVersion(currentVersion, target);

if (nextVersion === currentVersion) {
  console.log(`Version already ${currentVersion}`);
  process.exit(0);
}

await writeVersion(nextVersion);
const updated = await readVersionState();
console.log(`Version bumped: ${currentVersion} -> ${nextVersion}`);
console.log(formatVersionState(updated));

