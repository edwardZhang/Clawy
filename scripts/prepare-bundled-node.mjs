#!/usr/bin/env zx

import 'zx/globals';

const ROOT_DIR = path.resolve(__dirname, '..');
const OUTPUT_BASE = path.join(ROOT_DIR, 'resources', 'bin');

const TARGETS = {
  'darwin-arm64': 'node',
  'darwin-x64': 'node',
  'win32-arm64': 'node.exe',
  'win32-x64': 'node.exe',
  'linux-arm64': 'node',
  'linux-x64': 'node',
};

async function main() {
  const currentTarget = `${os.platform()}-${os.arch()}`;
  const binName = TARGETS[currentTarget];
  if (!binName) {
    echo(chalk.red(`Unsupported Node bundling target: ${currentTarget}`));
    process.exit(1);
  }

  const sourceNode = process.execPath;
  const targetDir = path.join(OUTPUT_BASE, currentTarget);
  const targetNode = path.join(targetDir, binName);

  echo(chalk.cyan(`Bundling Node runtime for ${currentTarget}`));
  echo(`  source: ${sourceNode}`);
  echo(`  target: ${targetNode}`);

  await fs.ensureDir(targetDir);
  await fs.copy(sourceNode, targetNode, { overwrite: true });

  if (os.platform() !== 'win32') {
    await fs.chmod(targetNode, 0o755);
  }

  echo(chalk.green(`Bundled Node runtime at ${targetNode}`));
}

await main();
