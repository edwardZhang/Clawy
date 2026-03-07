#!/usr/bin/env zx

import 'zx/globals';

const ROOT_DIR = path.resolve(__dirname, '..');
const PACKAGE_NAME = 'clawhub';
const OUTPUT_ROOT = path.join(ROOT_DIR, 'build', 'clawhub');

function getVirtualStoreNodeModules(realPkgPath) {
  let dir = realPkgPath;
  while (dir !== path.dirname(dir)) {
    if (path.basename(dir) === 'node_modules') {
      return dir;
    }
    dir = path.dirname(dir);
  }
  return null;
}

async function listPackages(nodeModulesDir) {
  const result = [];
  if (!(await fs.pathExists(nodeModulesDir))) {
    return result;
  }

  const entries = await fs.readdir(nodeModulesDir);
  for (const entry of entries) {
    if (entry === '.bin') {
      continue;
    }

    const fullPath = path.join(nodeModulesDir, entry);
    const stat = await fs.stat(fullPath).catch(() => null);
    if (!stat?.isDirectory()) {
      continue;
    }

    if (entry.startsWith('@')) {
      const scopedEntries = await fs.readdir(fullPath).catch(() => []);
      for (const scopedEntry of scopedEntries) {
        const scopedPath = path.join(fullPath, scopedEntry);
        const scopedStat = await fs.stat(scopedPath).catch(() => null);
        if (scopedStat?.isDirectory()) {
          result.push({ name: `${entry}/${scopedEntry}`, fullPath: scopedPath });
        }
      }
      continue;
    }

    result.push({ name: entry, fullPath });
  }

  return result;
}

async function main() {
  const sourcePkgPath = path.join(ROOT_DIR, 'node_modules', PACKAGE_NAME);
  if (!(await fs.pathExists(sourcePkgPath))) {
    echo(chalk.red(`Package not found: ${sourcePkgPath}`));
    process.exit(1);
  }

  const realPkgPath = await fs.realpath(sourcePkgPath);
  const rootVirtualNodeModules = getVirtualStoreNodeModules(realPkgPath);
  if (!rootVirtualNodeModules) {
    echo(chalk.red(`Failed to resolve virtual store node_modules for ${realPkgPath}`));
    process.exit(1);
  }

  echo(chalk.cyan(`Bundling ${PACKAGE_NAME} into ${OUTPUT_ROOT}`));

  await fs.remove(OUTPUT_ROOT);
  await fs.ensureDir(OUTPUT_ROOT);
  await fs.copy(realPkgPath, OUTPUT_ROOT, { dereference: true });

  const collected = new Map();
  const copiedNames = new Set();
  const queue = [{ nodeModulesDir: rootVirtualNodeModules, skipPkg: PACKAGE_NAME }];
  const skipPackages = new Set(['typescript', '@playwright/test']);

  while (queue.length > 0) {
    const { nodeModulesDir, skipPkg } = queue.shift();
    const packages = await listPackages(nodeModulesDir);
    for (const pkg of packages) {
      if (pkg.name === skipPkg || skipPackages.has(pkg.name) || pkg.name.startsWith('@types/')) {
        continue;
      }

      const realPath = await fs.realpath(pkg.fullPath).catch(() => null);
      if (!realPath || collected.has(realPath)) {
        continue;
      }

      collected.set(realPath, pkg.name);
      const depVirtualNodeModules = getVirtualStoreNodeModules(realPath);
      if (depVirtualNodeModules && depVirtualNodeModules !== nodeModulesDir) {
        queue.push({ nodeModulesDir: depVirtualNodeModules, skipPkg: pkg.name });
      }
    }
  }

  const destNodeModules = path.join(OUTPUT_ROOT, 'node_modules');
  await fs.ensureDir(destNodeModules);

  let copiedCount = 0;
  for (const [realPath, pkgName] of collected) {
    if (copiedNames.has(pkgName)) {
      continue;
    }

    copiedNames.add(pkgName);
    const destPath = path.join(destNodeModules, pkgName);
    await fs.ensureDir(path.dirname(destPath));
    await fs.copy(realPath, destPath, { dereference: true });
    copiedCount += 1;
  }

  echo(chalk.green(`Bundled ${PACKAGE_NAME} with ${copiedCount} transitive dependencies.`));
}

await main();
