#!/usr/bin/env node
// Differential harness for the interactive config editor. Each case runs
// upstream ccstatusline (node + preload.mjs) and the Rust binary on a
// pseudo-terminal of the same size (tui/pty-run.py), sends the same keys, and
// reads both terminals through @xterm/headless. After every step it compares
// the whole screen cell by cell (character, colors, bold, dim, italic,
// underline, inverse, strikethrough); at the end it compares the exit codes
// and every file under HOME. The two sides run one after the other in the
// same HOME path, since paths can appear on screen. See README.md for the
// case format.
import { spawn } from 'node:child_process';
import * as fs from 'node:fs';
import { createRequire } from 'node:module';
import * as os from 'node:os';
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';

const PARITY_DIR = path.dirname(fileURLToPath(import.meta.url));
const REPO_DIR = path.resolve(PARITY_DIR, '..');
const UPSTREAM_DIST = path.join(PARITY_DIR, 'upstream', 'node_modules', 'ccstatusline', 'dist', 'ccstatusline.js');
const PRELOAD = path.join(PARITY_DIR, 'preload.mjs');
const PTY_RUN = path.join(PARITY_DIR, 'tui', 'pty-run.py');
const DEFAULT_CORPUS = path.join(PARITY_DIR, 'tui-corpus');
const DEFAULT_RUST_BIN = path.join(REPO_DIR, 'target', 'release', 'ccstatusline-rs');
const LAST_REPORT = path.join(PARITY_DIR, 'last-tui-report.json');

const { Terminal } = createRequire(path.join(PARITY_DIR, 'tui', 'package.json'))('@xterm/headless');

const DEFAULT_NOW_MS = Date.UTC(2026, 9, 21, 12, 0, 0);
const DEFAULT_SIZE = [100, 40];
const SYSTEM_PATH = ['/usr/bin', '/bin', '/usr/sbin', '/sbin'];
// Never do real work: network clients, the GitHub/GitLab CLIs, the keychain.
const FAILING_STUBS = ['gh', 'glab', 'curl', 'wget', 'security'];
// Openers succeed without opening anything.
const OK_STUBS = ['open', 'xdg-open', 'gio'];
// A step is over once the program has written nothing for this long (longer
// at start, while node loads upstream). Upstream's screens are the reference,
// so its window is the longer one; the Rust side also waits for a match.
const DEFAULT_QUIET_MS = 700;
const RUST_QUIET_MS = 300;
const START_QUIET_MS = 1500;
// The Rust side then gets up to this long to reach upstream's screen for the
// same step, so a stall on a loaded machine is not a failure.
const MATCH_WAIT_MS = 3000;
const STEP_TIMEOUT_MS = 10_000;
const EXIT_TIMEOUT_MS = 3_000;

const KEYS = {
    up: '\u001B[A',
    down: '\u001B[B',
    right: '\u001B[C',
    left: '\u001B[D',
    enter: '\r',
    escape: '\u001B',
    tab: '\t',
    backspace: '\u007F',
    delete: '\u001B[3~',
    space: ' ',
    'ctrl+left': '\u001B[1;5D',
    'ctrl+right': '\u001B[1;5C',
    'ctrl+c': '\u0003',
    'ctrl+s': '\u0013'
};

function usage() {
    return `usage: node parity/tui.mjs [options]

  --rust-bin PATH   Rust binary (default target/release/ccstatusline-rs)
  --filter SUBSTR   only cases whose name contains SUBSTR (repeatable, OR)
  --corpus DIR      load cases from DIR instead of parity/tui-corpus (repeatable)
  --show-diff       print both screens of the first differing step
  --json PATH       also write the report to PATH (always written to parity/last-tui-report.json)
  --upstream-only   run only upstream and print its screens (exploration)
  --keep-tmp        keep the scratch directories
  -q, --quiet       only print failures and the summary`;
}

function parseArgs(argv) {
    const opts = { rustBin: DEFAULT_RUST_BIN, filters: [], corpora: [], showDiff: false, jsonOut: null, upstreamOnly: false, keepTmp: false, quiet: false };
    for (let i = 0; i < argv.length; i += 1) {
        const arg = argv[i];
        const value = () => {
            if (argv[i + 1] === undefined) {
                throw new Error(`${arg} requires a value`);
            }
            i += 1;
            return argv[i];
        };
        switch (arg) {
            case '--rust-bin': opts.rustBin = path.resolve(value()); break;
            case '--filter': opts.filters.push(value()); break;
            case '--corpus': opts.corpora.push(path.resolve(value())); break;
            case '--show-diff': opts.showDiff = true; break;
            case '--json': opts.jsonOut = path.resolve(value()); break;
            case '--upstream-only': opts.upstreamOnly = true; break;
            case '--keep-tmp': opts.keepTmp = true; break;
            case '-q':
            case '--quiet': opts.quiet = true; break;
            case '-h':
            case '--help': console.log(usage()); process.exit(0); break;
            default: throw new Error(`unknown argument: ${arg}\n\n${usage()}`);
        }
    }
    return opts;
}

function loadCases(dirs) {
    const cases = [];
    const walk = (root, dir) => {
        for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
            const full = path.join(dir, entry.name);
            if (entry.isDirectory()) {
                walk(root, full);
            } else if (entry.name.endsWith('.json')) {
                const parsed = JSON.parse(fs.readFileSync(full, 'utf8'));
                const list = Array.isArray(parsed) ? parsed : [parsed];
                const base = path.relative(root, full).replace(/\.json$/, '');
                list.forEach((c, i) => cases.push({ ...c, name: c.name ? `${base}/${c.name}` : (list.length > 1 ? `${base}/${i}` : base) }));
            }
        }
    };
    for (const dir of dirs) {
        walk(dir, dir);
    }
    return cases;
}

function findOnPath(command, pathValue) {
    for (const dir of (pathValue ?? '').split(path.delimiter)) {
        const candidate = path.join(dir, command);
        try {
            fs.accessSync(candidate, fs.constants.X_OK);
            return candidate;
        } catch { /* keep looking */ }
    }
    return null;
}

function makeBinDir(tmpRoot) {
    const bin = path.join(tmpRoot, 'bin');
    fs.mkdirSync(bin, { recursive: true });
    const git = findOnPath('git', process.env.PATH);
    if (git) {
        fs.symlinkSync(fs.realpathSync(git), path.join(bin, 'git'));
    }
    fs.symlinkSync(process.execPath, path.join(bin, 'node'));
    for (const command of FAILING_STUBS) {
        fs.writeFileSync(path.join(bin, command), `#!/bin/sh\necho "${command}: blocked by the parity harness" >&2\nexit 127\n`, { mode: 0o755 });
    }
    for (const command of OK_STUBS) {
        fs.writeFileSync(path.join(bin, command), '#!/bin/sh\nexit 0\n', { mode: 0o755 });
    }
    return bin;
}

const substitute = (value, vars) => {
    if (typeof value === 'string') {
        return value.replace(/\$\{(\w+)\}/g, (m, k) => (k in vars ? vars[k] : m));
    }
    if (Array.isArray(value)) {
        return value.map(v => substitute(v, vars));
    }
    if (value && typeof value === 'object') {
        return Object.fromEntries(Object.entries(value).map(([k, v]) => [k, substitute(v, vars)]));
    }
    return value;
};

function fileContent(spec) {
    if (typeof spec === 'string') {
        return { content: spec };
    }
    const mode = spec.mode === undefined ? undefined : (typeof spec.mode === 'string' ? Number.parseInt(spec.mode, 8) : spec.mode);
    if ('json' in spec) {
        return { content: JSON.stringify(spec.json, null, 2), mode };
    }
    if ('raw' in spec) {
        return { content: String(spec.raw), mode };
    }
    if ('executable' in spec) {
        return { content: String(spec.executable), mode: mode ?? 0o755 };
    }
    throw new Error(`bad file spec: ${JSON.stringify(spec)}`);
}

function prepareHome(caseDef, dir) {
    fs.rmSync(dir, { recursive: true, force: true });
    const home = path.join(dir, 'home');
    fs.mkdirSync(path.join(home, '.config', 'ccstatusline'), { recursive: true });
    fs.mkdirSync(path.join(home, '.claude'), { recursive: true });
    const vars = { HOME: home, CASE_DIR: dir };
    const files = { ...(caseDef.files ?? {}) };
    if (caseDef.config !== undefined && caseDef.config !== null) {
        files['.config/ccstatusline/settings.json'] = typeof caseDef.config === 'string' ? caseDef.config : { json: caseDef.config };
    }
    for (const [rel, spec] of Object.entries(substitute(files, vars))) {
        const file = path.join(home, rel);
        fs.mkdirSync(path.dirname(file), { recursive: true });
        const { content, mode } = fileContent(spec);
        fs.writeFileSync(file, content);
        if (mode !== undefined) {
            fs.chmodSync(file, mode);
        }
    }
    return { home, vars };
}

function caseEnv(caseDef, home, binDir, vars) {
    // pathPrepend: directories (relative to HOME, or absolute) searched before the harness's bin dir, for fakes.
    const prepend = substitute(caseDef.pathPrepend ?? [], vars).map(d => path.resolve(home, d));
    const env = {
        PATH: [...prepend, binDir, ...SYSTEM_PATH].join(path.delimiter),
        HOME: home,
        TZ: 'UTC',
        LANG: 'en_US.UTF-8',
        LC_ALL: 'en_US.UTF-8',
        TERM: 'xterm-256color',
        CCSL_TEST_NOW_MS: String(caseDef.nowMs ?? DEFAULT_NOW_MS),
        CCSL_TEST_NO_PROBE: '1',
        CCSL_TEST_NO_NETWORK: '1',
        CCSL_TEST_NO_KEYCHAIN: '1',
        CCSL_TEST_NO_BACKGROUND: '1',
        CCSL_TEST_RANDOM_SEED: String(caseDef.seed ?? 1),
        GIT_CONFIG_GLOBAL: '/dev/null',
        GIT_CONFIG_NOSYSTEM: '1'
    };
    // An instrumented Rust build (CI's coverage job) writes its coverage profile here.
    if (process.env.LLVM_PROFILE_FILE) {
        env.LLVM_PROFILE_FILE = process.env.LLVM_PROFILE_FILE;
    }
    for (const [key, value] of Object.entries(substitute(caseDef.env ?? {}, vars))) {
        if (value === null) {
            delete env[key];
        } else {
            env[key] = String(value);
        }
    }
    return env;
}

function keyBytes(step) {
    if (typeof step === 'string') {
        if (!(step in KEYS)) {
            throw new Error(`unknown key "${step}" (use {"text": ...} for literal input)`);
        }
        return KEYS[step];
    }
    if (step && typeof step.text === 'string') {
        return step.text;
    }
    throw new Error(`bad step: ${JSON.stringify(step)}`);
}

const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));

const cellAttrs = c => [
    c.getFgColorMode(), c.getFgColor(), c.getBgColorMode(), c.getBgColor(),
    c.isBold(), c.isDim(), c.isItalic(), c.isUnderline(), c.isInverse(), c.isStrikethrough()
].map(Number).join(',');
const PLAIN = '0,0,0,0,0,0,0,0,0,0';

// The visible screen as text plus a per-line list of styled cells.
function snapshot(term) {
    const buffer = term.buffer.active;
    const lines = [];
    for (let y = 0; y < term.rows; y++) {
        const line = buffer.getLine(buffer.viewportY + y);
        let textLine = '';
        const styles = [];
        for (let x = 0; x < term.cols; x++) {
            const cell = line.getCell(x);
            const chars = cell.getChars();
            textLine += chars || (cell.getWidth() === 0 ? '' : ' ');
            const attrs = cellAttrs(cell);
            if (attrs !== PLAIN && (chars.trim() || cell.getBgColorMode() || cell.isInverse() || cell.isUnderline())) {
                styles.push(`${x}:${attrs}`);
            }
        }
        lines.push({ text: textLine.replace(/\s+$/, ''), styles: styles.join(' ') });
    }
    while (lines.length && !lines.at(-1).text && !lines.at(-1).styles) {
        lines.pop();
    }
    return lines;
}

const screenText = lines => lines.map(l => l.text).join('\n');
const screenKey = lines => lines.map(l => `${l.text}\u0000${l.styles}`).join('\n');

class Session {
    constructor(command, args, env, size) {
        this.term = new Terminal({ cols: size[0], rows: size[1], allowProposedApi: true, scrollback: 1000 });
        this.lastOutput = Date.now();
        this.exitCode = undefined;
        this.stderr = '';
        this.child = spawn(findOnPath('python3', process.env.PATH) ?? 'python3', ['-I', PTY_RUN, String(size[0]), String(size[1]), command, ...args], { env, stdio: ['pipe', 'pipe', 'pipe'] });
        this.child.stdout.on('data', (data) => {
            this.lastOutput = Date.now();
            this.term.write(data);
        });
        this.child.stderr.on('data', (data) => { this.stderr += data; });
        this.exited = new Promise((resolve) => {
            this.child.on('exit', (code, signal) => {
                this.exitCode = code ?? (signal ? `signal ${signal}` : null);
                resolve();
            });
        });
        this.child.stdin.on('error', () => { /* the program exited */ });
    }

    send(bytes) {
        this.lastOutput = Date.now();
        if (this.exitCode === undefined) {
            this.child.stdin.write(bytes);
        }
    }

    flushed() {
        return new Promise(resolve => this.term.write('', resolve));
    }

    kill() {
        if (this.exitCode === undefined) {
            this.child.kill('SIGTERM');
        }
    }
}

// Waits until no session has written for quietMs (or they all exited).
async function settle(sessions, quietMs) {
    const started = Date.now();
    await sleep(Math.min(quietMs, 150));
    while (Date.now() - started < STEP_TIMEOUT_MS) {
        const busy = sessions.some(s => s.exitCode === undefined && Date.now() - s.lastOutput < quietMs);
        if (!busy) {
            break;
        }
        await sleep(25);
    }
    await Promise.all(sessions.map(s => s.flushed()));
}

function readTree(root) {
    const out = {};
    const walk = (dir) => {
        for (const entry of fs.readdirSync(dir, { withFileTypes: true }).sort((a, b) => a.name.localeCompare(b.name))) {
            const full = path.join(dir, entry.name);
            const rel = path.relative(root, full);
            if (rel === '.cache' || rel.startsWith(`.cache${path.sep}`)) {
                continue;
            }
            if (entry.isDirectory()) {
                walk(full);
            } else if (entry.isSymbolicLink()) {
                out[rel] = `-> ${fs.readlinkSync(full)}`;
            } else {
                out[rel] = fs.readFileSync(full, 'utf8').replaceAll(root, '${HOME}');
            }
        }
    };
    walk(root);
    return out;
}

// Runs one side through every step, in a HOME at `dir` (the same path for both sides: paths show on screen).
// With `expected` (upstream's screens), each step also waits for the screen to match, up to MATCH_WAIT_MS.
async function runSide(side, caseDef, opts, dir, binDir, steps, expected = null) {
    const size = caseDef.size ?? DEFAULT_SIZE;
    const quietMs = side === 'rust' ? RUST_QUIET_MS : (caseDef.quietMs ?? DEFAULT_QUIET_MS);
    const { home, vars } = prepareHome(caseDef, dir);
    const env = caseEnv(caseDef, home, binDir, vars);
    const args = substitute(caseDef.args ?? [], vars);
    const session = side === 'upstream'
        ? new Session(process.execPath, ['--import', pathToFileURL(PRELOAD).href, UPSTREAM_DIST, ...args], env, size)
        : new Session(opts.rustBin, args, env, size);
    const screens = [];
    for (const [i, step] of steps.entries()) {
        if (step.bytes !== undefined) {
            session.send(step.bytes);
        }
        await settle([session], i === 0 ? Math.max(quietMs, caseDef.startQuietMs ?? START_QUIET_MS) : quietMs);
        let screen = snapshot(session.term);
        // Nothing on screen yet at start means the program is still loading.
        for (const started = Date.now(); i === 0 && !screen.length && Date.now() - started < STEP_TIMEOUT_MS;) {
            await settle([session], quietMs);
            screen = snapshot(session.term);
        }
        if (expected) {
            const want = screenKey(expected[i]);
            const started = Date.now();
            while (screenKey(screen) !== want && Date.now() - started < MATCH_WAIT_MS) {
                await sleep(50);
                await session.flushed();
                screen = snapshot(session.term);
            }
        }
        screens.push(screen);
    }
    // A case that leaves the editor running ends with Ctrl+C, a clean exit (which also lets an instrumented
    // build write its coverage profile).
    await Promise.race([session.exited, sleep(200)]);
    session.send(KEYS['ctrl+c']);
    await Promise.race([session.exited, sleep(EXIT_TIMEOUT_MS)]);
    session.kill();
    await session.exited;
    return { screens, exitCode: session.exitCode, tree: readTree(home), stderr: session.stderr };
}

async function runCase(caseDef, opts, tmpRoot, binDir) {
    const dir = path.join(tmpRoot, 'case');
    const steps = [{ label: 'start' }, ...(caseDef.keys ?? []).map(k => ({ label: typeof k === 'string' ? k : JSON.stringify(k), bytes: keyBytes(k) }))];
    const up = await runSide('upstream', caseDef, opts, dir, binDir, steps);
    if (opts.upstreamOnly) {
        return { name: caseDef.name, steps: steps.map((s, i) => ({ label: s.label, screen: screenText(up.screens[i]) })), exitCode: up.exitCode };
    }
    const rs = await runSide('rust', caseDef, opts, dir, binDir, steps, up.screens);

    const failures = [];
    let firstDiff = null;
    steps.forEach((step, i) => {
        if (screenKey(up.screens[i]) !== screenKey(rs.screens[i])) {
            failures.push(`screen after step ${i} (${step.label})`);
            firstDiff ??= { step: i, label: step.label, upstream: up.screens[i], rust: rs.screens[i] };
        }
    });
    if (up.exitCode !== rs.exitCode) {
        failures.push(`exit code ${up.exitCode} vs ${rs.exitCode}`);
    }
    for (const file of [...new Set([...Object.keys(up.tree), ...Object.keys(rs.tree)])].sort()) {
        if (up.tree[file] !== rs.tree[file]) {
            failures.push(`file ${file} ${up.tree[file] === undefined ? 'only from rust' : rs.tree[file] === undefined ? 'only from upstream' : 'differs'}`);
        }
    }
    return { name: caseDef.name, pass: failures.length === 0, failures, firstDiff, files: { upstream: up.tree, rust: rs.tree } };
}

function printDiff(result) {
    const d = result.firstDiff;
    if (!d) {
        return;
    }
    const show = lines => lines.map((l, i) => `${String(i).padStart(3)} ${l.text}${l.styles ? `   {${l.styles}}` : ''}`).join('\n');
    console.log(`  first differing screen, after step ${d.step} (${d.label}):`);
    console.log(`  --- upstream\n${show(d.upstream)}\n  --- rust\n${show(d.rust)}`);
    for (const f of result.failures.filter(x => x.startsWith('file '))) {
        const file = f.split(' ')[1];
        console.log(`  --- ${file} (upstream)\n${result.files.upstream[file] ?? '(missing)'}\n  --- ${file} (rust)\n${result.files.rust[file] ?? '(missing)'}`);
    }
}

async function main() {
    const opts = parseArgs(process.argv.slice(2));
    let cases = loadCases(opts.corpora.length ? opts.corpora : [DEFAULT_CORPUS]);
    if (opts.filters.length) {
        cases = cases.filter(c => opts.filters.some(f => c.name.includes(f)));
    }
    if (!opts.upstreamOnly && !fs.existsSync(opts.rustBin)) {
        throw new Error(`Rust binary not found: ${opts.rustBin} (cargo build --release)`);
    }
    const tmpRoot = fs.mkdtempSync(path.join(os.tmpdir(), 'ccsl-tui-'));
    const binDir = makeBinDir(tmpRoot);
    const results = [];
    try {
        for (const caseDef of cases) {
            const result = await runCase(caseDef, opts, tmpRoot, binDir);
            results.push(result);
            if (opts.upstreamOnly) {
                console.log(`== ${result.name} (exit ${result.exitCode})`);
                for (const s of result.steps) {
                    console.log(`-- after ${s.label}\n${s.screen}`);
                }
                continue;
            }
            if (!result.pass) {
                console.log(`FAIL ${result.name}: ${result.failures.join('; ')}`);
                if (opts.showDiff) {
                    printDiff(result);
                }
            } else if (!opts.quiet) {
                console.log(`ok   ${result.name}`);
            }
        }
    } finally {
        if (!opts.keepTmp) {
            fs.rmSync(tmpRoot, { recursive: true, force: true });
        }
    }
    if (opts.upstreamOnly) {
        return;
    }
    const passed = results.filter(r => r.pass).length;
    const report = { passed, total: results.length, failed: results.filter(r => !r.pass).map(r => ({ name: r.name, failures: r.failures })) };
    fs.writeFileSync(LAST_REPORT, JSON.stringify(report, null, 2));
    if (opts.jsonOut) {
        fs.writeFileSync(opts.jsonOut, JSON.stringify(report, null, 2));
    }
    console.log(`tui parity: ${passed}/${results.length} pass, ${results.length - passed} fail`);
    process.exitCode = passed === results.length ? 0 : 1;
}

await main();
