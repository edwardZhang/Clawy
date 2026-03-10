#!/usr/bin/env zx

import 'zx/globals';
import { execFileSync, spawn } from 'node:child_process';
import fs from 'node:fs/promises';
import syncFs from 'node:fs';
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

function runCommand(command, args, env) {
  const vsDevCmd = findVsDevCmd();
  const commandEnv = vsDevCmd ? loadWindowsBuildEnv(vsDevCmd, env) : env;

  return new Promise((resolve, reject) => {
    const child =
      process.platform === 'win32'
        ? spawn('cmd.exe', ['/d', '/s', '/c', `${command} ${args.map(quoteCmdArg).join(' ')}`], {
            cwd: repoRoot,
            env: commandEnv,
            stdio: 'inherit',
          })
        : spawn(command, args, {
            cwd: repoRoot,
            env: commandEnv,
            stdio: 'inherit',
            shell: false,
          });

    child.on('error', reject);
    child.on('close', (code) => {
      if (code === 0) {
        resolve();
        return;
      }

      reject(new Error(`Command failed with exit code ${code}: ${command} ${args.join(' ')}`));
    });
  });
}

function quoteCmdArg(arg) {
  return /[\s"]/u.test(arg) ? `"${arg.replace(/"/g, '""')}"` : arg;
}

function findVsDevCmd() {
  if (process.platform !== 'win32') {
    return null;
  }

  const candidates = [];
  const vswherePath = 'C:\\Program Files (x86)\\Microsoft Visual Studio\\Installer\\vswhere.exe';

  if (syncFs.existsSync(vswherePath)) {
    try {
      const installationPath = execFileSync(
        vswherePath,
        ['-latest', '-products', '*', '-requires', 'Microsoft.VisualStudio.Component.VC.Tools.x86.x64', '-property', 'installationPath'],
        { encoding: 'utf8' },
      ).trim();

      if (installationPath) {
        candidates.push(path.join(installationPath, 'Common7', 'Tools', 'VsDevCmd.bat'));
      }
    } catch {
      // Fall back to common installation paths below.
    }
  }

  candidates.push('C:\\Program Files\\Microsoft Visual Studio\\2022\\BuildTools\\Common7\\Tools\\VsDevCmd.bat');
  candidates.push('C:\\Program Files (x86)\\Microsoft Visual Studio\\2022\\BuildTools\\Common7\\Tools\\VsDevCmd.bat');

  return candidates.find((candidate) => syncFs.existsSync(candidate)) ?? null;
}

function loadWindowsBuildEnv(vsDevCmd, baseEnv) {
  if (process.platform !== 'win32') {
    return baseEnv;
  }

  const tempScriptPath = path.join(repoRoot, `.vsdevcmd-env-${process.pid}.cmd`);
  syncFs.writeFileSync(
    tempScriptPath,
    `@echo off\r\ncall "${vsDevCmd}" -no_logo >nul\r\nset\r\n`,
    'utf8',
  );

  let output;
  try {
    output = execFileSync('cmd.exe', ['/d', '/c', tempScriptPath], {
      cwd: repoRoot,
      env: baseEnv,
      encoding: 'utf8',
    });
  } finally {
    syncFs.rmSync(tempScriptPath, { force: true });
  }

  const mergedEnv = { ...baseEnv };
  for (const line of output.split(/\r?\n/u)) {
    const separatorIndex = line.indexOf('=');
    if (separatorIndex <= 0) {
      continue;
    }

    mergedEnv[line.slice(0, separatorIndex)] = line.slice(separatorIndex + 1);
  }

  return mergedEnv;
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
await runCommand(process.platform === 'win32' ? 'pnpm.cmd' : 'pnpm', args, env);
