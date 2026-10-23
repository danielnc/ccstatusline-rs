#!/usr/bin/env node
// Differential parity harness: runs every corpus case through upstream
// ccstatusline (node + preload.mjs) and through the Rust binary, and compares
// stdout bytes and exit codes. Cases run sequentially, one process at a time,
// and a machine-wide lock serializes concurrent harness invocations.
// See README.md for the case format.
import { spawnSync } from 'node:child_process';
import * as fs from 'node:fs';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const PARITY_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_DIR = path.resolve(PARITY_DIR, '..');
const UPSTREAM_DIST = path.join(PARITY_DIR, 'upstream', 'node_modules', 'ccstatusline', 'dist', 'ccstatusline.js');
const PRELOAD = path.join(PARITY_DIR, 'preload.mjs');
const FIXTURES_DIR = path.join(PARITY_DIR, 'fixtures');
const DEFAULT_CORPUS = path.join(PARITY_DIR, 'corpus');
const DEFAULT_RUST_BIN = path.join(REPO_DIR, 'target', 'release', 'ccstatusline-rs');
const LAST_REPORT = path.join(PARITY_DIR, 'last-report.json');

export const DEFAULT_NOW_MS = Date.UTC(2026, 9, 21, 12, 0, 0); // 2026-10-21T12:00:00Z
export const DEFAULT_MEM_TOTAL_BYTES = 16 * 1024 ** 3;
export const DEFAULT_MEM_FREE_BYTES = 5 * 1024 ** 3 + 300 * 1024 ** 2;
const DEFAULT_TIMEOUT_MS = 10_000;

// Only these system directories are on PATH, after the case's pathPrepend dirs
// and the harness's own bin dir (git + node links, failing stubs below).
const SYSTEM_PATH = ['/usr/bin', '/bin', '/usr/sbin', '/sbin'];
// Commands that must never do real work under the harness: network clients,
// the GitHub/GitLab CLIs (one shared token) and the macOS keychain. The stub
// shadows any copy in the system dirs; a case can still supply a fake through
// pathPrepend, which comes first.
const STUBBED_COMMANDS = ['gh', 'glab', 'curl', 'wget', 'security'];

const LOCK_DIR = process.env.CCSL_PARITY_LOCK_DIR ?? path.join(os.tmpdir(), 'ccsl-parity-run.lock');
const LOCK_STALE_MS = 10 * 60 * 1000;
const LOCK_RETRY_MS = 500;

// --- CLI -------------------------------------------------------------------

function usage() {
    return `usage: node parity/run.mjs [options]

  --rust-bin PATH     Rust binary (default ${path.relative(process.cwd(), DEFAULT_RUST_BIN) || DEFAULT_RUST_BIN})
  --filter SUBSTR     only cases whose name contains SUBSTR (repeatable, OR)
  --tag TAG           only cases carrying TAG (repeatable, OR)
  --show-diff         print both outputs and the first differing byte on failure
  --json PATH         also write the report to PATH (always written to parity/last-report.json)
  --upstream-only     run only upstream and print its output (exploration)
  --determinism       run upstream twice per case and compare the runs
  --corpus DIR        load cases from DIR instead of parity/corpus (repeatable, union)
  --tmp-root DIR      scratch root (default <os.tmpdir()>/ccsl-parity-<pid>)
  --keep-tmp          keep the scratch root afterwards
  --timeout MS        per-process timeout (default ${DEFAULT_TIMEOUT_MS})
  --fail-fast         stop at the first failure
  -q, --quiet         only print failures and the summary

Concurrent invocations wait for the lock ${LOCK_DIR}
(override with CCSL_PARITY_LOCK_DIR).`;
}

function parseArgs(argv) {
    const opts = {
        rustBin: DEFAULT_RUST_BIN,
        filters: [],
        tags: [],
        showDiff: false,
        jsonOut: null,
        upstreamOnly: false,
        determinism: false,
        corpora: [],
        tmpRoot: null,
        keepTmp: false,
        timeoutMs: DEFAULT_TIMEOUT_MS,
        failFast: false,
        quiet: false
    };
    const takeValue = (i, flag) => {
        const value = argv[i + 1];
        if (value === undefined) {
            throw new Error(`${flag} requires a value`);
        }
        return value;
    };
    for (let i = 0; i < argv.length; i += 1) {
        const arg = argv[i];
        switch (arg) {
            case '--rust-bin': opts.rustBin = path.resolve(takeValue(i, arg)); i += 1; break;
            case '--filter': opts.filters.push(takeValue(i, arg)); i += 1; break;
            case '--tag': opts.tags.push(takeValue(i, arg)); i += 1; break;
            case '--show-diff': opts.showDiff = true; break;
            case '--json': opts.jsonOut = path.resolve(takeValue(i, arg)); i += 1; break;
            case '--upstream-only': opts.upstreamOnly = true; break;
            case '--determinism': opts.determinism = true; break;
            case '--corpus': opts.corpora.push(path.resolve(takeValue(i, arg))); i += 1; break;
            case '--corpus-only': break; // accepted for old invocations: any --corpus already replaces the default
            case '--tmp-root': opts.tmpRoot = path.resolve(takeValue(i, arg)); i += 1; break;
            case '--keep-tmp': opts.keepTmp = true; break;
            case '--timeout': opts.timeoutMs = Number(takeValue(i, arg)); i += 1; break;
            case '--fail-fast': opts.failFast = true; break;
            case '-q':
            case '--quiet': opts.quiet = true; break;
            case '-h':
            case '--help': console.log(usage()); process.exit(0); break;
            default: throw new Error(`unknown argument: ${arg}\n\n${usage()}`);
        }
    }
    return opts;
}

// --- Corpus ----------------------------------------------------------------

function walkJson(dir) {
    const out = [];
    for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
        const full = path.join(dir, entry.name);
        if (entry.isDirectory()) {
            out.push(...walkJson(full));
        } else if (entry.isFile() && entry.name.endsWith('.json')) {
            out.push(full);
        }
    }
    return out;
}

// Case names default to the path relative to the nearest directory named
// `corpus` (so <worktree>/parity/corpus/<group>/x.json is "<group>/x"), and a
// `fixtures` dir beside that `corpus` dir is searched before parity/fixtures.
function corpusRoot(dir) {
    for (let d = dir; ; d = path.dirname(d)) {
        if (path.basename(d) === 'corpus') {
            return d;
        }
        if (path.dirname(d) === d) {
            return dir;
        }
    }
}

export function loadCases(corpusDirs) {
    const cases = [];
    const byName = new Map();
    const seenDirs = new Set();
    for (const corpusDir of corpusDirs) {
        if (!fs.existsSync(corpusDir)) {
            throw new Error(`corpus dir not found: ${corpusDir}`);
        }
        const real = fs.realpathSync(corpusDir);
        if (seenDirs.has(real)) {
            continue;
        }
        seenDirs.add(real);
        const root = corpusRoot(corpusDir);
        const fixtureDirs = [path.join(path.dirname(root), 'fixtures'), FIXTURES_DIR]
            .filter((d, i, all) => fs.existsSync(d) && all.findIndex(o => path.resolve(o) === path.resolve(d)) === i);
        for (const file of walkJson(corpusDir)) {
            const rel = path.relative(root, file).replace(/\\/g, '/').replace(/\.json$/, '');
            let parsed;
            try {
                parsed = JSON.parse(fs.readFileSync(file, 'utf8'));
            } catch (error) {
                throw new Error(`${file}: ${error.message}`);
            }
            const list = Array.isArray(parsed) ? parsed : Array.isArray(parsed.cases) ? parsed.cases : [parsed];
            list.forEach((c, index) => {
                const name = c.name ?? (list.length === 1 ? rel : `${rel}#${index}`);
                const body = JSON.stringify({ ...c, name });
                const previous = byName.get(name);
                if (previous) {
                    // The same case reached through two corpora (e.g. a worktree's copy
                    // of the seed corpus) runs once; a changed copy is an error.
                    if (previous.body === body) {
                        return;
                    }
                    throw new Error(`case ${name} is defined differently in ${previous.file} and ${file}`);
                }
                byName.set(name, { body, file });
                cases.push({ ...c, name, tags: c.tags ?? [], sourceFile: file, fixtureDirs });
            });
        }
    }
    return cases;
}

// --- Case setup ------------------------------------------------------------

const PLACEHOLDER = /\$\{(CASE_DIR|HOME)\}/g;

export function substitute(value, vars) {
    if (typeof value === 'string') {
        return value.replace(PLACEHOLDER, (_, key) => vars[key]);
    }
    if (Array.isArray(value)) {
        return value.map(v => substitute(v, vars));
    }
    if (value && typeof value === 'object') {
        return Object.fromEntries(Object.entries(value).map(([k, v]) => [substitute(k, vars), substitute(v, vars)]));
    }
    return value;
}

function slug(name) {
    return name.replace(/\//g, '__').replace(/[^A-Za-z0-9._-]/g, '_');
}

function resolveInHome(p, home) {
    return path.isAbsolute(p) ? p : path.join(home, p);
}

function findOnPath(command, pathValue) {
    for (const dir of (pathValue ?? '').split(path.delimiter)) {
        if (!dir) {
            continue;
        }
        const candidate = path.join(dir, command);
        try {
            fs.accessSync(candidate, fs.constants.X_OK);
            if (fs.statSync(candidate).isFile()) {
                return candidate;
            }
        } catch {
            // keep looking
        }
    }
    return null;
}

// The harness bin dir: git (whichever the parent PATH finds), node (this
// process's), and a failing stub for each STUBBED_COMMANDS entry.
function prepareHarnessBin(tmpRoot) {
    const bin = path.join(tmpRoot, 'bin');
    fs.mkdirSync(bin, { recursive: true });
    const git = findOnPath('git', process.env.PATH);
    if (!git) {
        throw new Error('git not found on PATH');
    }
    fs.symlinkSync(fs.realpathSync(git), path.join(bin, 'git'));
    fs.symlinkSync(process.execPath, path.join(bin, 'node'));
    for (const command of STUBBED_COMMANDS) {
        const stub = path.join(bin, command);
        fs.writeFileSync(stub, `#!/bin/sh\necho "${command}: blocked by the parity harness" >&2\nexit 127\n`);
        fs.chmodSync(stub, 0o755);
    }
    return bin;
}

function baseEnv(home, caseDef, pathDirs, fakeBinDirs) {
    const env = {
        PATH: pathDirs.join(path.delimiter),
        HOME: home,
        TZ: 'UTC',
        LANG: 'en_US.UTF-8',
        LC_ALL: 'en_US.UTF-8',
        CCSL_TEST_NOW_MS: String(caseDef.nowMs ?? DEFAULT_NOW_MS),
        CCSL_TEST_NO_PROBE: '1',
        CCSL_TEST_NO_NETWORK: '1',
        CCSL_TEST_NO_KEYCHAIN: '1',
        CCSL_TEST_NO_BACKGROUND: '1',
        CCSL_TEST_MEM_TOTAL_BYTES: String(DEFAULT_MEM_TOTAL_BYTES),
        CCSL_TEST_MEM_FREE_BYTES: String(DEFAULT_MEM_FREE_BYTES),
        GIT_CONFIG_GLOBAL: '/dev/null',
        GIT_CONFIG_NOSYSTEM: '1'
    };
    if (fakeBinDirs.length) {
        env.CCSL_TEST_FAKE_BIN_DIRS = fakeBinDirs.join(path.delimiter);
    }
    return env;
}

function buildEnv(caseDef, vars, pathDirs, fakeBinDirs) {
    const env = baseEnv(vars.HOME, caseDef, pathDirs, fakeBinDirs);
    for (const [key, value] of Object.entries(substitute(caseDef.env ?? {}, vars))) {
        if (value === null) {
            delete env[key];
        } else {
            env[key] = String(value);
        }
    }
    return env;
}

function writeFileEnsured(file, { content, mode }) {
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, content);
    if (mode !== undefined) {
        fs.chmodSync(file, mode);
    }
}

// string | {json} | {raw} | {executable}, the objects optionally with `mode`
// ("755" or 493).
function fileSpec(spec) {
    if (typeof spec === 'string') {
        return { content: spec };
    }
    if (!spec || typeof spec !== 'object') {
        throw new Error(`bad file spec: ${JSON.stringify(spec)}`);
    }
    const mode = spec.mode === undefined ? undefined : typeof spec.mode === 'string' ? Number.parseInt(spec.mode, 8) : spec.mode;
    if ('executable' in spec) {
        return { content: String(spec.executable), mode: mode ?? 0o755 };
    }
    if ('json' in spec) {
        return { content: JSON.stringify(spec.json, null, 2), mode };
    }
    if ('raw' in spec) {
        return { content: String(spec.raw), mode };
    }
    throw new Error(`bad file spec: ${JSON.stringify(spec)}`);
}

function findFixture(caseDef, fixture) {
    for (const dir of caseDef.fixtureDirs ?? [FIXTURES_DIR]) {
        const script = path.join(dir, `${fixture}.sh`);
        if (fs.existsSync(script)) {
            return script;
        }
    }
    throw new Error(`unknown fixture ${fixture}`);
}

// Re-creates the case directory from scratch: HOME, fixtures, files, config.
function prepareCase(caseDef, caseDir, harnessBin) {
    fs.rmSync(caseDir, { recursive: true, force: true });
    const home = path.join(caseDir, 'home');
    fs.mkdirSync(path.join(home, '.config', 'ccstatusline'), { recursive: true });
    fs.mkdirSync(path.join(home, '.claude'), { recursive: true });
    const vars = { CASE_DIR: caseDir, HOME: home };
    const fakeBinDirs = substitute(caseDef.pathPrepend ?? [], vars).map(d => path.resolve(caseDir, d));
    const systemDirs = [harnessBin, ...SYSTEM_PATH];
    const env = buildEnv(caseDef, vars, [...fakeBinDirs, ...systemDirs], fakeBinDirs);

    for (const fixture of caseDef.fixtures ?? []) {
        const built = spawnSync('sh', [findFixture(caseDef, fixture), path.join(caseDir, fixture)], {
            env: { PATH: systemDirs.join(path.delimiter), HOME: home, GIT_CONFIG_GLOBAL: '/dev/null', GIT_CONFIG_NOSYSTEM: '1' },
            encoding: 'utf8',
            timeout: 60_000
        });
        if (built.status !== 0) {
            throw new Error(`fixture ${fixture} failed (${built.status ?? built.signal}): ${built.stderr}`);
        }
    }

    for (const [rel, spec] of Object.entries(substitute(caseDef.files ?? {}, vars))) {
        writeFileEnsured(resolveInHome(rel, home), fileSpec(spec));
    }
    for (const dir of fakeBinDirs) {
        fs.mkdirSync(dir, { recursive: true });
    }

    const args = [];
    let configPath;
    if (!caseDef.noConfig) {
        configPath = caseDef.useDefaultConfigPath
            ? path.join(home, '.config', 'ccstatusline', 'settings.json')
            : path.join(caseDir, 'config', 'settings.json');
        if (!caseDef.useDefaultConfigPath) {
            args.push('--config', configPath);
        }
        if (caseDef.configRaw !== undefined) {
            writeFileEnsured(configPath, { content: substitute(caseDef.configRaw, vars) });
        } else if (caseDef.config !== undefined) {
            writeFileEnsured(configPath, { content: JSON.stringify(substitute(caseDef.config, vars), null, 2) });
        }
        // Neither config nor configRaw: the path is passed but the file is missing.
    } else {
        configPath = path.join(home, '.config', 'ccstatusline', 'settings.json');
    }
    args.push(...substitute(caseDef.args ?? [], vars));

    let input = '';
    if (caseDef.stdinRaw !== undefined) {
        input = substitute(caseDef.stdinRaw, vars);
    } else if (caseDef.stdin !== undefined) {
        input = JSON.stringify(substitute(caseDef.stdin, vars));
    }

    const cwd = caseDef.cwd ? path.resolve(caseDir, substitute(caseDef.cwd, vars)) : caseDir;
    fs.mkdirSync(cwd, { recursive: true });

    const compareFiles = (caseDef.compareFilesAfter ?? []).map(p => resolveInHome(substitute(p, vars), home));
    return { vars, env, args, input, cwd, configPath, compareFiles };
}

// --- Execution -------------------------------------------------------------

function runProcess(command, args, setup, timeoutMs) {
    const started = process.hrtime.bigint();
    const result = spawnSync(command, args, {
        cwd: setup.cwd,
        env: setup.env,
        input: setup.input,
        timeout: timeoutMs,
        killSignal: 'SIGKILL',
        maxBuffer: 64 * 1024 * 1024
    });
    const ms = Number(process.hrtime.bigint() - started) / 1e6;
    return {
        status: result.status,
        signal: result.signal,
        error: result.error ? (result.error.code ?? String(result.error)) : null,
        stdout: result.stdout ?? Buffer.alloc(0),
        stderr: result.stderr ?? Buffer.alloc(0),
        ms: Math.round(ms)
    };
}

const SIDES = {
    upstream: (args, setup, opts) => runProcess(
        process.execPath,
        ['--import', pathToFileURL(PRELOAD).href, UPSTREAM_DIST, ...args],
        setup,
        opts.timeoutMs
    ),
    rust: (args, setup, opts) => runProcess(opts.rustBin, args, withProfileFile(setup), opts.timeoutMs)
};

// An instrumented Rust build (CI's coverage job) writes its coverage profile to
// LLVM_PROFILE_FILE, which the case environment would otherwise drop.
function withProfileFile(setup) {
    const profileFile = process.env.LLVM_PROFILE_FILE;
    return profileFile ? { ...setup, env: { ...setup.env, LLVM_PROFILE_FILE: profileFile } } : setup;
}

function readMaybe(file) {
    try {
        return fs.readFileSync(file);
    } catch {
        return null;
    }
}

// Each distinct UUID v4 becomes <GUID#n> in order of first appearance, so the
// shape (which ids are equal) is still compared. v1 migration mints random ids.
function normalizeGuids(buffer) {
    if (buffer === null) {
        return null;
    }
    const ids = new Map();
    const text = buffer.toString('utf8').replace(/[0-9a-f]{8}-[0-9a-f]{4}-4[0-9a-f]{3}-[89ab][0-9a-f]{3}-[0-9a-f]{12}/gi, (id) => {
        const key = id.toLowerCase();
        if (!ids.has(key)) {
            ids.set(key, ids.size + 1);
        }
        return `<GUID#${ids.get(key)}>`;
    });
    return Buffer.from(text, 'utf8');
}

const DEFAULT_HOME_TREE_IGNORE = ['.cache/'];

// Every file under HOME as "== path\n<content>\n" blocks (paths sorted), so
// files one side writes and the other does not show up as a diff.
function homeTree(home, ignore) {
    const entries = [];
    const walk = (dir) => {
        for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
            const full = path.join(dir, entry.name);
            const rel = path.relative(home, full).replace(/\\/g, '/');
            if (ignore.some(prefix => rel === prefix.replace(/\/$/, '') || rel.startsWith(prefix))) {
                continue;
            }
            if (entry.isDirectory()) {
                walk(full);
            } else {
                const content = entry.isSymbolicLink() ? Buffer.from(`-> ${fs.readlinkSync(full)}`) : fs.readFileSync(full);
                entries.push(Buffer.from(`== ${rel}\n`), content, Buffer.from('\n'));
            }
        }
    };
    walk(home);
    return Buffer.concat(entries);
}

function runSide(sideName, caseDef, caseDir, opts, harnessBin) {
    const setup = prepareCase(caseDef, caseDir, harnessBin);
    const result = SIDES[sideName](setup.args, setup, opts);
    const guids = caseDef.normalizeGuids ?? true;
    const normalize = buffer => (guids ? normalizeGuids(buffer) : buffer);
    if (caseDef.compareConfigFileAfter) {
        result.configAfter = normalize(readMaybe(setup.configPath));
    }
    result.filesAfter = setup.compareFiles.map(file => ({ file: path.relative(caseDir, file), content: normalize(readMaybe(file)) }));
    if (caseDef.compareHomeTree ?? true) {
        result.filesAfter.push({
            file: 'home/** (tree)',
            content: normalize(homeTree(setup.vars.HOME, caseDef.homeTreeIgnore ?? DEFAULT_HOME_TREE_IGNORE))
        });
    }
    return result;
}

function firstDiffOffset(a, b) {
    const n = Math.min(a.length, b.length);
    for (let i = 0; i < n; i += 1) {
        if (a[i] !== b[i]) {
            return i;
        }
    }
    return a.length === b.length ? -1 : n;
}

function buffersEqual(a, b) {
    if (a === null || b === null) {
        return a === b;
    }
    return Buffer.compare(a, b) === 0;
}

function compareResults(caseDef, a, b) {
    const reasons = [];
    for (const [label, r] of [['A', a], ['B', b]]) {
        if (r.error) {
            reasons.push(`side ${label} failed to run: ${r.error}`);
        }
    }
    if (a.status !== b.status || a.signal !== b.signal) {
        reasons.push(`exit: ${a.status ?? a.signal} vs ${b.status ?? b.signal}`);
    }
    if (!buffersEqual(a.stdout, b.stdout)) {
        reasons.push(`stdout differs at byte ${firstDiffOffset(a.stdout, b.stdout)}`);
    }
    if (caseDef.compareStderr && !buffersEqual(a.stderr, b.stderr)) {
        reasons.push(`stderr differs at byte ${firstDiffOffset(a.stderr, b.stderr)}`);
    }
    if (caseDef.compareConfigFileAfter && !buffersEqual(a.configAfter, b.configAfter)) {
        reasons.push('config file after run differs');
    }
    a.filesAfter.forEach((fa, i) => {
        const fb = b.filesAfter[i];
        if (!buffersEqual(fa.content, fb.content)) {
            reasons.push(`file ${fa.file} after run differs`);
        }
    });
    return reasons;
}

// --- Output ----------------------------------------------------------------

// Characters JSON.stringify leaves alone but a terminal would not show: C1
// controls, NBSP (upstream turns every space into U+00A0), soft hyphen,
// zero-width and bidi marks, line/paragraph separators, word joiner, the
// Private Use Area (powerline glyphs), BOM and the replacement character.
const INVISIBLE = new RegExp(`[${[[0x80, 0xa0], [0xad], [0x200b, 0x200f], [0x2028, 0x2029], [0x2060], [0xe000, 0xf8ff], [0xfeff], [0xfffd]]
    .map(range => range.map(c => `\\u${c.toString(16).padStart(4, '0')}`).join('-'))
    .join('')}]`, 'g');

export function escapeVisible(text) {
    return JSON.stringify(text).replace(INVISIBLE, ch => `\\u${ch.charCodeAt(0).toString(16).padStart(4, '0')}`);
}

function visible(buffer) {
    if (buffer === null) {
        return '<missing>';
    }
    const text = buffer.toString('utf8');
    if (text === '') {
        return '""';
    }
    return text.split(/(?<=\n)/).map(line => `    ${escapeVisible(line)}`).join('\n');
}

function showBuffers(label, a, b, nameA, nameB) {
    const offset = firstDiffOffset(a ?? Buffer.alloc(0), b ?? Buffer.alloc(0));
    console.log(`  --- ${label} (${nameA}):`);
    console.log(visible(a));
    console.log(`  --- ${label} (${nameB}):`);
    console.log(visible(b));
    if (offset >= 0 && a && b) {
        const lo = Math.max(0, offset - 24);
        console.log(`  first differing byte: ${offset}`);
        console.log(`    ${nameA}: …${escapeVisible(a.subarray(lo, offset + 24).toString('utf8'))}`);
        console.log(`    ${nameB}: …${escapeVisible(b.subarray(lo, offset + 24).toString('utf8'))}`);
    }
}

function showDiff(caseDef, a, b, nameA, nameB) {
    console.log(`  exit: ${nameA}=${a.status ?? a.signal ?? a.error} ${nameB}=${b.status ?? b.signal ?? b.error}`);
    showBuffers('stdout', a.stdout, b.stdout, nameA, nameB);
    if (a.stderr.length || b.stderr.length) {
        console.log(`  --- stderr (${nameA}):`);
        console.log(visible(a.stderr));
        console.log(`  --- stderr (${nameB}):`);
        console.log(visible(b.stderr));
    }
    if (caseDef.compareConfigFileAfter && !buffersEqual(a.configAfter, b.configAfter)) {
        showBuffers('config file after', a.configAfter, b.configAfter, nameA, nameB);
    }
    a.filesAfter.forEach((fa, i) => {
        const fb = b.filesAfter[i];
        if (!buffersEqual(fa.content, fb.content)) {
            showBuffers(`file ${fa.file} after`, fa.content, fb.content, nameA, nameB);
        }
    });
}

function serializeResult(r) {
    if (!r) {
        return null;
    }
    return {
        status: r.status,
        signal: r.signal,
        error: r.error,
        ms: r.ms,
        stdout: r.stdout.toString('utf8'),
        stderr: r.stderr.toString('utf8'),
        ...(r.configAfter !== undefined ? { configAfter: r.configAfter?.toString('utf8') ?? null } : {}),
        ...(r.filesAfter.length ? { filesAfter: r.filesAfter.map(f => ({ file: f.file, content: f.content?.toString('utf8') ?? null })) } : {})
    };
}

// --- Machine-wide run lock -------------------------------------------------

let lockHeld = false;

function readLockOwner() {
    try {
        return JSON.parse(fs.readFileSync(path.join(LOCK_DIR, 'owner.json'), 'utf8'));
    } catch {
        return null;
    }
}

function pidAlive(pid) {
    try {
        process.kill(pid, 0);
        return true;
    } catch (error) {
        return error.code === 'EPERM';
    }
}

function tryAcquireLock() {
    try {
        fs.mkdirSync(LOCK_DIR);
    } catch (error) {
        if (error.code === 'EEXIST') {
            return false;
        }
        throw error;
    }
    lockHeld = true;
    fs.writeFileSync(path.join(LOCK_DIR, 'owner.json'), JSON.stringify({
        pid: process.pid,
        host: os.hostname(),
        started: new Date().toISOString(),
        cwd: process.cwd(),
        argv: process.argv.slice(2)
    }));
    return true;
}

// Why the current lock may be taken over, or null while it is live.
function staleReason() {
    let stat;
    try {
        stat = fs.statSync(LOCK_DIR);
    } catch {
        return null; // released meanwhile; the next mkdir decides
    }
    if (Date.now() - stat.mtimeMs > LOCK_STALE_MS) {
        return `not refreshed for ${Math.round((Date.now() - stat.mtimeMs) / 1000)} s`;
    }
    const owner = readLockOwner();
    if (owner && owner.host === os.hostname() && !pidAlive(owner.pid)) {
        return `owner pid ${owner.pid} is gone`;
    }
    return null;
}

function stealLock(reason) {
    // Rename first so two waiters cannot both delete a lock someone re-took.
    const graveyard = `${LOCK_DIR}.stale-${process.pid}-${Date.now()}`;
    try {
        fs.renameSync(LOCK_DIR, graveyard);
    } catch {
        return;
    }
    console.error(`parity lock: took over stale lock (${reason})`);
    fs.rmSync(graveyard, { recursive: true, force: true });
}

async function acquireLock() {
    let noticed = false;
    while (!tryAcquireLock()) {
        const reason = staleReason();
        if (reason) {
            stealLock(reason);
            continue;
        }
        if (!noticed) {
            const owner = readLockOwner();
            const who = owner ? ` (held by pid ${owner.pid}${owner.cwd ? ` in ${owner.cwd}` : ''} since ${owner.started})` : '';
            console.error(`waiting for parity lock${who}: ${LOCK_DIR}`);
            noticed = true;
        }
        await new Promise(resolve => setTimeout(resolve, LOCK_RETRY_MS));
    }
}

function refreshLock() {
    if (lockHeld) {
        const now = new Date();
        try {
            fs.utimesSync(LOCK_DIR, now, now);
        } catch {
            // lock dir gone (stolen): carry on, the run itself is still valid
        }
    }
}

function releaseLock() {
    if (!lockHeld) {
        return;
    }
    lockHeld = false;
    if (readLockOwner()?.pid === process.pid) {
        fs.rmSync(LOCK_DIR, { recursive: true, force: true });
    }
}

// --- Main ------------------------------------------------------------------

function corpusDirs(opts) {
    return opts.corpora.length ? opts.corpora : [DEFAULT_CORPUS];
}

let cleanupTmpRoot = null;

function cleanup() {
    releaseLock();
    if (cleanupTmpRoot) {
        fs.rmSync(cleanupTmpRoot, { recursive: true, force: true });
        cleanupTmpRoot = null;
    }
}

process.on('exit', cleanup);
for (const [signal, code] of [['SIGINT', 130], ['SIGTERM', 143], ['SIGHUP', 129]]) {
    process.on(signal, () => {
        cleanup();
        process.exit(code);
    });
}

const nextTick = () => new Promise(resolve => setImmediate(resolve));

async function main() {
    const opts = parseArgs(process.argv.slice(2));
    if (!fs.existsSync(UPSTREAM_DIST)) {
        console.error(`upstream not installed: run npm install --prefix ${path.join(PARITY_DIR, 'upstream')}`);
        process.exit(2);
    }
    const mode = opts.upstreamOnly ? 'upstream-only' : opts.determinism ? 'determinism' : 'parity';
    if (mode === 'parity') {
        try {
            fs.accessSync(opts.rustBin, fs.constants.X_OK);
        } catch {
            console.error(`rust binary not found or not executable: ${opts.rustBin} (use --rust-bin)`);
            process.exit(2);
        }
    }

    let cases = loadCases(corpusDirs(opts));
    if (opts.filters.length) {
        cases = cases.filter(c => opts.filters.some(f => c.name.includes(f)));
    }
    if (opts.tags.length) {
        cases = cases.filter(c => opts.tags.some(t => c.tags.includes(t)));
    }
    if (cases.length === 0) {
        console.error('no cases matched');
        process.exit(2);
    }

    await acquireLock();

    const tmpRoot = opts.tmpRoot ?? path.join(fs.realpathSync(os.tmpdir()), `ccsl-parity-${process.pid}`);
    fs.rmSync(path.join(tmpRoot, 'bin'), { recursive: true, force: true });
    fs.mkdirSync(tmpRoot, { recursive: true });
    if (!opts.keepTmp) {
        cleanupTmpRoot = tmpRoot;
    }
    const harnessBin = prepareHarnessBin(tmpRoot);
    const sideB = mode === 'parity' ? 'rust' : 'upstream';
    const nameA = 'upstream';
    const nameB = mode === 'parity' ? 'rust' : 'upstream#2';

    const records = [];
    let stoppedEarly = false;
    for (const caseDef of cases) {
        // Yield so SIGINT/SIGTERM handlers run between cases.
        await nextTick();
        refreshLock();
        const caseDir = path.join(tmpRoot, slug(caseDef.name));
        let record;
        try {
            const a = runSide('upstream', caseDef, caseDir, opts, harnessBin);
            if (mode === 'upstream-only') {
                console.log(`### ${caseDef.name} [${caseDef.tags.join(', ')}] exit=${a.status ?? a.signal ?? a.error} ${a.ms}ms`);
                console.log(visible(a.stdout));
                if (a.stderr.length) {
                    console.log('  stderr:');
                    console.log(visible(a.stderr));
                }
                if (a.configAfter !== undefined) {
                    console.log('  config after:');
                    console.log(visible(a.configAfter));
                }
                for (const f of a.filesAfter) {
                    if (f.content === null || f.content.length > 0) {
                        console.log(`  file ${f.file} after:`);
                        console.log(visible(f.content));
                    }
                }
                const ok = !a.error && a.signal === null;
                record = { name: caseDef.name, tags: caseDef.tags, pass: ok, reasons: ok ? [] : [`upstream failed: ${a.error ?? a.signal}`], upstream: serializeResult(a) };
            } else {
                const b = runSide(sideB, caseDef, caseDir, opts, harnessBin);
                const reasons = compareResults(caseDef, a, b);
                record = { name: caseDef.name, tags: caseDef.tags, pass: reasons.length === 0, reasons, upstream: serializeResult(a), [mode === 'parity' ? 'rust' : 'upstream2']: serializeResult(b) };
                if (!record.pass) {
                    console.log(`FAIL ${caseDef.name}: ${reasons.join('; ')}`);
                    if (opts.showDiff) {
                        showDiff(caseDef, a, b, nameA, nameB);
                    }
                } else if (!opts.quiet) {
                    console.log(`ok   ${caseDef.name}`);
                }
            }
        } catch (error) {
            record = { name: caseDef.name, tags: caseDef.tags, pass: false, reasons: [`harness error: ${error.message}`] };
            console.log(`FAIL ${caseDef.name}: harness error: ${error.message}`);
        }
        records.push(record);
        if (!opts.keepTmp) {
            fs.rmSync(caseDir, { recursive: true, force: true });
        }
        if (opts.failFast && !record.pass) {
            stoppedEarly = true;
            break;
        }
    }
    cleanup();

    const tagStats = {};
    for (const r of records) {
        for (const tag of r.tags) {
            tagStats[tag] ??= { pass: 0, total: 0 };
            tagStats[tag].total += 1;
            if (r.pass) {
                tagStats[tag].pass += 1;
            }
        }
    }
    const passed = records.filter(r => r.pass).length;
    const upstreamVersion = JSON.parse(fs.readFileSync(path.join(path.dirname(UPSTREAM_DIST), '..', 'package.json'), 'utf8')).version;
    const report = {
        mode,
        upstreamVersion,
        rustBin: mode === 'parity' ? opts.rustBin : null,
        corpora: corpusDirs(opts),
        totals: { cases: records.length, pass: passed, fail: records.length - passed, stoppedEarly },
        tags: Object.fromEntries(Object.entries(tagStats).sort(([a], [b]) => a.localeCompare(b))),
        cases: records
    };
    const reportJson = `${JSON.stringify(report, null, 2)}\n`;
    fs.writeFileSync(LAST_REPORT, reportJson);
    if (opts.jsonOut) {
        fs.writeFileSync(opts.jsonOut, reportJson);
    }

    if (mode !== 'upstream-only' || !opts.quiet) {
        console.log('\nPer tag (pass/total):');
        const width = Math.max(...Object.keys(tagStats).map(t => t.length), 3);
        for (const [tag, s] of Object.entries(report.tags)) {
            const mark = s.pass === s.total ? ' ' : '!';
            console.log(`  ${mark} ${tag.padEnd(width)}  ${s.pass}/${s.total}`);
        }
    }
    const label = mode === 'upstream-only' ? 'upstream ran cleanly' : mode === 'determinism' ? 'deterministic' : 'pass';
    console.log(`\n${mode}: ${passed}/${records.length} ${label}, ${records.length - passed} fail${stoppedEarly ? ' (stopped early)' : ''}`);
    console.log(`report: ${path.relative(process.cwd(), LAST_REPORT) || LAST_REPORT}`);
    process.exit(passed === records.length ? 0 : 1);
}

if (process.argv[1] && path.resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
    main().catch((error) => {
        console.error(error.stack ?? String(error));
        process.exit(2);
    });
}
