#!/usr/bin/env node
// Generates the seed corpus in parity/corpus/ (committed output). Deterministic:
// re-running it rewrites the same files. Widget types and powerline theme names
// are read from the upstream source in upstream-src/v2.2.32/src.
//
//   node parity/gen/seed.mjs
import * as fs from 'node:fs';
import * as path from 'node:path';
import { fileURLToPath } from 'node:url';

const PARITY_DIR = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const REPO_DIR = path.resolve(PARITY_DIR, '..');
const UPSTREAM_SRC = path.join(REPO_DIR, 'upstream-src', 'v2.2.32', 'src');
const CORPUS_DIR = path.join(PARITY_DIR, 'corpus');

const NOW_MS = Date.UTC(2026, 9, 21, 12, 0, 0);
const NOW_S = NOW_MS / 1000;

// --- Upstream lists --------------------------------------------------------

function readWidgetTypes() {
    const src = fs.readFileSync(path.join(UPSTREAM_SRC, 'utils', 'widget-manifest.ts'), 'utf8');
    const manifest = src.slice(src.indexOf('WIDGET_MANIFEST'), src.indexOf('LAYOUT_WIDGET_MANIFEST'));
    const types = [...manifest.matchAll(/\{ type: '([a-z0-9-]+)', create:/g)].map(m => m[1]);
    if (types.length < 80) {
        throw new Error(`parsed only ${types.length} widget types from widget-manifest.ts`);
    }
    return types;
}

function readThemeNames() {
    const src = fs.readFileSync(path.join(UPSTREAM_SRC, 'utils', 'colors.ts'), 'utf8');
    const block = src.slice(src.indexOf('export const POWERLINE_THEMES'), src.indexOf('export function getPowerlineThemes'));
    const names = [...block.matchAll(/^ {4}'([a-z0-9-]+)': \{/gm)].map(m => m[1]);
    if (!names.includes('nord') || !names.includes('custom')) {
        throw new Error(`could not parse powerline theme names: ${names.join(',')}`);
    }
    return names;
}

const WIDGET_TYPES = readWidgetTypes();
const THEMES = readThemeNames();

// --- Building blocks -------------------------------------------------------

function isPlainObject(v) {
    return v !== null && typeof v === 'object' && !Array.isArray(v);
}

// Deep-merge `overrides` into a copy of `base`; an `undefined` value deletes the key.
function merge(base, overrides = {}) {
    const out = structuredClone(base);
    for (const [key, value] of Object.entries(overrides)) {
        if (value === undefined) {
            delete out[key];
        } else if (isPlainObject(value) && isPlainObject(out[key])) {
            out[key] = merge(out[key], value);
        } else {
            out[key] = structuredClone(value);
        }
    }
    return out;
}

const SESSION_ID = 'parity-session-0001';

function richStdin(cwd = '${CASE_DIR}/no-git', overrides = {}) {
    return merge({
        hook_event_name: 'Status',
        session_id: SESSION_ID,
        cwd,
        model: { id: 'claude-opus-4-1-20250805', display_name: 'Opus 4.1' },
        workspace: { current_dir: cwd, project_dir: cwd },
        version: '2.0.37',
        output_style: { name: 'Explanatory' },
        effort: { level: 'high' },
        cost: {
            total_cost_usd: 1.2345,
            total_duration_ms: 3723000,
            total_api_duration_ms: 1234567,
            total_lines_added: 156,
            total_lines_removed: 23
        },
        context_window: {
            context_window_size: 200000,
            total_input_tokens: 45678,
            total_output_tokens: 12345,
            current_usage: {
                input_tokens: 1234,
                output_tokens: 567,
                cache_creation_input_tokens: 8910,
                cache_read_input_tokens: 34567
            },
            used_percentage: 22,
            remaining_percentage: 78
        },
        rate_limits: {
            five_hour: { used_percentage: 42.5, resets_at: NOW_S + 2 * 3600 + 13 * 60 + 7 },
            seven_day: { used_percentage: 17, resets_at: NOW_S + 3 * 86400 + 5 * 3600 + 30 * 60 },
            seven_day_sonnet: { used_percentage: 8, resets_at: NOW_S + 3 * 86400 + 5 * 3600 + 30 * 60 },
            seven_day_opus: { used_percentage: 61.25, resets_at: NOW_S + 3 * 86400 + 5 * 3600 + 30 * 60 }
        },
        vim: { mode: 'INSERT' },
        worktree: {
            name: 'feature-x',
            path: '${CASE_DIR}/wt/feature-x',
            branch: 'wt/feature-x',
            original_cwd: cwd,
            original_branch: 'main'
        }
    }, overrides);
}

function cfg(lines, overrides = {}) {
    let n = 0;
    const withIds = lines.map(line => line.map((item) => {
        n += 1;
        return { id: String(n), ...item };
    }));
    return merge({ version: 4, colorLevel: 2, lines: withIds }, overrides);
}

const cases = new Map();

function add(name, c) {
    if (cases.has(name)) {
        throw new Error(`duplicate case ${name}`);
    }
    const tags = [...new Set(c.tags ?? [])];
    cases.set(name, { name, ...c, tags });
}

function plain(c) {
    return { fixtures: ['no-git'], cwd: 'no-git', ...c };
}

const ct = (text, extra = {}) => ({ type: 'custom-text', customText: text, ...extra });
const sep = (extra = {}) => ({ type: 'separator', ...extra });
const flex = () => ({ type: 'flex-separator' });

// A line of plain (non-git) widgets used by the rendering-feature cases.
const STD_LINE = [
    { type: 'model', color: 'cyan' },
    sep(),
    { type: 'context-length', color: 'brightBlack' },
    sep(),
    { type: 'session-cost', color: 'green' },
    sep(),
    ct('hello world', { color: 'magenta' })
];
// The same widgets without explicit separators (powerline / defaultSeparator).
const STD_ITEMS = [
    { type: 'model', color: 'white', backgroundColor: 'bgBlue' },
    { type: 'context-percentage', color: 'black', backgroundColor: 'bgMagenta' },
    { type: 'session-cost', color: 'black', backgroundColor: 'bgGreen' },
    ct('hello', { color: 'white', backgroundColor: 'bgRed' })
];

// --- Core ------------------------------------------------------------------

function genCore() {
    const tags = ['core'];
    add('core/default-config-missing', plain({
        tags: [...tags, 'model', 'context-length', 'git-branch', 'git-changes', 'separator'],
        noConfig: true,
        stdin: richStdin(),
        compareConfigFileAfter: true
    }));
    add('core/default-config-missing-git', {
        tags: [...tags, 'model', 'context-length', 'git-branch', 'git-changes', 'separator'],
        noConfig: true,
        fixtures: ['git-dirty'],
        cwd: 'git-dirty',
        stdin: richStdin('${CASE_DIR}/git-dirty'),
        compareConfigFileAfter: true
    });
    add('core/version4-defaults', plain({ tags, config: { version: 4 }, stdin: richStdin() }));
    add('core/version4-defaults-git', {
        tags,
        fixtures: ['git-dirty'],
        cwd: 'git-dirty',
        config: { version: 4 },
        stdin: richStdin('${CASE_DIR}/git-dirty')
    });
    add('core/stdin-empty-object', plain({ tags, config: { version: 4 }, stdin: {} }));
    add('core/stdin-empty-object-no-config', plain({ tags, noConfig: true, stdin: {} }));
    add('core/model-missing', plain({ tags: [...tags, 'model'], config: { version: 4 }, stdin: richStdin(undefined, { model: undefined }) }));
    add('core/model-string', plain({ tags: [...tags, 'model'], config: { version: 4 }, stdin: richStdin(undefined, { model: 'claude-sonnet-4-5' }) }));
    add('core/model-object-id-only', plain({ tags: [...tags, 'model'], config: { version: 4 }, stdin: richStdin(undefined, { model: { id: 'claude-haiku-4-5' } }) }));
    add('core/model-object-empty', plain({ tags: [...tags, 'model'], config: { version: 4 }, stdin: richStdin(undefined, { model: {} }) }));
    add('core/workspace-null', plain({ tags, config: { version: 4 }, stdin: richStdin(undefined, { workspace: null, cwd: undefined }) }));
    add('core/numeric-strings-coerced', plain({
        tags: [...tags, 'session-cost', 'context-length'],
        config: cfg([[{ type: 'session-cost' }, sep(), { type: 'context-length' }, sep(), { type: 'context-percentage' }]]),
        stdin: richStdin(undefined, { cost: { total_cost_usd: ' 2.5 ' }, context_window: { context_window_size: '1000000', used_percentage: '12.5' } })
    }));
    add('core/unknown-fields-passthrough', plain({ tags, config: { version: 4 }, stdin: richStdin(undefined, { extra_field: { nested: [1, 2, 3] }, another: 'x' }) }));
    add('cli/version', { tags: ['cli'], noConfig: true, args: ['--version'] });
    add('cli/version-with-stdin', { tags: ['cli'], noConfig: true, args: ['--version'], stdin: richStdin() });
    add('cli/config-missing-arg', { tags: ['cli'], noConfig: true, args: ['--config'], stdin: richStdin(), compareStderr: true });
    add('cli/config-arg-is-flag', { tags: ['cli'], noConfig: true, args: ['--config', '--foo'], stdin: richStdin(), compareStderr: true });
    add('cli/config-then-version', { tags: ['cli'], noConfig: true, args: ['--config', '--version'] });
    add('cli/config-relative-path', plain({
        tags: ['cli'],
        noConfig: true,
        files: { '${CASE_DIR}/no-git/rel-settings.json': { json: cfg([[ct('from relative config', { color: 'yellow' })]]) } },
        args: ['--config', 'rel-settings.json'],
        stdin: richStdin()
    }));
    add('cli/invalid-json-stdin', { tags: ['cli', 'core'], noConfig: true, stdinRaw: '{"model": ' });
    add('cli/empty-stdin', { tags: ['cli', 'core'], noConfig: true, stdinRaw: '' });
    add('cli/whitespace-stdin', { tags: ['cli', 'core'], noConfig: true, stdinRaw: '  \n\t\n' });
    add('cli/stdin-array', { tags: ['cli', 'core'], noConfig: true, stdinRaw: '[]' });
    add('cli/stdin-null', { tags: ['cli', 'core'], noConfig: true, stdinRaw: 'null' });
    add('cli/stdin-number', { tags: ['cli', 'core'], noConfig: true, stdinRaw: '42' });
    add('cli/stdin-schema-error', { tags: ['cli', 'core'], noConfig: true, stdin: { model: 42 } });
    add('cli/stdin-schema-error-cost', { tags: ['cli', 'core'], noConfig: true, stdin: { cost: { total_cost_usd: 'abc' } } });
    add('cli/stdin-trailing-garbage', { tags: ['cli', 'core'], noConfig: true, stdinRaw: '{} x' });
    add('cli/stdin-bom', plain({ tags: ['cli', 'core'], config: { version: 4 }, stdinRaw: '\ufeff{}' }));
    add('cli/unknown-flag-ignored', plain({ tags: ['cli'], config: { version: 4 }, args: ['--frobnicate'], stdin: richStdin() }));

    const skillsFile = `.cache/ccstatusline/skills/skills-${SESSION_ID}.jsonl`;
    add('cli/hook-pretooluse-skill', {
        tags: ['cli', 'hook', 'skills'],
        noConfig: true,
        args: ['--hook'],
        stdin: { session_id: SESSION_ID, hook_event_name: 'PreToolUse', tool_name: 'Skill', tool_input: { skill: 'pdf' } },
        compareFilesAfter: [skillsFile, '.config/ccstatusline/settings.json']
    });
    add('cli/hook-prompt-slash-command', {
        tags: ['cli', 'hook', 'skills'],
        noConfig: true,
        args: ['--hook'],
        stdin: { session_id: SESSION_ID, hook_event_name: 'UserPromptSubmit', prompt: '/review-prs now please' },
        files: { [skillsFile]: `${JSON.stringify({ timestamp: '2026-10-21T11:00:00.000Z', session_id: SESSION_ID, skill: 'pdf', source: 'PreToolUse' })}\n` },
        compareFilesAfter: [skillsFile]
    });
    add('cli/hook-prompt-no-command', {
        tags: ['cli', 'hook'],
        noConfig: true,
        args: ['--hook'],
        stdin: { session_id: SESSION_ID, hook_event_name: 'UserPromptSubmit', prompt: 'hello /notacommand' },
        compareFilesAfter: [skillsFile]
    });
    add('cli/hook-unsafe-session-id', {
        tags: ['cli', 'hook'],
        noConfig: true,
        args: ['--hook'],
        stdin: { session_id: '../escape', hook_event_name: 'PreToolUse', tool_name: 'Skill', tool_input: { skill: 'pdf' } },
        compareFilesAfter: ['.cache/ccstatusline/skills/skills-../escape.jsonl']
    });
    add('cli/hook-invalid-json', { tags: ['cli', 'hook'], noConfig: true, args: ['--hook'], stdinRaw: 'not json' });
}

// --- Widgets ---------------------------------------------------------------

const GIT_FIXTURES = [
    ['no-git', 'no-git'],
    ['git-clean', 'git-clean'],
    ['git-empty', 'git-empty'],
    ['git-dirty', 'git-dirty'],
    ['git-ahead-behind', 'git-ahead-behind'],
    ['git-worktree', 'git-worktree/linked'],
    ['git-conflict', 'git-conflict'],
    ['git-detached', 'git-detached']
];

function widgetCase(type, variant, item = {}, extra = {}) {
    const { stdin, tags = [], lineOverrides, settings, ...rest } = extra;
    const line = [{ type, ...item }];
    add(`widgets/${type}/${variant}`, plain({
        tags: [type, ...tags],
        config: cfg([line], settings),
        stdin: stdin ?? richStdin(),
        ...rest
    }));
}

function gitWidgetCases(type) {
    for (const [fixture, cwd] of GIT_FIXTURES) {
        add(`widgets/${type}/${fixture}`, {
            tags: [type, 'git', fixture],
            fixtures: [fixture],
            cwd,
            config: cfg([[{ type }]]),
            stdin: richStdin(`\${CASE_DIR}/${cwd}`)
        });
    }
    add(`widgets/${type}/git-dirty-raw`, {
        tags: [type, 'git', 'rawValue'],
        fixtures: ['git-dirty'],
        cwd: 'git-dirty',
        config: cfg([[{ type, rawValue: true }]]),
        stdin: richStdin('${CASE_DIR}/git-dirty')
    });
    add(`widgets/${type}/no-git-hide`, {
        tags: [type, 'git', 'hide'],
        fixtures: ['no-git'],
        cwd: 'no-git',
        config: cfg([[ct('A'), sep(), { type, metadata: { hide: 'no-git,no-remote,no-upstream,not-fork,zero,no-data' } }, sep(), ct('B')]]),
        stdin: richStdin('${CASE_DIR}/no-git')
    });
    add(`widgets/${type}/git-clean-hide-zero`, {
        tags: [type, 'git', 'hide'],
        fixtures: ['git-clean'],
        cwd: 'git-clean',
        config: cfg([[ct('A'), sep(), { type, metadata: { hide: 'zero' } }, sep(), ct('B')]]),
        stdin: richStdin('${CASE_DIR}/git-clean')
    });
}

const USAGE_PERCENT_WIDGETS = ['session-usage', 'weekly-usage', 'weekly-sonnet-usage', 'weekly-opus-usage', 'fable-weekly-usage'];
const RESET_WIDGETS = ['block-timer', 'reset-timer', 'weekly-reset-timer'];
const EXTRA_USAGE_WIDGETS = ['extra-usage-utilization', 'extra-usage-remaining', 'extra-usage-used', 'extra-usage-daily-budget'];

function genWidgets() {
    for (const type of WIDGET_TYPES) {
        if (type.startsWith('git-')) {
            gitWidgetCases(type);
            continue;
        }
        widgetCase(type, 'default');
        widgetCase(type, 'raw', { rawValue: true }, { tags: ['rawValue'] });
        widgetCase(type, 'empty-stdin', {}, { stdin: {} });
        widgetCase(type, 'color-level-3', { color: 'hex:FF8800', bold: true }, { settings: { colorLevel: 3 }, tags: ['colorLevel'] });
    }

    // Model
    widgetCase('model', 'string-model', {}, { stdin: richStdin(undefined, { model: 'claude-sonnet-4-5' }) });
    widgetCase('model', 'id-only', {}, { stdin: richStdin(undefined, { model: { id: 'claude-haiku-4-5-20251001' } }) });

    // Context family
    const contextTypes = ['context-length', 'context-window', 'context-percentage', 'context-percentage-usable', 'context-bar', 'tokens-input', 'tokens-output', 'tokens-cached', 'tokens-total', 'cache-hit-rate', 'cache-read', 'cache-write'];
    for (const type of contextTypes) {
        widgetCase(type, 'usage-as-number', {}, { stdin: richStdin(undefined, { context_window: { current_usage: 98765 } }) });
        widgetCase(type, 'no-current-usage', {}, { stdin: richStdin(undefined, { context_window: { current_usage: null, used_percentage: undefined, remaining_percentage: undefined } }) });
        widgetCase(type, 'one-million', {}, {
            stdin: richStdin(undefined, {
                model: { id: 'claude-sonnet-4-5-20250929[1m]', display_name: 'Sonnet 4.5 (1M context)' },
                context_window: { context_window_size: 1000000, used_percentage: 61, remaining_percentage: 39, current_usage: { input_tokens: 600000, cache_read_input_tokens: 10000 } }
            })
        });
        widgetCase(type, 'no-context-window', {}, { stdin: richStdin(undefined, { context_window: undefined }) });
        widgetCase(type, 'high-usage', {}, {
            stdin: richStdin(undefined, { context_window: { used_percentage: 93.7, remaining_percentage: 6.3, current_usage: { input_tokens: 150000, cache_read_input_tokens: 37400 } } })
        });
    }
    widgetCase('context-length', 'fallback-env', {}, { stdin: richStdin(undefined, { context_window: { context_window_size: undefined } }), env: { CCSTATUSLINE_CONTEXT_SIZE_FALLBACK: '400000' } });
    widgetCase('context-percentage', 'fallback-env', {}, { stdin: richStdin(undefined, { context_window: { context_window_size: undefined, used_percentage: undefined } }), env: { CCSTATUSLINE_CONTEXT_SIZE_FALLBACK: '400000' } });
    for (const display of ['progress', 'progress-short', 'slider', 'slider-only']) {
        widgetCase('context-bar', `display-${display}`, { metadata: { display } });
    }
    for (const display of ['slider', 'slider-only']) {
        widgetCase('context-percentage', `display-${display}`, { metadata: { display } });
        widgetCase('context-percentage-usable', `display-${display}`, { metadata: { display } });
    }
    widgetCase('context-percentage', 'inverse', { metadata: { inverse: 'true' } });
    widgetCase('context-percentage-usable', 'inverse', { metadata: { inverse: 'true' } });
    for (const style of ['precise', 'compact', 'whole']) {
        widgetCase('context-length', `number-format-${style}`, { numberFormat: { style, decimals: 2 } }, { tags: ['numberFormat'] });
        widgetCase('session-cost', `number-format-${style}`, { numberFormat: { style, decimals: 3 } }, { tags: ['numberFormat'] });
        widgetCase('tokens-total', `global-number-format-${style}`, {}, { settings: { numberFormat: { token: { style, decimals: 1 } } }, tags: ['numberFormat'] });
    }

    // Usage (rate_limits) and reset timers
    for (const type of USAGE_PERCENT_WIDGETS) {
        for (const display of ['progress', 'progress-short', 'slider', 'slider-only']) {
            widgetCase(type, `display-${display}`, { metadata: { display } });
        }
        widgetCase(type, 'inverted-progress', { metadata: { display: 'progress', invert: 'true' } });
        widgetCase(type, 'cursor-slider', { metadata: { display: 'slider', cursor: 'true' } });
        widgetCase(type, 'no-rate-limits', {}, { stdin: richStdin(undefined, { rate_limits: undefined }) });
    }
    for (const type of RESET_WIDGETS) {
        widgetCase(type, 'compact', { metadata: { compact: 'true' } });
        widgetCase(type, 'hours', { metadata: { hours: 'true' } });
        widgetCase(type, 'progress', { metadata: { display: 'progress' } });
        widgetCase(type, 'slider', { metadata: { display: 'slider' } });
        widgetCase(type, 'absolute', { metadata: { absolute: 'true' } });
        widgetCase(type, 'absolute-tokyo-12h', { metadata: { absolute: 'true', timezone: 'Asia/Tokyo', hour12: 'true' } });
        widgetCase(type, 'absolute-de-weekday', { metadata: { absolute: 'true', locale: 'de-DE', weekday: 'true' } });
        widgetCase(type, 'no-rate-limits', {}, { stdin: richStdin(undefined, { rate_limits: undefined }) });
        widgetCase(type, 'reset-in-past', {}, { stdin: richStdin(undefined, { rate_limits: { five_hour: { resets_at: NOW_S - 60 }, seven_day: { resets_at: NOW_S - 60 } } }) });
    }
    for (const type of EXTRA_USAGE_WIDGETS) {
        widgetCase(type, 'hide-disabled', { metadata: { hide: 'disabled,no-data' } });
    }

    // Session
    for (const [label, ms] of [['zero', 0], ['seconds', 42000], ['minutes', 754000], ['hours', 3723000], ['days', 2 * 86400000 + 3 * 3600000]]) {
        widgetCase('session-clock', `duration-${label}`, {}, { stdin: richStdin(undefined, { cost: { total_duration_ms: ms } }) });
        widgetCase('session-cost-rate', `duration-${label}`, {}, { stdin: richStdin(undefined, { cost: { total_duration_ms: ms } }) });
    }
    for (const [label, usd] of [['zero', 0], ['tiny', 0.004], ['large', 1234.5678]]) {
        widgetCase('session-cost', `cost-${label}`, {}, { stdin: richStdin(undefined, { cost: { total_cost_usd: usd } }) });
    }
    for (const level of ['low', 'medium', 'high', 'xhigh', 'max', null]) {
        widgetCase('thinking-effort', `level-${level}`, {}, { stdin: richStdin(undefined, { effort: { level } }) });
    }
    widgetCase('thinking-effort', 'no-effort', {}, { stdin: richStdin(undefined, { effort: undefined }) });
    for (const mode of ['INSERT', 'NORMAL', 'VISUAL', 'VISUAL LINE']) {
        for (const format of ['icon-dash-letter', 'icon-letter', 'icon', 'letter', 'word']) {
            widgetCase('vim-mode', `${mode.replace(' ', '-').toLowerCase()}-${format}`, { metadata: { format } }, { stdin: richStdin(undefined, { vim: { mode } }) });
        }
    }
    widgetCase('vim-mode', 'nerd-font', { metadata: { nerdFont: 'true' } });
    widgetCase('vim-mode', 'no-vim', {}, { stdin: richStdin(undefined, { vim: undefined }) });
    widgetCase('output-style', 'no-style', {}, { stdin: richStdin(undefined, { output_style: undefined }) });
    widgetCase('version', 'null-version', {}, { stdin: richStdin(undefined, { version: null }) });
    for (const type of ['worktree-mode', 'worktree-name', 'worktree-branch', 'worktree-original-branch']) {
        widgetCase(type, 'no-worktree', {}, { stdin: richStdin(undefined, { worktree: undefined }) });
    }
    widgetCase('terminal-width', 'width-120', {}, { env: { CCSTATUSLINE_WIDTH: '120' }, tags: ['width'] });
    widgetCase('free-memory', 'number-format', { numberFormat: { style: 'compact', decimals: 2 } }, { tags: ['numberFormat'] });

    // Text-ish widgets
    widgetCase('custom-text', 'text', { customText: 'Hello, parity!', color: 'yellow' });
    widgetCase('custom-text', 'unicode', { customText: 'ünïcødé ✓ 日本語 🚀' });
    widgetCase('custom-text', 'parens-dim', { customText: '(dimmed parens)', dim: 'parens' });
    widgetCase('custom-text', 'control-chars', { customText: 'bell\u0007esc\u001b[31mred' });
    widgetCase('custom-symbol', 'star', { customSymbol: '★', color: 'brightYellow' });
    widgetCase('custom-symbol', 'emoji', { customSymbol: '🔥' });
    widgetCase('link', 'url-and-text', { metadata: { url: 'https://example.com/docs', text: 'Docs' } });
    widgetCase('link', 'url-only', { metadata: { url: 'https://example.com' } });
    widgetCase('link', 'unsafe-url', { metadata: { url: 'https://example.com/\u0007x', text: 'bad' } });
    widgetCase('custom-command', 'echo', { commandPath: 'echo hello from command' }, { tags: ['process'] });
    widgetCase('custom-command', 'printf-ansi-stripped', { commandPath: "printf '\\033[31mred\\033[0m text'" }, { tags: ['process'] });
    widgetCase('custom-command', 'printf-ansi-preserved', { commandPath: "printf '\\033[31mred\\033[0m text'", preserveColors: true }, { tags: ['process'] });
    widgetCase('custom-command', 'max-width', { commandPath: 'echo abcdefghijklmnopqrstuvwxyz', maxWidth: 10 }, { tags: ['process'] });
    widgetCase('custom-command', 'multiline-first-line', { commandPath: "printf 'line one\\nline two\\n'" }, { tags: ['process'] });
    widgetCase('custom-command', 'reads-stdin', { commandPath: "sh -c 'cat | head -c 40'" }, { tags: ['process'] });
    widgetCase('custom-command', 'failing', { commandPath: 'sh -c "exit 3"' }, { tags: ['process'] });
    widgetCase('custom-command', 'missing-binary', { commandPath: 'definitely-not-a-real-binary-xyz' }, { tags: ['process'] });
    widgetCase('custom-command', 'pwd', { commandPath: 'pwd' }, { tags: ['process'] });
    widgetCase('claude-session-id', 'no-session', {}, { stdin: richStdin(undefined, { session_id: undefined }) });

    // CWD
    for (const [label, metadata] of [['segments-1', { segments: '1' }], ['segments-2', { segments: '2' }], ['fish', { fishStyle: 'true' }], ['abbreviate-home', { abbreviateHome: 'true' }], ['fish-abbrev', { fishStyle: 'true', abbreviateHome: 'true' }]]) {
        add(`widgets/current-working-dir/home-${label}`, {
            tags: ['current-working-dir'],
            files: { 'projects/my-app/src/.keep': '' },
            cwd: '${HOME}/projects/my-app/src',
            config: cfg([[{ type: 'current-working-dir', metadata }]]),
            stdin: richStdin('${HOME}/projects/my-app/src')
        });
        widgetCase('current-working-dir', label, { metadata });
    }

    // Claude settings readers
    const statusFormats = {
        'sandbox-status': ['glyph', 'text', 'word'],
        'voice-status': ['icon', 'icon-text', 'text', 'word'],
        'remote-control-status': ['icon', 'icon-text', 'text', 'word', 'label-check', 'label-mark']
    };
    for (const [type, formats] of Object.entries(statusFormats)) {
        const key = type === 'sandbox-status' ? 'sandbox' : 'voice';
        for (const enabled of [true, false]) {
            for (const format of formats) {
                const files = type === 'remote-control-status'
                    ? { '.claude/sessions/12345.json': { json: { sessionId: SESSION_ID, bridgeSessionId: enabled ? 'bridge-1' : null } } }
                    : { '.claude/settings.json': { json: { [key]: { enabled } } } };
                widgetCase(type, `${enabled ? 'on' : 'off'}-${format}`, { metadata: { format } }, { files });
            }
        }
        widgetCase(type, 'nerd-font-on', { metadata: { nerdFont: 'true' } }, {
            files: type === 'remote-control-status'
                ? { '.claude/sessions/1.json': { json: { sessionId: SESSION_ID, bridgeSessionId: 'b' } } }
                : { '.claude/settings.json': { json: { [key]: { enabled: true } } } }
        });
    }
    widgetCase('sandbox-status', 'project-local-overrides', {}, {
        files: {
            '.claude/settings.json': { json: { sandbox: { enabled: false } } },
            '${CASE_DIR}/no-git/.claude/settings.local.json': { json: { sandbox: { enabled: true } } }
        }
    });
    widgetCase('voice-status', 'malformed-settings', {}, { files: { '.claude/settings.json': '{ nope' } });
    widgetCase('voice-status', 'claude-config-dir', {}, {
        env: { CLAUDE_CONFIG_DIR: '${CASE_DIR}/claude-alt' },
        files: { '${CASE_DIR}/claude-alt/settings.json': { json: { voice: { enabled: true } } } }
    });
    widgetCase('claude-account-email', 'with-email', {}, { files: { '.claude.json': { json: { oauthAccount: { emailAddress: 'dev@example.com' } } } } });
    widgetCase('claude-account-email', 'config-dir-email', {}, {
        env: { CLAUDE_CONFIG_DIR: '${CASE_DIR}/claude-alt' },
        files: { '${CASE_DIR}/claude-alt/.claude.json': { json: { oauthAccount: { emailAddress: 'alt@example.com' } } } }
    });
    widgetCase('claude-account-email', 'malformed', {}, { files: { '.claude.json': 'nope' } });

    // Skills cache
    const skillsFile = `.cache/ccstatusline/skills/skills-${SESSION_ID}.jsonl`;
    const skillLines = ['pdf', 'docx', 'pdf', 'xlsx'].map((skill, i) => JSON.stringify({ timestamp: `2026-10-21T1${i}:00:00.000Z`, session_id: SESSION_ID, skill, source: 'PreToolUse' })).join('\n');
    for (const mode of ['current', 'count', 'list']) {
        widgetCase('skills', `mode-${mode}`, { metadata: { mode } }, { files: { [skillsFile]: `${skillLines}\n` } });
    }
    widgetCase('skills', 'hide-empty', { metadata: { hide: 'empty' } });

    // jj (jj may be absent: then this covers the no-jj placeholder)
    for (const type of WIDGET_TYPES.filter(t => t.startsWith('jj-'))) {
        widgetCase(type, 'hide-no-jj', { metadata: { hide: 'no-jj' } }, { tags: ['hide'] });
    }

    // Compaction counter formats (no transcript: zero)
    for (const format of ['icon-space-number', 'text-and-number', 'number']) {
        widgetCase('compaction-counter', `format-${format}`, { metadata: { format } });
    }
    widgetCase('compaction-counter', 'hide-zero', { metadata: { hide: 'zero' } });

    // Git extras
    for (const type of ['git-branch', 'git-root-dir', 'git-origin-owner-repo']) {
        add(`widgets/${type}/link-to-repo`, {
            tags: [type, 'git', 'link'],
            fixtures: ['git-ahead-behind'],
            cwd: 'git-ahead-behind',
            config: cfg([[{ type, metadata: { linkToRepo: 'true', linkToIDE: 'vscode' } }]]),
            stdin: richStdin('${CASE_DIR}/git-ahead-behind')
        });
    }
    add('widgets/git-branch/cwd-from-process', {
        tags: ['git-branch', 'git'],
        fixtures: ['git-dirty'],
        cwd: 'git-dirty',
        config: cfg([[{ type: 'git-branch' }, sep(), { type: 'git-changes' }]]),
        stdin: richStdin(undefined, { cwd: undefined, workspace: undefined })
    });
    add('widgets/git-branch/subdirectory', {
        tags: ['git-branch', 'git'],
        fixtures: ['git-dirty'],
        cwd: 'git-dirty/src',
        config: cfg([[{ type: 'git-branch' }, sep(), { type: 'git-root-dir' }, sep(), { type: 'git-status' }]]),
        stdin: richStdin('${CASE_DIR}/git-dirty/src')
    });
    add('widgets/git-sha/main-worktree', {
        tags: ['git-sha', 'git-worktree', 'git'],
        fixtures: ['git-worktree'],
        cwd: 'git-worktree/main',
        config: cfg([[{ type: 'git-sha' }, sep(), { type: 'git-worktree' }, sep(), { type: 'git-branch' }]]),
        stdin: richStdin('${CASE_DIR}/git-worktree/main')
    });
    add('widgets/git-all/git-dirty', {
        tags: ['git', 'multi'],
        fixtures: ['git-dirty'],
        cwd: 'git-dirty',
        config: cfg([WIDGET_TYPES.filter(t => t.startsWith('git-')).flatMap(type => [{ type }, sep()]).slice(0, -1)]),
        stdin: richStdin('${CASE_DIR}/git-dirty')
    });
    add('widgets/git-all/git-ahead-behind', {
        tags: ['git', 'multi'],
        fixtures: ['git-ahead-behind'],
        cwd: 'git-ahead-behind',
        config: cfg([WIDGET_TYPES.filter(t => t.startsWith('git-')).flatMap(type => [{ type }, sep()]).slice(0, -1)]),
        stdin: richStdin('${CASE_DIR}/git-ahead-behind')
    });

    // Every non-git widget on one line
    add('widgets/all-non-git/default', plain({
        tags: ['multi'],
        config: cfg([WIDGET_TYPES.filter(t => !t.startsWith('git-')).flatMap(type => [{ type }, sep()]).slice(0, -1)]),
        stdin: richStdin()
    }));
    add('widgets/all-non-git/raw', plain({
        tags: ['multi', 'rawValue'],
        config: cfg([WIDGET_TYPES.filter(t => !t.startsWith('git-')).map(type => ({ type, rawValue: true }))], { defaultSeparator: ' · ' }),
        stdin: richStdin()
    }));

    // Layout widgets
    const layoutStdin = richStdin();
    add('widgets/separator/default', plain({ tags: ['separator'], config: cfg([[ct('A'), sep(), ct('B')]]), stdin: layoutStdin }));
    add('widgets/separator/character', plain({ tags: ['separator'], config: cfg([[ct('A'), sep({ character: '•' }), ct('B')]]), stdin: layoutStdin }));
    add('widgets/separator/colored', plain({ tags: ['separator'], config: cfg([[ct('A'), sep({ color: 'red', backgroundColor: 'bgWhite', bold: true }), ct('B')]]), stdin: layoutStdin }));
    add('widgets/separator/leading-trailing', plain({ tags: ['separator'], config: cfg([[sep(), ct('A'), sep(), sep(), ct('B'), sep()]]), stdin: layoutStdin }));
    add('widgets/separator/around-empty-widget', plain({ tags: ['separator'], config: cfg([[ct('A'), sep(), ct(''), sep(), ct('B')]]), stdin: layoutStdin }));
    add('widgets/separator/only', plain({ tags: ['separator'], config: cfg([[sep()]]), stdin: layoutStdin }));
    for (const width of [null, '40', '80', '200']) {
        add(`widgets/flex-separator/width-${width ?? 'unset'}`, plain({
            tags: ['flex-separator', 'flex'],
            config: cfg([[ct('left'), flex(), ct('right')]]),
            stdin: layoutStdin,
            ...(width ? { env: { CCSTATUSLINE_WIDTH: width } } : {})
        }));
    }
    add('widgets/flex-separator/two-flex', plain({ tags: ['flex-separator', 'flex'], config: cfg([[ct('a'), flex(), ct('b'), flex(), ct('c')]]), env: { CCSTATUSLINE_WIDTH: '61' }, stdin: layoutStdin }));
    add('widgets/flex-separator/only-flex', plain({ tags: ['flex-separator', 'flex'], config: cfg([[flex()]]), env: { CCSTATUSLINE_WIDTH: '80' }, stdin: layoutStdin }));
}

// --- Rendering features ----------------------------------------------------

function genRendering() {
    const stdin = richStdin();
    const r = (name, tags, config, extra = {}) => add(`render/${name}`, plain({ tags, config, stdin, ...extra }));

    for (const level of [0, 1, 2, 3]) {
        r(`color-level-${level}`, ['colorLevel'], cfg([STD_LINE], { colorLevel: level }));
        r(`color-level-${level}-styled`, ['colorLevel'], cfg([[
            ct('hex', { color: 'hex:FF8800', backgroundColor: 'hex:003366' }),
            sep(),
            ct('ansi256', { color: 'ansi256:208', backgroundColor: 'ansi256:17' }),
            sep(),
            ct('named', { color: 'brightGreen', backgroundColor: 'bgBrightBlack', bold: true }),
            sep(),
            ct('gradient text here', { color: 'gradient:hex:FF0000,hex:0000FF' })
        ]], { colorLevel: level }));
    }
    r('no-color-env', ['colorLevel', 'env'], cfg([STD_LINE]), { env: { NO_COLOR: '1' } });
    r('force-color-env', ['colorLevel', 'env'], cfg([STD_LINE], { colorLevel: 1 }), { env: { FORCE_COLOR: '3', COLORTERM: 'truecolor', TERM: 'xterm-256color' } });
    r('global-bold', ['globalBold'], cfg([STD_LINE], { globalBold: true }));
    r('inherit-separator-colors', ['separator', 'inheritSeparatorColors'], cfg([STD_LINE], { inheritSeparatorColors: true }));
    r('inherit-separator-colors-bg', ['separator', 'inheritSeparatorColors'], cfg([STD_ITEMS.flatMap(i => [i, sep()]).slice(0, -1)], { inheritSeparatorColors: true }));
    for (const [label, s] of [['pipe', '|'], ['space', ' '], ['comma', ','], ['dash', '-'], ['guillemet', '»'], ['spaced-pipe', ' | '], ['empty', '']]) {
        r(`default-separator-${label}`, ['separator', 'defaultSeparator'], cfg([STD_ITEMS.map(({ backgroundColor, ...i }) => i)], { defaultSeparator: s }));
    }
    r('default-separator-with-explicit-separators', ['separator', 'defaultSeparator'], cfg([STD_LINE], { defaultSeparator: '/' }));
    for (const side of ['both', 'left', 'right']) {
        r(`default-padding-${side}`, ['padding'], cfg([STD_LINE], { defaultPadding: ' ', defaultPaddingSide: side }));
        r(`default-padding-${side}-bg`, ['padding'], cfg([STD_ITEMS], { defaultPadding: ' ', defaultPaddingSide: side }));
    }
    r('default-padding-dots', ['padding'], cfg([STD_LINE], { defaultPadding: '..' }));
    for (const [label, color] of [['named', 'red'], ['hex', 'hex:FF8800'], ['ansi256', 'ansi256:208'], ['gradient', 'gradient:hex:FF0000,hex:0000FF'], ['gradient-preset', 'gradient:rainbow'], ['gradient-dash', 'gradient:FF0000-00FF00-0000FF']]) {
        for (const level of [1, 2, 3]) {
            r(`override-fg-${label}-level-${level}`, ['overrideForegroundColor', 'colorLevel'], cfg([STD_LINE], { overrideForegroundColor: color, colorLevel: level }));
        }
    }
    for (const [label, color] of [['named', 'bgBlue'], ['hex', 'hex:202020'], ['ansi256', 'ansi256:236']]) {
        for (const level of [1, 2, 3]) {
            r(`override-bg-${label}-level-${level}`, ['overrideBackgroundColor', 'colorLevel'], cfg([STD_LINE], { overrideBackgroundColor: color, colorLevel: level }));
        }
    }
    r('override-fg-and-bg', ['overrideForegroundColor', 'overrideBackgroundColor'], cfg([STD_ITEMS], { overrideForegroundColor: 'brightWhite', overrideBackgroundColor: 'bgBlack', defaultPadding: ' ' }));
    r('item-gradient-level-1', ['gradient', 'colorLevel'], cfg([[ct('gradient across words', { color: 'gradient:atlas' })]], { colorLevel: 1 }));
    r('item-gradient-bold', ['gradient'], cfg([[ct('bold gradient', { color: 'gradient:hex:00FF00,hex:FF00FF', bold: true })]], { colorLevel: 3 }));
    r('item-invalid-colors', ['colorLevel'], cfg([[ct('bad fg', { color: 'notacolor' }), sep(), ct('bad hex', { color: 'hex:XYZ', backgroundColor: 'bgNope' }), sep(), ct('bad gradient', { color: 'gradient:' })]]));

    const flexLine = [{ type: 'model', color: 'cyan' }, flex(), { type: 'session-cost', color: 'green' }];
    for (const mode of ['full', 'full-minus-40', 'full-until-compact']) {
        for (const width of [null, '40', '80', '200']) {
            r(`flex-${mode}-width-${width ?? 'unset'}`, ['flex', 'flexMode', 'flex-separator'], cfg([flexLine], { flexMode: mode }), width ? { env: { CCSTATUSLINE_WIDTH: width } } : {});
        }
        r(`flex-${mode}-width-120-high-context`, ['flex', 'flexMode', 'flex-separator'], cfg([flexLine], { flexMode: mode }), {
            env: { CCSTATUSLINE_WIDTH: '120' },
            stdin: richStdin(undefined, { context_window: { used_percentage: 75, remaining_percentage: 25 } })
        });
    }
    r('flex-until-compact-threshold-10', ['flex', 'flexMode'], cfg([flexLine], { flexMode: 'full-until-compact', compactThreshold: 10 }), { env: { CCSTATUSLINE_WIDTH: '100' } });
    r('width-invalid-env', ['flex', 'width'], cfg([flexLine]), { env: { CCSTATUSLINE_WIDTH: 'abc' } });
    r('width-zero-env', ['flex', 'width'], cfg([flexLine]), { env: { CCSTATUSLINE_WIDTH: '0' } });

    const longLine = [ct('The quick brown fox', { color: 'red' }), sep(), ct('jumps over the lazy dog', { color: 'green' }), sep(), { type: 'model' }, sep(), ct('and keeps on running far away')];
    for (const width of ['10', '20', '40', '60', '200']) {
        r(`truncate-width-${width}`, ['truncation', 'width'], cfg([longLine]), { env: { CCSTATUSLINE_WIDTH: width } });
        r(`truncate-powerline-width-${width}`, ['truncation', 'width', 'powerline'], cfg([STD_ITEMS.concat(STD_ITEMS)], { powerline: { enabled: true } }), { env: { CCSTATUSLINE_WIDTH: width } });
    }
    r('truncate-unicode-width-12', ['truncation', 'width'], cfg([[ct('日本語のテキスト🚀🚀 wide')]]), { env: { CCSTATUSLINE_WIDTH: '12' } });
    r('truncate-full-minus-40-width-60', ['truncation', 'flexMode'], cfg([longLine], { flexMode: 'full-minus-40' }), { env: { CCSTATUSLINE_WIDTH: '60' } });

    r('merge-true', ['merge'], cfg([[ct('A', { color: 'red', merge: true }), ct('B', { color: 'blue' }), sep(), ct('C')]]));
    r('merge-no-padding', ['merge', 'padding'], cfg([[ct('A', { color: 'red', merge: 'no-padding' }), ct('B', { color: 'blue' }), sep(), ct('C')]], { defaultPadding: ' ' }));
    r('merge-true-padding', ['merge', 'padding'], cfg([[ct('A', { color: 'red', merge: true }), ct('B', { color: 'blue' }), sep(), ct('C')]], { defaultPadding: ' ' }));
    r('merge-chain', ['merge'], cfg([[ct('A', { merge: true }), ct('B', { merge: 'no-padding' }), ct('C', { merge: true }), ct('D')]], { defaultPadding: ' ', defaultSeparator: '|' }));
    r('merge-powerline', ['merge', 'powerline'], cfg([[{ ...STD_ITEMS[0], merge: true }, STD_ITEMS[1], { ...STD_ITEMS[2], merge: 'no-padding' }, STD_ITEMS[3]]], { powerline: { enabled: true }, defaultPadding: ' ' }));

    // Powerline
    const pl = (powerline, extra = {}) => cfg([STD_ITEMS], merge({ powerline: { enabled: true, ...powerline }, defaultPadding: ' ' }, extra));
    r('powerline-default', ['powerline'], pl({}));
    r('powerline-no-padding', ['powerline'], cfg([STD_ITEMS], { powerline: { enabled: true } }));
    r('powerline-multiple-separators', ['powerline'], pl({ separators: ['', '', ''] }));
    r('powerline-invert-background', ['powerline'], pl({ separators: ['', ''], separatorInvertBackground: [true, false] }));
    r('powerline-caps', ['powerline', 'caps'], pl({ startCaps: [''], endCaps: [''] }));
    r('powerline-multiple-caps-3-lines', ['powerline', 'caps', 'multiline'], cfg([STD_ITEMS, STD_ITEMS.slice(0, 2), STD_ITEMS.slice(1)], { defaultPadding: ' ', powerline: { enabled: true, startCaps: ['', ''], endCaps: ['', ''] } }));
    r('powerline-with-separator-items', ['powerline', 'separator'], pl({}, { lines: [[STD_ITEMS[0], sep(), STD_ITEMS[1], flex(), STD_ITEMS[2]]].map(l => l.map((it, i) => ({ id: `p${i}`, ...it }))) }), { env: { CCSTATUSLINE_WIDTH: '80' } });
    r('powerline-flex-width-100', ['powerline', 'flex'], cfg([[STD_ITEMS[0], flex(), STD_ITEMS[1]]], { powerline: { enabled: true }, defaultPadding: ' ' }), { env: { CCSTATUSLINE_WIDTH: '100' } });
    r('powerline-items-without-bg', ['powerline'], cfg([STD_ITEMS.map(({ backgroundColor, ...i }) => i)], { powerline: { enabled: true }, defaultPadding: ' ' }));
    r('powerline-global-bold', ['powerline', 'globalBold'], pl({}, { globalBold: true }));
    r('powerline-override-colors', ['powerline', 'overrideForegroundColor', 'overrideBackgroundColor'], pl({}, { overrideForegroundColor: 'black', overrideBackgroundColor: 'bgYellow' }));
    r('powerline-empty-widget', ['powerline'], cfg([[STD_ITEMS[0], ct('', { backgroundColor: 'bgCyan' }), STD_ITEMS[1]]], { powerline: { enabled: true }, defaultPadding: ' ' }));
    for (const theme of THEMES) {
        for (const level of [1, 2, 3]) {
            r(`powerline-theme-${theme}-level-${level}`, ['powerline', 'theme', `theme:${theme}`, 'colorLevel'], pl({ theme }, { colorLevel: level }));
        }
    }
    r('powerline-theme-unknown', ['powerline', 'theme'], pl({ theme: 'no-such-theme' }));
    const threeLines = [STD_ITEMS, [STD_ITEMS[2], STD_ITEMS[0], STD_ITEMS[3]], [ct('x', { backgroundColor: 'bgBlue' }), ct('longer text', { backgroundColor: 'bgRed' })]];
    r('powerline-auto-align-3-lines', ['powerline', 'autoAlign', 'multiline'], cfg(threeLines, { powerline: { enabled: true, autoAlign: true }, defaultPadding: ' ' }));
    r('powerline-auto-align-exclude', ['powerline', 'autoAlign', 'multiline'], cfg([[{ ...STD_ITEMS[0], excludeFromAutoAlign: true }, STD_ITEMS[1]], [STD_ITEMS[2], STD_ITEMS[3]]], { powerline: { enabled: true, autoAlign: true }, defaultPadding: ' ' }));
    r('powerline-continue-theme-3-lines', ['powerline', 'continueThemeAcrossLines', 'theme', 'multiline'], cfg(threeLines, { powerline: { enabled: true, theme: 'nord', continueThemeAcrossLines: true }, defaultPadding: ' ' }));
    r('powerline-no-continue-theme-3-lines', ['powerline', 'theme', 'multiline'], cfg(threeLines, { powerline: { enabled: true, theme: 'nord' }, defaultPadding: ' ' }));
    r('powerline-level-0', ['powerline', 'colorLevel'], pl({}, { colorLevel: 0 }));

    // Lines
    r('multiple-lines', ['multiline'], cfg([STD_LINE, [{ type: 'session-clock', color: 'yellow' }, sep(), { type: 'session-usage' }], [ct('third line')]]));
    r('multiple-lines-empty-middle', ['multiline'], cfg([STD_LINE, [], [ct('third line')]]));
    r('multiple-lines-blank-output', ['multiline'], cfg([[ct('')], [ct('second')]]));
    r('separator-index-across-lines', ['multiline', 'separator'], cfg([[ct('a'), ct('b')], [ct('c'), ct('d')]], { defaultSeparator: '|' }));
    r('five-lines', ['multiline'], cfg([[ct('1')], [ct('2')], [ct('3')], [ct('4')], [ct('5')]]));
    r('minimalist-mode', ['minimalistMode'], cfg([[{ type: 'model' }, sep(), { type: 'context-percentage' }, sep(), { type: 'session-cost' }, sep(), { type: 'session-usage' }, sep(), { type: 'vim-mode' }]], { minimalistMode: true }));
    r('minimalist-mode-off', ['minimalistMode'], cfg([[{ type: 'model' }, sep(), { type: 'context-percentage' }, sep(), { type: 'session-cost' }, sep(), { type: 'session-usage' }, sep(), { type: 'vim-mode' }]], { minimalistMode: false }));

    // Widget styling
    r('widget-bold', ['bold'], cfg([[ct('bold', { bold: true, color: 'red' }), sep(), { type: 'model', bold: true }]]));
    r('widget-dim', ['dim'], cfg([[ct('dim', { dim: true, color: 'red' }), sep(), { type: 'model', dim: true }]]));
    r('widget-dim-parens', ['dim'], cfg([[ct('text (in parens) more', { dim: 'parens', color: 'cyan' }), sep(), { type: 'git-changes', dim: 'parens' }]]));
    r('widget-dim-parens-bold', ['dim', 'bold'], cfg([[ct('a (b) c', { dim: 'parens', bold: true })]]));
    r('widget-background', ['backgroundColor'], cfg([[ct('bg', { color: 'black', backgroundColor: 'bgYellow' }), sep(), ct('hexbg', { backgroundColor: 'hex:336699' })]]));
    r('widget-background-padding', ['backgroundColor', 'padding'], cfg([[ct('bg', { color: 'black', backgroundColor: 'bgYellow' }), sep(), ct('x')]], { defaultPadding: ' ' }));
    r('widget-raw-values', ['rawValue'], cfg([[{ type: 'model', rawValue: true }, sep(), { type: 'context-length', rawValue: true }, sep(), { type: 'session-cost', rawValue: true }]]));
    r('widget-bold-dim-bg-level-1', ['bold', 'dim', 'backgroundColor', 'colorLevel'], cfg([[ct('x', { bold: true, dim: true, color: 'green', backgroundColor: 'bgBlack' })]], { colorLevel: 1 }));
    r('widget-label-override', ['label'], cfg([[{ type: 'model', metadata: { label: 'M→ ' } }, sep(), { type: 'session-cost', metadata: { label: '' } }]]));
    r('unknown-widget-type', ['core'], cfg([[ct('before'), sep(), { type: 'no-such-widget' }, sep(), ct('after')]]));
    r('legacy-widget-type-git-pr', ['core', 'migration'], cfg([[{ type: 'git-pr' }]]));
    r('spaces-to-nbsp', ['core'], cfg([[ct('  many   spaces  ')]]));
}

// --- Config ----------------------------------------------------------------

function genConfig() {
    const stdin = richStdin();
    const migrationFiles = ['.claude/settings.json'];
    const c = (name, tags, extra) => add(`config/${name}`, plain({ tags: ['config', ...tags], stdin, ...extra }));

    c('v1-basic', ['migration', 'v1'], {
        config: {
            lines: [[{ id: 'old1', type: 'model', color: 'cyan' }, { id: 'old2', type: 'separator' }, { id: 'old3', type: 'context-length' }], [{ id: 'old4', type: 'custom-text', customText: 'line2' }]],
            colorLevel: 2,
            flexMode: 'full',
            globalBold: false
        },
        compareConfigFileAfter: true,
        normalizeGuids: true,
        compareFilesAfter: migrationFiles
    });
    c('v1-strips-separators', ['migration', 'v1'], {
        config: {
            lines: [[{ id: 'm', type: 'model' }, { id: 's', type: 'separator' }, { id: 'c', type: 'session-cost' }]],
            defaultSeparator: '|',
            defaultPadding: ' ',
            inheritSeparatorColors: true,
            overrideForegroundColor: 'red',
            compactThreshold: 50,
            unknownKey: 'dropped'
        },
        compareConfigFileAfter: true,
        normalizeGuids: true,
        compareFilesAfter: migrationFiles
    });
    c('v1-empty-object', ['migration', 'v1'], { config: {}, compareConfigFileAfter: true, normalizeGuids: true, compareFilesAfter: migrationFiles });
    c('v1-invalid-field-types', ['migration', 'v1', 'invalid'], {
        config: { lines: [[{ id: 'm', type: 'model' }]], colorLevel: 'high' },
        compareConfigFileAfter: true
    });
    c('v1-items-missing-id', ['migration', 'v1', 'invalid'], {
        config: { lines: [[{ type: 'model' }, 'junk', { notype: true }, { type: 42 }], 'not-a-line'] },
        compareConfigFileAfter: true,
        normalizeGuids: true,
        compareFilesAfter: migrationFiles
    });
    c('v1-separator-items-kept', ['migration', 'v1'], {
        config: { lines: [[{ id: 'a', type: 'model' }, { id: 'b', type: 'separator', character: '/' }, { id: 'c', type: 'version' }], []], globalBold: true },
        compareConfigFileAfter: true,
        normalizeGuids: true,
        compareFilesAfter: migrationFiles
    });
    c('v2-basic', ['migration', 'v2'], {
        config: { version: 2, lines: [[{ id: 'a', type: 'model' }, { id: 'b', type: 'separator' }, { id: 'c', type: 'block-timer' }]], colorLevel: 1 },
        compareConfigFileAfter: true,
        compareFilesAfter: migrationFiles
    });
    c('v2-with-updatemessage', ['migration', 'v2', 'updatemessage'], {
        config: { version: 2, lines: [[{ id: 'a', type: 'model' }]], updatemessage: { message: 'old message', remaining: 3 } },
        compareConfigFileAfter: true,
        compareFilesAfter: migrationFiles
    });
    c('v3-hide-flags', ['migration', 'v3'], {
        fixtures: ['no-git'],
        config: {
            version: 3,
            lines: [[
                { id: 'a', type: 'git-branch', metadata: { hideNoGit: 'true' } },
                { id: 'b', type: 'git-ahead-behind', metadata: { hideNoGit: 'false' } },
                { id: 'c', type: 'git-conflicts', metadata: { zeroDisplay: 'hidden' } },
                { id: 'd', type: 'cache-read', metadata: { hideWhenEmpty: 'true', hide: 'empty' } },
                { id: 'e', type: 'git-pr', metadata: { hideNoGit: 'true', hideStatus: 'true', other: 'keep' } },
                { id: 'f', type: 'compaction-counter', metadata: { hideZero: 'false' } },
                { id: 'g', type: 'git-ahead-behind', metadata: { hideNoGit: 'true' } },
                { id: 'h', type: 'model', metadata: { hideNoGit: 'true' } },
                { id: 'i', type: 'git-is-fork', metadata: { hideWhenNotFork: 'true' } },
                { id: 'j', type: 'extra-usage-used', metadata: { hideIfDisabled: 'true' } }
            ]]
        },
        compareConfigFileAfter: true,
        compareFilesAfter: migrationFiles
    });
    c('v3-no-lines', ['migration', 'v3'], { config: { version: 3, colorLevel: 3 }, compareConfigFileAfter: true, compareFilesAfter: migrationFiles });
    c('v4-unchanged', ['migration'], { config: cfg([[{ type: 'model' }]]), compareConfigFileAfter: true });
    c('version-5-newer', ['migration'], { config: { version: 5, lines: [[{ id: '1', type: 'model' }]] }, compareConfigFileAfter: true });
    c('version-string', ['invalid', 'migration'], { config: { version: '4', lines: [[{ id: '1', type: 'model' }]] }, compareConfigFileAfter: true });
    c('invalid-json', ['invalid'], { configRaw: '{ "version": 4, "lines": [[', compareConfigFileAfter: true });
    c('invalid-json-empty-file', ['invalid'], { configRaw: '', compareConfigFileAfter: true });
    c('invalid-json-level-0', ['invalid', 'colorLevel'], { configRaw: 'nope', compareConfigFileAfter: true, stdin: richStdin(undefined, { model: undefined }) });
    c('schema-error-color-level', ['invalid'], { config: { version: 4, colorLevel: 9 }, compareConfigFileAfter: true });
    c('schema-error-empty-lines', ['invalid'], { config: { version: 4, lines: [] }, compareConfigFileAfter: true });
    c('schema-error-lines-type', ['invalid'], { config: { version: 4, lines: 'x' }, compareConfigFileAfter: true });
    c('schema-error-item-missing-id', ['invalid'], { config: { version: 4, lines: [[{ type: 'model' }]] }, compareConfigFileAfter: true });
    c('schema-error-flex-mode', ['invalid'], { config: { version: 4, flexMode: 'wide' }, compareConfigFileAfter: true });
    c('schema-error-compact-threshold', ['invalid'], { config: { version: 4, compactThreshold: 100 }, compareConfigFileAfter: true });
    c('schema-error-badge-empty-render', ['invalid'], { config: { version: 4, colorLevel: 9 }, stdin: {} });
    c('json-array', ['invalid'], { configRaw: '[1,2,3]', compareConfigFileAfter: true });
    c('json-null', ['invalid'], { configRaw: 'null', compareConfigFileAfter: true });
    c('missing-file-explicit-path', ['missing'], { compareConfigFileAfter: true });
    c('missing-file-default-path', ['missing'], { noConfig: true, compareConfigFileAfter: true });
    c('default-path-existing', ['defaultPath'], { useDefaultConfigPath: true, config: cfg([[ct('from default path', { color: 'green' })]]), compareConfigFileAfter: true });
    c('default-path-v2-migrates', ['defaultPath', 'migration', 'v2'], { useDefaultConfigPath: true, config: { version: 2, lines: [[{ id: 'x', type: 'model' }]] }, compareConfigFileAfter: true, compareFilesAfter: migrationFiles });
    c('config-is-directory', ['invalid'], { files: { '${CASE_DIR}/config/settings.json/.keep': '' }, compareStderr: false });
    c('extra-keys-kept', [], { config: cfg([[{ type: 'model' }]], { someFutureKey: { a: 1 } }), compareConfigFileAfter: true });
    c('installation-metadata', [], { config: cfg([[{ type: 'model' }]], { installation: { method: 'pinned', installedVersion: '2.2.32' } }), compareConfigFileAfter: true });

    const um = (name, updatemessage, extra = {}) => c(`updatemessage-${name}`, ['updatemessage'], {
        config: cfg([[{ type: 'model' }]], { updatemessage }),
        compareConfigFileAfter: true,
        compareFilesAfter: migrationFiles,
        ...extra
    });
    um('remaining-2', { message: 'ccstatusline updated, see changelog', remaining: 2 });
    um('remaining-1', { message: 'last showing', remaining: 1 });
    um('remaining-0', { message: 'never shown', remaining: 0 });
    um('empty-message', { message: '   ', remaining: 5 });
    um('null-message', { message: null, remaining: 5 });
    um('control-chars', { message: 'hi\u001b[31m red\u0007', remaining: 2 });
    um('with-hook-widgets', { message: 'hooks', remaining: 2 }, {
        config: cfg([[{ type: 'skills' }]], { updatemessage: { message: 'hooks', remaining: 2 } }),
        files: { '.claude/settings.json': { json: { statusLine: { type: 'command', command: 'npx -y ccstatusline@latest' }, hooks: { Stop: [{ hooks: [{ type: 'command', command: 'echo keep' }] }] } } } }
    });
    um('existing-claude-settings', { message: 'keep other settings', remaining: 3 }, {
        files: { '.claude/settings.json': { json: { model: 'opus', permissions: { allow: ['Bash(ls)'] } } } }
    });
}

// --- Write -----------------------------------------------------------------

function main() {
    genCore();
    genWidgets();
    genRendering();
    genConfig();

    fs.rmSync(CORPUS_DIR, { recursive: true, force: true });
    for (const [name, c] of cases) {
        const { name: _name, ...body } = c;
        const file = path.join(CORPUS_DIR, `${name}.json`);
        fs.mkdirSync(path.dirname(file), { recursive: true });
        fs.writeFileSync(file, `${JSON.stringify({ name, ...body }, null, 2)}\n`);
    }
    console.log(`wrote ${cases.size} cases to ${path.relative(process.cwd(), CORPUS_DIR) || CORPUS_DIR}`);
}

main();
