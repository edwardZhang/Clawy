#!/usr/bin/env node

import { execFileSync } from 'node:child_process';

function git(args, options = {}) {
  return execFileSync('git', args, {
    cwd: options.cwd ?? process.cwd(),
    encoding: 'utf8',
    stdio: options.stdio ?? ['ignore', 'pipe', 'pipe'],
  }).trim();
}

function readWorktreeList(cwd) {
  const output = git(['worktree', 'list', '--porcelain'], { cwd });
  const blocks = output
    .split('\n\n')
    .map((block) => block.trim())
    .filter(Boolean);

  return blocks.map((block) => {
    const entry = {};
    for (const line of block.split('\n')) {
      const [key, ...rest] = line.split(' ');
      entry[key] = rest.join(' ');
    }
    return entry;
  });
}

function fail(message, details = []) {
  console.error(`Release preflight failed: ${message}`);
  for (const detail of details) {
    console.error(`- ${detail}`);
  }
  process.exit(1);
}

const repoRoot = git(['rev-parse', '--show-toplevel']);
const worktrees = readWorktreeList(repoRoot);
const mainWorktree = worktrees[0]?.worktree;

if (!mainWorktree) {
  fail('Unable to determine the main repository checkout.');
}

if (repoRoot !== mainWorktree) {
  fail('Final packaging must be run from the main repository checkout.', [
    `Current checkout: ${repoRoot}`,
    `Main checkout: ${mainWorktree}`,
  ]);
}

const branch = git(['rev-parse', '--abbrev-ref', 'HEAD'], { cwd: repoRoot });
if (branch !== 'develop') {
  fail('Final packaging must be run from the develop branch.', [
    `Current branch: ${branch}`,
  ]);
}

const status = git(['status', '--porcelain', '--untracked-files=all'], {
  cwd: repoRoot,
});

if (status) {
  fail('The main repository checkout has uncommitted changes.', status.split('\n'));
}

git(['fetch', 'origin', 'develop', '--quiet'], {
  cwd: repoRoot,
  stdio: ['ignore', 'ignore', 'pipe'],
});

const localHead = git(['rev-parse', 'HEAD'], { cwd: repoRoot });
const remoteHead = git(['rev-parse', 'origin/develop'], { cwd: repoRoot });

if (localHead !== remoteHead) {
  fail('Local develop is not aligned with origin/develop.', [
    `local HEAD: ${localHead}`,
    `origin/develop: ${remoteHead}`,
    'Pull or rebase the main checkout before packaging.',
  ]);
}

console.log(`Release preflight passed for develop @ ${localHead.slice(0, 7)}`);
