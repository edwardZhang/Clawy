#!/usr/bin/env node

import fs from 'node:fs/promises';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const repoRoot = path.resolve(fileURLToPath(new URL('..', import.meta.url)));

export const VERSION_FILES = {
  packageJson: path.join(repoRoot, 'package.json'),
  cargoToml: path.join(repoRoot, 'src-tauri', 'Cargo.toml'),
  tauriConf: path.join(repoRoot, 'src-tauri', 'tauri.conf.json'),
};

function parseSemver(value) {
  const match = String(value).trim().match(
    /^(\d+)\.(\d+)\.(\d+)(?:-([0-9A-Za-z.-]+))?(?:\+([0-9A-Za-z.-]+))?$/,
  );
  if (!match) {
    throw new Error(`Invalid version: ${value}`);
  }

  return {
    major: Number.parseInt(match[1], 10),
    minor: Number.parseInt(match[2], 10),
    patch: Number.parseInt(match[3], 10),
    prerelease: match[4] ?? '',
    build: match[5] ?? '',
  };
}

function stringifySemver(version) {
  let value = `${version.major}.${version.minor}.${version.patch}`;
  if (version.prerelease) value += `-${version.prerelease}`;
  if (version.build) value += `+${version.build}`;
  return value;
}

export function resolveNextVersion(currentVersion, releaseTypeOrVersion = 'patch') {
  const input = String(releaseTypeOrVersion).trim();
  if (!input) {
    return resolveNextVersion(currentVersion, 'patch');
  }

  if (/^\d+\.\d+\.\d+(?:-[0-9A-Za-z.-]+)?(?:\+[0-9A-Za-z.-]+)?$/.test(input)) {
    parseSemver(input);
    return input;
  }

  const current = parseSemver(currentVersion);
  switch (input) {
    case 'patch':
      current.patch += 1;
      current.prerelease = '';
      current.build = '';
      return stringifySemver(current);
    case 'minor':
      current.minor += 1;
      current.patch = 0;
      current.prerelease = '';
      current.build = '';
      return stringifySemver(current);
    case 'major':
      current.major += 1;
      current.minor = 0;
      current.patch = 0;
      current.prerelease = '';
      current.build = '';
      return stringifySemver(current);
    default:
      throw new Error(`Unsupported version bump type: ${input}`);
  }
}

function replaceJsonVersion(contents, nextVersion) {
  const updated = contents.replace(
    /("version"\s*:\s*")([^"]+)(")/,
    `$1${nextVersion}$3`,
  );
  if (updated === contents) {
    throw new Error('Could not update JSON version field.');
  }
  return updated;
}

function replaceCargoVersion(contents, nextVersion) {
  const updated = contents.replace(
    /(\[package\][\s\S]*?\nversion\s*=\s*")([^"]+)(")/,
    `$1${nextVersion}$3`,
  );
  if (updated === contents) {
    throw new Error('Could not update Cargo package version field.');
  }
  return updated;
}

async function readPackageVersion() {
  const contents = await fs.readFile(VERSION_FILES.packageJson, 'utf8');
  return JSON.parse(contents).version;
}

async function readCargoVersion() {
  const contents = await fs.readFile(VERSION_FILES.cargoToml, 'utf8');
  const match = contents.match(/^\[package\][\s\S]*?^version\s*=\s*"([^"]+)"/m);
  if (!match) {
    throw new Error('Could not read version from src-tauri/Cargo.toml');
  }
  return match[1];
}

async function readTauriVersion() {
  const contents = await fs.readFile(VERSION_FILES.tauriConf, 'utf8');
  return JSON.parse(contents).version;
}

export async function readVersionState() {
  const [packageVersion, cargoVersion, tauriVersion] = await Promise.all([
    readPackageVersion(),
    readCargoVersion(),
    readTauriVersion(),
  ]);

  return {
    packageJson: packageVersion,
    cargoToml: cargoVersion,
    tauriConf: tauriVersion,
  };
}

export async function assertVersionsAligned() {
  const versions = await readVersionState();
  const distinct = [...new Set(Object.values(versions))];
  if (distinct.length !== 1) {
    const details = Object.entries(versions)
      .map(([file, version]) => `${file}: ${version}`)
      .join('\n');
    throw new Error(`Version files are not aligned:\n${details}`);
  }
  return distinct[0];
}

export async function writeVersion(nextVersion) {
  parseSemver(nextVersion);

  const [packageJsonContents, cargoTomlContents, tauriConfContents] = await Promise.all([
    fs.readFile(VERSION_FILES.packageJson, 'utf8'),
    fs.readFile(VERSION_FILES.cargoToml, 'utf8'),
    fs.readFile(VERSION_FILES.tauriConf, 'utf8'),
  ]);

  await Promise.all([
    fs.writeFile(
      VERSION_FILES.packageJson,
      replaceJsonVersion(packageJsonContents, nextVersion),
      'utf8',
    ),
    fs.writeFile(
      VERSION_FILES.cargoToml,
      replaceCargoVersion(cargoTomlContents, nextVersion),
      'utf8',
    ),
    fs.writeFile(
      VERSION_FILES.tauriConf,
      replaceJsonVersion(tauriConfContents, nextVersion),
      'utf8',
    ),
  ]);

  return nextVersion;
}

export function formatVersionState(state) {
  return [
    `package.json: ${state.packageJson}`,
    `src-tauri/Cargo.toml: ${state.cargoToml}`,
    `src-tauri/tauri.conf.json: ${state.tauriConf}`,
  ].join('\n');
}

