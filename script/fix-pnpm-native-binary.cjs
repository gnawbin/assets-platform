#!/usr/bin/env node

/**
 * Repairs `Could not download the pnpm <version> binary: Invalid URL`.
 *
 * pnpm 12 ships a native executable. Corepack installs the pnpm wrapper without
 * its dependencies and runs no lifecycle scripts, so the wrapper
 * (`bin/pnpm.mjs`) downloads that executable from the configured npm registry
 * on first use, through `get-pnpm`. Two things break that download:
 *
 *  - A mirror that answers with a *relative* `dist.tarball` — Huawei Cloud
 *    returns `/@pnpm/exe.win32-x64/-/exe.win32-x64-12.15.0.tgz`, and
 *    `new URL()` on that throws, aborting every pnpm command before any of it
 *    runs (on Windows the abrupt exit then trips a libuv assertion in
 *    src\win\async.c).
 *  - A connection that dies mid-transfer. The executable is a ~25 MB tarball
 *    and `get-pnpm` neither resumes nor retries, so a slow or unstable link
 *    leaves `pnpm-native.<hash>.tgz` fragments next to the wrapper and answers
 *    `Could not download the pnpm <version> binary: ...`.
 *
 * This script repeats the download with an absolute tarball URL built from the
 * registry (resuming and retrying when a transfer is cut short, and falling
 * back to the public mirrors below), verifies the bytes against the integrity
 * npm published, and drops the executable where the wrapper looks for it:
 *
 *   <corepack home>/v1/pnpm/<version>/pnpm-native[.exe]
 *
 * It touches nothing inside the project, so it is safe to run any time.
 *
 * Usage:
 *   node script/fix-pnpm-native-binary.cjs [--force]
 *
 * `--force` re-downloads even when the executable is already in place.
 *
 * Environment:
 *   PNPM_NATIVE_BINARY_MIRROR  Comma-separated registry URLs to try before the
 *                              built-in mirrors, e.g. `https://registry.npmmirror.com`.
 *   COREPACK_NPM_REGISTRY      Registry to read the metadata and download from.
 *   COREPACK_HOME              Corepack cache, when it is not the default one.
 */

const { createHash } = require('crypto');
const { execFileSync, spawnSync } = require('child_process');
const fs = require('fs');
const os = require('os');
const path = require('path');
const { Readable } = require('stream');
const { pipeline } = require('stream/promises');

const PROJECT_ROOT = path.join(__dirname, '..');
// Mirrors corepack's own INSTALL_FOLDER_VERSION (v1).
const INSTALL_FOLDER_VERSION = 1;
const DEFAULT_REGISTRY = 'https://registry.npmjs.org';
const BINARY_NAME = process.platform === 'win32' ? 'pnpm-native.exe' : 'pnpm-native';
const PROXY_VARS = ['https_proxy', 'HTTPS_PROXY', 'http_proxy', 'HTTP_PROXY'];
// Tried last, after the configured registry: the bytes are still checked against
// the integrity npm published, so a mirror can only ever fail that check.
const FALLBACK_MIRRORS = ['https://registry.npmmirror.com'];
// A truncated transfer is resumed from what already arrived, so a slow link only
// has to deliver the missing tail.
const MAX_ATTEMPTS = 8;
const RETRY_DELAY_MS = 2_000;
const DOWNLOAD_TIMEOUT_MS = 5 * 60_000;

const colors = {
  reset: '\x1b[0m',
  red: '\x1b[31m',
  green: '\x1b[32m',
  yellow: '\x1b[33m',
  blue: '\x1b[34m',
  cyan: '\x1b[36m'
};

function log(message, color = 'reset') {
  console.log(`${colors[color]}${message}${colors.reset}`);
}

const info = (message) => log(`ℹ️  ${message}`, 'blue');
const step = (message) => log(`🔧 ${message}`, 'cyan');
const ok = (message) => log(`✅ ${message}`, 'green');
const warn = (message) => log(`⚠️  ${message}`, 'yellow');
const fail = (message) => log(`❌ ${message}`, 'red');

/** The `packageManager` pin, which is the pnpm the wrappers will run. */
function packageManagerEntry() {
  const manifestPath = path.join(PROJECT_ROOT, 'package.json');
  const { packageManager } = JSON.parse(fs.readFileSync(manifestPath, 'utf8'));
  if (typeof packageManager !== 'string' || !packageManager.startsWith('pnpm@')) {
    throw new Error(`${manifestPath} does not pin pnpm through "packageManager".`);
  }
  // `pnpm@12.15.0+sha512.…`
  const version = packageManager.slice('pnpm@'.length).split('+')[0];
  return { version, major: Number(version.split('.')[0]) };
}

/** corepack's getCorepackHomeFolder(), so both agree on where the wrapper is. */
function corepackHome() {
  if (process.env.COREPACK_HOME) {
    return process.env.COREPACK_HOME;
  }
  const base =
    process.env.XDG_CACHE_HOME ??
    process.env.LOCALAPPDATA ??
    path.join(os.homedir(), process.platform === 'win32' ? 'AppData/Local' : '.cache');
  return path.join(base, 'node', 'corepack');
}

// Every target pnpm 12 publishes an executable for (see @pnpm/exe.* packages).
const V12_TARGETS = new Set([
  'win32-x64',
  'win32-arm64',
  'darwin-x64',
  'darwin-arm64',
  'linux-x64',
  'linux-arm64',
  'linux-riscv64',
  'linux-ppc64',
  'linux-s390x',
  'linux-x64-musl',
  'linux-arm64-musl',
  'freebsd-x64'
]);

/**
 * Name of the npm package that carries the executable, following pnpm 12's
 * scheme.
 */
function platformPackageName() {
  const { platform, arch } = process;
  // glibc builds report a `glibcVersionRuntime`; musl builds leave it unset.
  const musl =
    platform === 'linux' && V12_TARGETS.has(`${platform}-${arch}-musl`) && safeGlibcVersion() === '';
  const target = `${platform}-${arch}${musl ? '-musl' : ''}`;
  if (!V12_TARGETS.has(target)) {
    throw new Error(`pnpm ships no prebuilt executable for ${platform}-${arch}.`);
  }
  return { name: `@pnpm/exe.${target}`, binFile: platform === 'win32' ? 'pnpm.exe' : 'pnpm' };
}

function safeGlibcVersion() {
  try {
    return process.report?.getReport().header.glibcVersionRuntime ?? '';
  } catch {
    return '';
  }
}

/** The registry the download comes from, resolved like pnpm's own launcher. */
function resolveRegistry() {
  if (process.env.COREPACK_NPM_REGISTRY) {
    return normalizeRegistry(process.env.COREPACK_NPM_REGISTRY);
  }
  const npmrc = {
    ...readNpmrc(path.join(os.homedir(), '.npmrc')),
    ...readNpmrc(path.join(PROJECT_ROOT, '.npmrc'))
  };
  const registry =
    npmrc['@pnpm:registry'] ||
    process.env.npm_config_registry ||
    process.env.NPM_CONFIG_REGISTRY ||
    npmrc.registry;
  return normalizeRegistry(registry || DEFAULT_REGISTRY);
}

/** A registry URL usable as a base for a relative path. */
function normalizeRegistry(registry) {
  return registry.endsWith('/') ? registry : `${registry}/`;
}

/** Top-level `key=value` settings of an `.npmrc`, `${VAR}` placeholders expanded. */
function readNpmrc(file) {
  const settings = Object.create(null);
  let text;
  try {
    text = fs.readFileSync(file, 'utf8');
  } catch {
    return settings;
  }
  for (const rawLine of text.split(/\r?\n/)) {
    const line = rawLine.trim();
    if (line.startsWith('[')) {
      break;
    }
    const separator = line.indexOf('=');
    if (separator === -1 || line.startsWith(';') || line.startsWith('#')) {
      continue;
    }
    const key = line.slice(0, separator).trim();
    const value = line
      .slice(separator + 1)
      .trim()
      .replace(/\$\{([^${}]+)\}/g, (_, name) => process.env[name] ?? '');
    settings[key] = value;
  }
  return settings;
}

/** Node only honours the proxy variables on its own from 24.14: opt in when we can. */
function enableProxyFromEnv() {
  if (!PROXY_VARS.some((name) => process.env[name])) {
    return;
  }
  try {
    require('http').setGlobalProxyFromEnv?.(process.env);
  } catch {
    // A malformed proxy URL is the user's to fix; the download reports it.
  }
}

async function fetchJson(url) {
  const response = await fetch(url, {
    headers: { accept: 'application/json' },
    signal: AbortSignal.timeout(30_000)
  });
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText} from ${url}`);
  }
  return response.json();
}

async function fetchBytes(url, tempPath) {
  for (let attempt = 1; ; attempt++) {
    try {
      await downloadTo(url, tempPath);
      return fs.readFileSync(tempPath);
    } catch (err) {
      if (attempt >= MAX_ATTEMPTS) {
        throw new Error(`${err.message} (gave up after ${attempt} attempts)`);
      }
      const received = fileSize(tempPath);
      warn(
        `Interrupted after ${formatBytes(received)}: ${err.message} — ` +
          `retrying in ${RETRY_DELAY_MS / 1_000}s${received > 0 ? ' from what already arrived' : ''}...`
      );
      await sleep(RETRY_DELAY_MS);
    }
  }
}

/**
 * Streams `url` into `tempPath`, picking up where the file left off. A server
 * that answers a `Range` request with `200` ignored it, so the file is
 * restarted rather than appended to twice.
 */
async function downloadTo(url, tempPath) {
  const startSize = fileSize(tempPath);
  const headers = startSize > 0 ? { range: `bytes=${startSize}-` } : {};
  const response = await fetch(url, { headers, signal: AbortSignal.timeout(DOWNLOAD_TIMEOUT_MS) });
  // 416: the file already holds every byte the server has.
  if (response.status === 416) {
    return;
  }
  if (!response.ok) {
    throw new Error(`${response.status} ${response.statusText} from ${url}`);
  }
  if (response.body == null) {
    throw new Error(`${url} answered with no body.`);
  }

  const resumed = startSize > 0 && response.status === 206;
  const declared = Number(response.headers.get('content-length') ?? 0);
  const expected = declared > 0 ? declared + (resumed ? startSize : 0) : 0;
  const body = Readable.fromWeb(response.body);
  let received = resumed ? startSize : 0;
  let nextMilestone = 10;
  const startedAt = Date.now();

  body.on('data', (chunk) => {
    received += chunk.length;
    if (expected === 0) {
      return;
    }
    const percent = Math.floor((received / expected) * 100);
    if (percent >= nextMilestone) {
      const elapsed = Math.max((Date.now() - startedAt) / 1_000, 1);
      const speed = formatBytes((received - (resumed ? startSize : 0)) / elapsed);
      info(`${percent}% — ${formatBytes(received)} of ${formatBytes(expected)} (${speed}/s)`);
      nextMilestone = percent + 10;
    }
  });

  await pipeline(body, fs.createWriteStream(tempPath, { flags: resumed ? 'a' : 'w' }));

  if (expected > 0 && received < expected) {
    throw new Error(`the transfer stopped at ${formatBytes(received)} of ${formatBytes(expected)}`);
  }
}

function fileSize(file) {
  return fs.statSync(file, { throwIfNoEntry: false })?.size ?? 0;
}

function sleep(ms) {
  return new Promise((resolve) => setTimeout(resolve, ms));
}

function formatBytes(bytes) {
  if (bytes >= 1024 * 1024) {
    return `${(bytes / (1024 * 1024)).toFixed(1)} MB`;
  }
  if (bytes >= 1024) {
    return `${(bytes / 1024).toFixed(1)} KB`;
  }
  return `${bytes} B`;
}

/**
 * Absolute download URLs, most preferred first: the canonical npm layout on the
 * configured registry (mirrors serve it, and it keeps a relative `dist.tarball`
 * out of the picture), then whatever absolute URL the registry advertises, then
 * the fallback mirrors.
 */
function tarballCandidates(registry, packageName, distTarball) {
  const fileName = path.posix.basename(new URL(distTarball, 'https://registry.invalid').pathname);
  const candidates = [];
  const add = (url) => {
    if (url != null && !candidates.includes(url.href)) {
      candidates.push(url.href);
    }
  };

  add(new URL(`${packageName}/-/${fileName}`, registry));
  try {
    add(new URL(distTarball));
  } catch {
    // Relative: the canonical URL above covers it.
  }
  for (const mirror of mirrorRegistries(registry)) {
    add(new URL(`${packageName}/-/${fileName}`, mirror));
  }
  return candidates;
}

/** Registries to fall back to, minus the one already being tried. */
function mirrorRegistries(registry) {
  const configured = (process.env.PNPM_NATIVE_BINARY_MIRROR ?? '')
    .split(',')
    .map((entry) => entry.trim())
    .filter((entry) => entry !== '')
    .map(normalizeRegistry);
  return [...configured, ...FALLBACK_MIRRORS.map(normalizeRegistry)].filter(
    (mirror) => mirror !== registry
  );
}

/** The digest the registry published for the tarball. */
function integrityDigest(meta) {
  const entry = String(meta.dist?.integrity ?? '').trim().split(/\s+/)[0];
  const separator = entry.indexOf('-');
  if (separator === -1) {
    throw new Error(`The registry published no usable integrity for ${meta.dist?.tarball}.`);
  }
  return { algorithm: entry.slice(0, separator), expected: entry.slice(separator + 1) };
}

/** Reads the executable out of the platform tarball. */
function extractExecutable(tarball, binFile) {
  const tempDir = fs.mkdtempSync(path.join(os.tmpdir(), 'pnpm-native-'));
  const archivePath = path.join(tempDir, 'package.tgz');
  try {
    fs.writeFileSync(archivePath, tarball);
    // `tar` ships with Windows 10+, macOS and Linux alike; only one member is needed.
    execFileSync('tar', ['-xzf', archivePath, '-C', tempDir, `package/${binFile}`], {
      stdio: 'inherit'
    });
    const extracted = path.join(tempDir, 'package', binFile);
    if (!fs.existsSync(extracted)) {
      throw new Error(`The tarball does not contain package/${binFile}.`);
    }
    return fs.readFileSync(extracted);
  } finally {
    fs.rmSync(tempDir, { recursive: true, force: true });
  }
}

/** Writes the executable atomically, so a half-written file is never spawned. */
function placeExecutable(executable, destPath) {
  const tempPath = `${destPath}.pnpm-tmp`;
  try {
    fs.writeFileSync(tempPath, executable);
    if (process.platform !== 'win32') {
      fs.chmodSync(tempPath, 0o755);
    }
    fs.renameSync(tempPath, destPath);
  } catch (err) {
    fs.rmSync(tempPath, { force: true });
    throw new Error(`Could not install the executable at ${destPath}: ${err.message}`);
  }
}

async function main() {
  const force = process.argv.includes('--force');
  const { version, major } = packageManagerEntry();
  if (major < 12) {
    info(`pnpm ${version} is pure JavaScript, so there is no native executable to repair.`);
    return 0;
  }

  const { name, binFile } = platformPackageName();
  const wrapperDir = path.join(corepackHome(), `v${INSTALL_FOLDER_VERSION}`, 'pnpm', version);
  const destPath = path.join(wrapperDir, BINARY_NAME);

  if (!fs.existsSync(path.join(wrapperDir, 'bin', 'pnpm.mjs'))) {
    warn(`Corepack has not unpacked the pnpm ${version} wrapper yet: ${wrapperDir}`);
    info('Run `pnpm -v` once — it fails with this very error but leaves the wrapper behind — then rerun this script.');
    return 1;
  }
  if (fs.existsSync(destPath) && !force) {
    ok(`Already in place: ${destPath}`);
    info('Pass --force to download it again.');
    return 0;
  }

  const registry = resolveRegistry();
  info(`Wrapper: ${wrapperDir}`);
  info(`Registry: ${registry}`);
  enableProxyFromEnv();

  step(`Reading metadata for ${name}@${version}...`);
  const meta = await fetchJson(new URL(`${name}/${version}`, registry).href);
  const { algorithm, expected } = integrityDigest(meta);

  step('Downloading the executable...');
  // A stable name, so a run that has to be interrupted resumes where it stopped.
  const tempPath = path.join(
    os.tmpdir(),
    `pnpm-native-${version}-${process.platform}-${process.arch}.part`
  );
  let tarball;
  const failures = [];
  try {
    for (const url of tarballCandidates(registry, name, meta.dist.tarball)) {
      try {
        info(url);
        tarball = await fetchBytes(url, tempPath);
      } catch (err) {
        failures.push(`  ${url}\n    ${err.message}`);
        fs.rmSync(tempPath, { force: true });
        continue;
      }
      step(`Verifying the ${algorithm} integrity...`);
      const actual = createHash(algorithm).update(tarball).digest('base64');
      if (actual === expected) {
        break;
      }
      failures.push(`  ${url}\n    the bytes do not match the published checksum (${actual})`);
      tarball = undefined;
      // Restarting from a clean file, so the next source is not resumed onto these bytes.
      fs.rmSync(tempPath, { force: true });
    }
  } finally {
    fs.rmSync(tempPath, { force: true });
  }
  if (tarball == null) {
    throw new Error(`Every download source failed:\n${failures.join('\n')}`);
  }
  ok(`${formatBytes(tarball.length)} match ${algorithm}-${expected.slice(0, 12)}...`);

  step('Extracting...');
  const executable = extractExecutable(tarball, binFile);

  step(`Installing ${destPath}...`);
  placeExecutable(executable, destPath);

  const check = spawnSync(destPath, ['--version'], { encoding: 'utf8' });
  const reported = (check.stdout ?? '').trim();
  if (check.status !== 0 || reported === '') {
    throw new Error(
      `The installed executable does not run: ${check.error?.message ?? check.stderr ?? `exit ${check.status}`}`
    );
  }
  if (reported !== version) {
    warn(`It reports ${reported}, not the pinned ${version}.`);
  }
  ok(`pnpm ${reported} is ready — pnpm no longer downloads an executable on startup.`);
  return 0;
}

main().then(
  (code) => {
    process.exitCode = code;
  },
  (err) => {
    fail(err.message);
    process.exitCode = 1;
  }
);

