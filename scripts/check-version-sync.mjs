#!/usr/bin/env node

import { assertVersionsAligned, formatVersionState, readVersionState } from './versioning.mjs';

try {
  const version = await assertVersionsAligned();
  console.log(`Version sync check passed: ${version}`);
} catch (error) {
  const state = await readVersionState().catch(() => null);
  console.error(error instanceof Error ? error.message : String(error));
  if (state) {
    console.error(formatVersionState(state));
  }
  process.exit(1);
}

