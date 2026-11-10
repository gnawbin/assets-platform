#!/usr/bin/env node

/**
 * dev-clean.cjs —— 清理「挂起 / 僵死」的 Tauri + Next.js 开发进程
 *
 * 背景：开发进程被 Ctrl+Z（SIGTSTP）或被终端/IDE 挂起后忘记恢复时，进程会停在
 * T（stopped）状态，但**依然持有** :1480 的监听套接字和 ~/.cargo/.package-cache 的
 * 文件锁。于是下一次 `pnpm tauri-dev` 必然失败：
 *
 *     Blocking waiting for file lock on package cache
 *     Error: listen EADDRINUSE: address already in use :::1480
 *     The "beforeDevCommand" terminated with a non-zero status code.
 *
 * 用法：
 *     pnpm dev:kill                        # 清理本项目残留的开发进程
 *     node script/dev-clean.cjs --dry-run   # 只列出将要清理的进程
 *
 * 说明：
 *   - 停止（T）状态的进程收不到 SIGTERM，所以先发 SIGCONT 再发 SIGTERM；
 *   - 只处理与本仓库相关的进程（tauri dev 链 / 本端口上的 next dev / cwd 在仓库内的 cargo），
 *     不会误杀其他项目；
 *   - 脚本不改动仓库里的任何文件，可随时重复执行。
 */

const { execSync } = require('child_process');
const fs = require('fs');
const path = require('path');
const os = require('os');

// ANSI color codes for better output
const colors = {
    reset: '\x1b[0m',
    bright: '\x1b[1m',
    red: '\x1b[31m',
    green: '\x1b[32m',
    yellow: '\x1b[33m',
    blue: '\x1b[34m',
    cyan: '\x1b[36m',
};

function log(message, color = 'reset') {
    console.log(`${colors[color]}${message}${colors.reset}`);
}
const logSuccess = message => log(`✅ ${message}`, 'green');
const logError = message => log(`❌ ${message}`, 'red');
const logWarning = message => log(`⚠️  ${message}`, 'yellow');
const logInfo = message => log(`ℹ️  ${message}`, 'blue');
const logStep = message => log(`🔧 ${message}`, 'cyan');

const repoRoot = path.resolve(__dirname, '..');
const dryRun = process.argv.includes('--dry-run');
const TERM_TIMEOUT_MS = 5000;

function sh(command) {
    try {
        return execSync(command, { encoding: 'utf8', stdio: ['ignore', 'pipe', 'ignore'] }).trim();
    } catch {
        return '';
    }
}

/** 端口取自 apps/web 的 dev 脚本，取不到再退回 1480 */
function resolveDevPort() {
    if (process.env.DEV_PORT) return Number(process.env.DEV_PORT);
    try {
        const pkg = JSON.parse(fs.readFileSync(path.join(repoRoot, 'apps/web/package.json'), 'utf8'));
        const match = /(?:-p|--port)\s+(\d+)/.exec((pkg.scripts && pkg.scripts.dev) || '');
        if (match) return Number(match[1]);
    } catch {
        // 读不到就用默认值
    }
    return 1480;
}

const PORT = resolveDevPort();

/** Linux 下读取进程工作目录，用于把 cargo 进程限定在本仓库内 */
function cwdOf(pid) {
    try {
        return fs.readlinkSync(`/proc/${pid}/cwd`);
    } catch {
        return null;
    }
}

function listProcesses() {
    return sh('ps -eo pid=,ppid=,stat=,command=')
        .split('\n')
        .map(line => {
            const match = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(.*)$/.exec(line);
            if (!match) return null;
            return { pid: Number(match[1]), ppid: Number(match[2]), state: match[3], args: match[4] };
        })
        .filter(Boolean);
}

function isTarget(proc) {
    const { pid, args } = proc;
    if (pid === process.pid || pid === process.ppid) return false;
    if (args.includes('dev-clean.cjs')) return false;

    // tauri dev 主链：pnpm tauri-dev / tauri.js dev
    if (/tauri(\.js)?\s+dev\b/.test(args)) return true;

    // tauri.conf.json 的 beforeDevCommand 起出来的 Next
    if (new RegExp(`(?:-p|--port)\\s+${PORT}\\b`).test(args) && /next/.test(args)) return true;
    if (/pnpm\s+--prefix\s+\.\.\/web\s+dev\b/.test(args)) return true;

    // next dev 派生出来的 next-server（命令行里没有端口，只能按名字认）
    if (/^next-server\b/.test(args)) return true;

    // 本仓库内的 cargo run/build/metadata（含被卡在包缓存锁上的孤儿）
    if (/\bcargo\b/.test(args) && /\b(run|build|metadata)\b/.test(args)) {
        if (inRepo(pid, args)) return true;
    }

    // tauri dev 编译出来的开发二进制（target/debug/assets-platform 等）
    if (/target[/\\]debug[/\\]/.test(args) && inRepo(pid, args)) return true;

    return false;
}

/** 进程是否属于本仓库（用工作目录或命令行里的绝对路径判断） */
function inRepo(pid, args) {
    if (args.includes(repoRoot)) return true;
    const cwd = cwdOf(pid);
    return Boolean(cwd) && (cwd === repoRoot || cwd.startsWith(`${repoRoot}${path.sep}`));
}

/** 把命中进程的所有子孙进程一并纳入（如 next-server、开发二进制），避免只杀父进程留下孤儿 */
function expandDescendants(processes, roots) {
    const byParent = new Map();
    for (const proc of processes) {
        if (!byParent.has(proc.ppid)) byParent.set(proc.ppid, []);
        byParent.get(proc.ppid).push(proc);
    }

    const seen = new Set([process.pid, process.ppid]);
    const all = [];
    const queue = [...roots];
    for (const root of roots) {
        if (seen.has(root.pid)) continue;
        seen.add(root.pid);
        all.push(root);
    }

    while (queue.length > 0) {
        const current = queue.shift();
        for (const child of byParent.get(current.pid) || []) {
            if (seen.has(child.pid)) continue;
            seen.add(child.pid);
            all.push(child);
            queue.push(child);
        }
    }

    return all;
}

function isAlive(pid) {
    try {
        process.kill(pid, 0);
        return true;
    } catch {
        return false;
    }
}

function signal(pid, sig) {
    try {
        process.kill(pid, sig);
        return true;
    } catch {
        return false;
    }
}

/** 端口是否仍被监听（ss → lsof → netstat 逐级降级） */
function portListeners() {
    const ss = sh(`ss -ltnp 2>/dev/null | grep ':${PORT} '`);
    if (ss) return ss;
    const lsof = sh(`lsof -nP -iTCP:${PORT} -sTCP:LISTEN 2>/dev/null`);
    if (lsof) return lsof;
    return sh(`netstat -ltnp 2>/dev/null | grep ':${PORT} '`);
}

/** Cargo 包缓存锁的持有者 */
function cargoLockHolders() {
    const cacheFile = path.join(os.homedir(), '.cargo', '.package-cache');
    const out = sh(`fuser ${cacheFile} 2>/dev/null`);
    return out ? out.split(/\s+/).filter(Boolean) : [];
}

function sleep(ms) {
    return new Promise(resolve => setTimeout(resolve, ms));
}

function reportResources() {
    const listeners = portListeners();
    if (listeners) {
        logError(`端口 ${PORT} 仍被占用：\n${listeners}`);
    } else {
        logSuccess(`端口 ${PORT} 已释放`);
    }

    const holders = cargoLockHolders();
    if (holders.length === 0) {
        logSuccess('Cargo 包缓存锁已释放');
    } else {
        logWarning(`Cargo 包缓存锁仍被持有：${holders.join(', ')}（执行 cargo 时会打印 Blocking waiting for file lock）`);
    }

    logInfo('清理完成后重新启动：pnpm tauri-dev');
}

async function main() {
    logStep(`仓库：${repoRoot}`);
    logStep(`前端端口：${PORT}${dryRun ? '（dry-run，不会真的结束进程）' : ''}`);

    const processes = listProcesses();
    const roots = processes.filter(isTarget);
    const targets = expandDescendants(processes, roots);
    if (targets.length === 0) {
        logSuccess('没有发现残留的 Tauri / Next.js 开发进程');
        reportResources();
        return;
    }

    logWarning(`发现 ${targets.length} 个相关进程（${roots.length} 个入口 + ${targets.length - roots.length} 个子进程）：`);
    for (const proc of targets) {
        const state = proc.state.includes('T') ? `${proc.state}（已挂起）` : proc.state;
        log(`   pid=${proc.pid} ppid=${proc.ppid} state=${state} ${colors.bright}${proc.args.slice(0, 110)}${colors.reset}`);
    }

    if (dryRun) {
        reportResources();
        return;
    }

    // 先 SIGCONT（挂起的进程收不到 SIGTERM），再按 pid 倒序 SIGTERM（子进程优先）
    const ordered = [...targets].sort((a, b) => b.pid - a.pid);
    for (const proc of ordered) signal(proc.pid, 'SIGCONT');
    for (const proc of ordered) signal(proc.pid, 'SIGTERM');

    const deadline = Date.now() + TERM_TIMEOUT_MS;
    let survivors = ordered.filter(proc => isAlive(proc.pid));
    while (survivors.length > 0 && Date.now() < deadline) {
        await sleep(200);
        survivors = ordered.filter(proc => isAlive(proc.pid));
    }

    if (survivors.length > 0) {
        logWarning(`SIGTERM 超时，仍有 ${survivors.length} 个进程存活，改用 SIGKILL`);
        for (const proc of survivors) {
            if (signal(proc.pid, 'SIGCONT')) signal(proc.pid, 'SIGKILL');
        }
        await sleep(500);
        survivors = survivors.filter(proc => isAlive(proc.pid));
        if (survivors.length > 0) {
            logError(`SIGKILL 后仍存活：${survivors.map(proc => proc.pid).join(', ')}`);
        }
    }

    logSuccess(`已结束 ${ordered.length - survivors.length} 个进程${survivors.length ? `，${survivors.length} 个未能结束` : ''}`);
    reportResources();
}

main().catch(error => {
    logError(error && error.stack ? error.stack : String(error));
    process.exitCode = 1;
});
