import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { dirname, join } from 'node:path';
import { fileURLToPath } from 'node:url';

const MCP_SDK_PACKAGE = '@modelcontextprotocol/sdk';

interface VerifyResult {
  ok: boolean;
  errors: string[];
  warnings: string[];
}

function readJson(path: string): Record<string, unknown> | null {
  try {
    return JSON.parse(readFileSync(path, 'utf8')) as Record<string, unknown>;
  } catch {
    return null;
  }
}

function getDeclaredSdkRange(pkg: Record<string, unknown> | null): string | null {
  if (!pkg) return null;
  const sections = ['dependencies', 'devDependencies', 'peerDependencies'] as const;
  for (const section of sections) {
    const deps = pkg[section];
    if (deps && typeof deps === 'object') {
      const range = (deps as Record<string, unknown>)[MCP_SDK_PACKAGE];
      if (typeof range === 'string') return range;
    }
  }
  return null;
}

function getInstalledSdkVersion(packageRoot: string): string | null {
  const sdkPkg = readJson(join(packageRoot, 'node_modules', MCP_SDK_PACKAGE, 'package.json'));
  if (sdkPkg && typeof sdkPkg.version === 'string') return sdkPkg.version;
  return null;
}

function normalizeVersion(version: string): string {
  return version.trim().replace(/^[~^=v]+/, '');
}

function majorOf(version: string): string | null {
  const match = normalizeVersion(version).match(/^(\d+)/);
  return match ? match[1] : null;
}

function checkMcpSdkVersion(packageRoot: string, result: VerifyResult): void {
  const pkg = readJson(join(packageRoot, 'package.json'));
  const declaredRange = getDeclaredSdkRange(pkg);
  const installedVersion = getInstalledSdkVersion(packageRoot);

  if (!declaredRange) {
    result.warnings.push(
      `No ${MCP_SDK_PACKAGE} dependency declared in package.json; skipping MCP SDK version check.`,
    );
    return;
  }

  if (!installedVersion) {
    result.errors.push(
      `MCP SDK (${MCP_SDK_PACKAGE}) is declared (${declaredRange}) but not installed. Run npm install.`,
    );
    return;
  }

  const declaredMajor = majorOf(declaredRange);
  const installedMajor = majorOf(installedVersion);

  if (declaredMajor && installedMajor && declaredMajor !== installedMajor) {
    result.errors.push(
      `MCP SDK version mismatch: package.json declares ${MCP_SDK_PACKAGE}@${declaredRange} ` +
        `but installed version is ${installedVersion}. A stale SDK may expose an incompatible ` +
        `ListTools shape. Reinstall dependencies to match the declared version.`,
    );
  }
}

export function verifyInstall(packageRoot?: string): VerifyResult {
  const root = packageRoot ?? join(dirname(fileURLToPath(import.meta.url)), '..');
  const result: VerifyResult = { ok: true, errors: [], warnings: [] };

  try {
    const nodeVersion = execFileSync('node', ['--version'], { encoding: 'utf8' }).trim();
    const nodeMajor = Number(nodeVersion.replace(/^v/, '').split('.')[0]);
    if (!Number.isFinite(nodeMajor) || nodeMajor < 18) {
      result.errors.push(`Node.js ${nodeVersion} is too old; >=18 is required.`);
    }
  } catch {
    result.errors.push('Unable to determine Node.js version.');
  }

  try {
    execFileSync('npm', ['--version'], { encoding: 'utf8' });
  } catch {
    result.errors.push('Unable to determine npm version.');
  }

  checkMcpSdkVersion(root, result);

  result.ok = result.errors.length === 0;
  return result;
}
