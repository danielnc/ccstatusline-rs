// Loaded into the upstream ccstatusline process with `node --import <this file>`.
// Makes upstream deterministic and offline. The Rust binary honours the same
// CCSL_TEST_* variables natively, so both sides see the same world.
//
//   CCSL_TEST_NOW_MS=<ms>          `new Date()` / `Date.now()` / `Date()` return this instant
//   CCSL_TEST_NO_PROBE=1           `ps` / `stty` (terminal width probe) fail as if missing,
//                                  and Linux /proc/<pid>/fd/* readlinks fail
//   CCSL_TEST_NO_BACKGROUND=1      detached `spawn()` (git-review refresh) is a no-op
//   CCSL_TEST_MEM_TOTAL_BYTES=<n>  os.totalmem(); also makes `vm_stat` fail
//   CCSL_TEST_MEM_FREE_BYTES=<n>   os.freemem()
//
// Always, regardless of env: the macOS keychain (`security`), `curl`/`wget`,
// `globalThis.fetch` and http(s).request/get are blocked. `gh`/`glab` are
// blocked unless they resolve into a CCSL_TEST_FAKE_BIN_DIRS dir (a case's
// pathPrepend fakes). git, jj and sh are left alone (git widgets and
// custom-command widgets need them).
import { EventEmitter } from 'node:events';
import { createRequire, syncBuiltinESMExports } from 'node:module';
import * as nodePath from 'node:path';

const require = createRequire(import.meta.url);
const childProcess = require('node:child_process');
const fs = require('node:fs');
const http = require('node:http');
const https = require('node:https');
const os = require('node:os');

const env = process.env;

// --- Fixed clock -----------------------------------------------------------

if (env.CCSL_TEST_NOW_MS !== undefined && env.CCSL_TEST_NOW_MS !== '') {
    const fixedNow = Number(env.CCSL_TEST_NOW_MS);
    if (!Number.isFinite(fixedNow)) {
        throw new Error(`CCSL_TEST_NOW_MS is not a number: ${env.CCSL_TEST_NOW_MS}`);
    }
    const RealDate = globalThis.Date;
    class FixedDate extends RealDate {
        constructor(...args) {
            if (args.length === 0) {
                super(fixedNow);
            } else {
                super(...args);
            }
        }

        static now() {
            return fixedNow;
        }
    }
    // `Date()` called without `new` returns a string for the current instant.
    globalThis.Date = new Proxy(FixedDate, {
        apply() {
            return new RealDate(fixedNow).toString();
        }
    });
}

// --- child_process ---------------------------------------------------------

const ALWAYS_BLOCKED = new Set(['security', 'curl', 'wget']);
// Forge CLIs share one real GitHub token across every agent: they only ever
// run when they resolve to a fake a case put in a pathPrepend dir
// (CCSL_TEST_FAKE_BIN_DIRS, set by the runner).
const FAKE_ONLY = new Set(['gh', 'glab']);

function blockedCommands() {
    const blocked = new Set(ALWAYS_BLOCKED);
    if (env.CCSL_TEST_NO_PROBE === '1') {
        blocked.add('ps');
        blocked.add('stty');
    }
    if (env.CCSL_TEST_MEM_TOTAL_BYTES) {
        blocked.add('vm_stat');
    }
    return blocked;
}

function firstToken(file, isShellString) {
    if (typeof file !== 'string') {
        return '';
    }
    return isShellString ? (file.trim().split(/\s+/)[0] ?? '') : file;
}

function commandName(file, isShellString) {
    return nodePath.basename(firstToken(file, isShellString));
}

function findOnPath(command, pathValue) {
    for (const dir of (pathValue ?? '').split(nodePath.delimiter)) {
        if (!dir) {
            continue;
        }
        const candidate = nodePath.join(dir, command);
        try {
            fs.accessSync(candidate, fs.constants.X_OK);
            return candidate;
        } catch {
            // keep looking
        }
    }
    return null;
}

function resolvesToFake(token, options) {
    const fakeDirs = (env.CCSL_TEST_FAKE_BIN_DIRS ?? '').split(nodePath.delimiter).filter(Boolean);
    if (fakeDirs.length === 0) {
        return false;
    }
    const resolved = token.includes('/') ? nodePath.resolve(token) : findOnPath(token, options?.env?.PATH ?? env.PATH);
    return resolved !== null && fakeDirs.some(dir => nodePath.dirname(resolved) === nodePath.resolve(dir));
}

function isBlocked(file, isShellString = false, options = null) {
    const name = commandName(file, isShellString);
    if (FAKE_ONLY.has(name)) {
        return !resolvesToFake(firstToken(file, isShellString), options);
    }
    return blockedCommands().has(name);
}

function blockedError(file) {
    const error = new Error(`spawn ${file} ENOENT (blocked by parity preload)`);
    error.code = 'ENOENT';
    error.errno = -2;
    error.syscall = `spawn ${file}`;
    error.path = file;
    error.status = null;
    error.signal = null;
    error.stdout = '';
    error.stderr = '';
    return error;
}

function lastFunctionArg(args) {
    for (let i = args.length - 1; i >= 0; i -= 1) {
        if (typeof args[i] === 'function') {
            return args[i];
        }
    }
    return null;
}

function optionsArg(args) {
    for (const arg of args) {
        if (arg && typeof arg === 'object' && !Array.isArray(arg)) {
            return arg;
        }
    }
    return null;
}

function fakeChild() {
    const child = new EventEmitter();
    child.pid = undefined;
    child.stdin = null;
    child.stdout = null;
    child.stderr = null;
    child.unref = () => child;
    child.ref = () => child;
    child.kill = () => false;
    return child;
}

const real = {
    execFileSync: childProcess.execFileSync,
    execSync: childProcess.execSync,
    spawnSync: childProcess.spawnSync,
    execFile: childProcess.execFile,
    exec: childProcess.exec,
    spawn: childProcess.spawn
};

childProcess.execFileSync = function execFileSync(file, ...rest) {
    if (isBlocked(file, false, optionsArg(rest))) {
        throw blockedError(file);
    }
    return real.execFileSync.call(this, file, ...rest);
};

childProcess.execSync = function execSync(command, ...rest) {
    if (isBlocked(command, true, optionsArg(rest))) {
        throw blockedError(commandName(command, true));
    }
    return real.execSync.call(this, command, ...rest);
};

childProcess.spawnSync = function spawnSync(file, ...rest) {
    const options = optionsArg(rest);
    const shell = Boolean(options?.shell);
    if (isBlocked(file, shell, options)) {
        const error = blockedError(commandName(file, shell));
        const empty = options?.encoding && options.encoding !== 'buffer' ? '' : Buffer.alloc(0);
        return { pid: 0, output: [null, empty, empty], stdout: empty, stderr: empty, status: null, signal: null, error };
    }
    return real.spawnSync.call(this, file, ...rest);
};

childProcess.execFile = function execFile(file, ...rest) {
    if (isBlocked(file, false, optionsArg(rest))) {
        const callback = lastFunctionArg(rest);
        const child = fakeChild();
        const error = blockedError(file);
        process.nextTick(() => {
            if (callback) {
                callback(error, '', '');
            }
            child.emit('error', error);
            child.emit('close', null, null);
        });
        return child;
    }
    return real.execFile.call(this, file, ...rest);
};

childProcess.exec = function exec(command, ...rest) {
    if (isBlocked(command, true, optionsArg(rest))) {
        const callback = lastFunctionArg(rest);
        const child = fakeChild();
        const error = blockedError(commandName(command, true));
        process.nextTick(() => {
            if (callback) {
                callback(error, '', '');
            }
            child.emit('close', null, null);
        });
        return child;
    }
    return real.exec.call(this, command, ...rest);
};

childProcess.spawn = function spawn(file, ...rest) {
    const options = optionsArg(rest);
    if (env.CCSL_TEST_NO_BACKGROUND === '1' && options?.detached) {
        return fakeChild();
    }
    if (isBlocked(file, Boolean(options?.shell), options)) {
        const child = fakeChild();
        const error = blockedError(commandName(file, Boolean(options?.shell)));
        process.nextTick(() => {
            if (child.listenerCount('error') > 0) {
                child.emit('error', error);
            }
            child.emit('close', -2, null);
        });
        return child;
    }
    return real.spawn.call(this, file, ...rest);
};

// --- Network ---------------------------------------------------------------

function networkError(host) {
    const error = new Error(`connect ECONNREFUSED ${host ?? 'network'} (blocked by parity preload)`);
    error.code = 'ECONNREFUSED';
    error.errno = -61;
    error.syscall = 'connect';
    return error;
}

function hostOf(args) {
    const [first] = args;
    if (typeof first === 'string') {
        return first;
    }
    if (first instanceof URL) {
        return first.host;
    }
    return first?.hostname ?? first?.host;
}

// A ClientRequest stand-in that fails asynchronously, the way an offline
// request does: callers attach 'error' after request() returns.
function blockedRequest(...args) {
    const request = new EventEmitter();
    let failed = false;
    const fail = () => {
        if (failed) {
            return;
        }
        failed = true;
        const error = networkError(hostOf(args));
        if (request.listenerCount('error') > 0) {
            request.emit('error', error);
        }
        request.emit('close');
    };
    request.destroyed = false;
    request.end = () => {
        process.nextTick(fail);
        return request;
    };
    request.write = () => true;
    request.setTimeout = () => request;
    request.setHeader = () => request;
    request.setNoDelay = () => request;
    request.setSocketKeepAlive = () => request;
    request.abort = () => {
        request.destroyed = true;
    };
    request.destroy = () => {
        request.destroyed = true;
        return request;
    };
    // Real requests error even when end() is never called
    process.nextTick(fail);
    return request;
}

function blockedGet(...args) {
    const request = blockedRequest(...args);
    request.end();
    return request;
}

for (const mod of [http, https]) {
    mod.request = blockedRequest;
    mod.get = blockedGet;
}

globalThis.fetch = async (input) => {
    const error = new TypeError('fetch failed');
    error.cause = networkError(typeof input === 'string' ? input : input?.url);
    throw error;
};

// --- Linux width probe -----------------------------------------------------

// On Linux upstream probes the width without subprocesses: it walks
// /proc/<pid>/stat ancestors and readlinks /proc/<pid>/fd/{0,1,2} looking for
// a tty. Hide those fds so no ancestor (e.g. the harness's own terminal) counts.
if (env.CCSL_TEST_NO_PROBE === '1') {
    const realReadlinkSync = fs.readlinkSync;
    fs.readlinkSync = function readlinkSync(p, ...rest) {
        if (typeof p === 'string' && /^\/proc\/\d+\/fd\/\d+$/.test(p)) {
            const error = new Error(`ENOENT: no such file or directory, readlink '${p}' (blocked by parity preload)`);
            error.code = 'ENOENT';
            throw error;
        }
        return realReadlinkSync.call(this, p, ...rest);
    };
}

// --- Memory ----------------------------------------------------------------

if (env.CCSL_TEST_MEM_TOTAL_BYTES) {
    const total = Number(env.CCSL_TEST_MEM_TOTAL_BYTES);
    os.totalmem = () => total;
}
if (env.CCSL_TEST_MEM_FREE_BYTES) {
    const free = Number(env.CCSL_TEST_MEM_FREE_BYTES);
    os.freemem = () => free;
}

// Bundled ESM uses named imports (`import { execFileSync } from "node:child_process"`);
// those bindings only see the patched CJS exports after a sync.
syncBuiltinESMExports();
