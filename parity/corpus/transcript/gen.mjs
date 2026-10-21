// Generates the transcript-group parity cases (node parity/corpus/transcript/gen.mjs).
// Each case writes a small JSONL transcript (and subagent transcripts) into
// HOME and points stdin's transcript_path at it.
import fs from 'node:fs';
import path from 'node:path';
import { fileURLToPath } from 'node:url';

const OUT = path.dirname(fileURLToPath(import.meta.url));
const NOW = 1792584000000; // harness default CCSL_TEST_NOW_MS
const iso = (msBeforeNow) => new Date(NOW - msBeforeNow).toISOString();
const S = 1000;
const M = 60 * S;
const J = (o) => JSON.stringify(o);
const jsonl = (lines) => lines.map(l => (typeof l === 'string' ? l : J(l)) + '\n').join('');

const TP = '.claude/projects/-tmp-proj/session-1.jsonl';
const TP_ABS = '${HOME}/' + TP;
const SUB1 = '.claude/projects/-tmp-proj/subagents/agent-a1.jsonl';
const SUB2 = '.claude/projects/-tmp-proj/session-1/subagents/agent-a2.jsonl';
const SUB_UNREF = '.claude/projects/-tmp-proj/subagents/agent-zz.jsonl';

const usage = (input, output, read = 0, creation = 0) => ({
    input_tokens: input, output_tokens: output, cache_read_input_tokens: read, cache_creation_input_tokens: creation
});
const user = (ago, content, extra = {}) => ({ type: 'user', timestamp: iso(ago), message: { role: 'user', content }, ...extra });
const asst = (ago, u, extra = {}, msgExtra = {}) => ({
    type: 'assistant', timestamp: iso(ago), message: { role: 'assistant', usage: u, ...msgExtra }, ...extra
});

// The main fixture: every record kind the analyses read.
const MAIN = [
    user(65 * M, 'start the work'),
    asst(65 * M - 4 * S, usage(10, 50, 0, 2000), {}, { stop_reason: null, content: [{ type: 'text', text: 'thinking' }] }),
    asst(65 * M - 5 * S, usage(10, 120, 0, 2000), {}, { stop_reason: 'tool_use' }),
    '{"type":"assistant","timestamp":"2026-10-21T10:55:10.000Z","message":{"usage":{"input_tokens":99',
    asst(64 * M, usage(5000, 5000, 1, 1), { isSidechain: true, agentId: 'a1' }, { stop_reason: 'end_turn' }),
    asst(63 * M, usage(777, 777, 777, 777), { isApiErrorMessage: true }, { stop_reason: 'end_turn' }),
    user(62 * M, [{ type: 'tool_result', content: 'done' }], { toolUseResult: { agentId: 'a1', status: 'completed' } }),
    asst(62 * M - 3 * S, usage(4, 300, 2000, 150), {}, { stop_reason: 'end_turn' }),
    user(61 * M, '<local-command-stdout>Set effort level to medium</local-command-stdout>'),
    { type: 'custom-title', customTitle: 'My Session', sessionId: 'x' },
    user(60 * M, [{ type: 'tool_use', input: { nested: [{ agentId: 'a2' }] } }]),
    asst(60 * M - 7 * S, usage(6, 80, 2150, 10), {}, { stop_reason: 'end_turn' }),
    { type: 'system', subtype: 'compact_boundary', timestamp: iso(40 * M), compactMetadata: { trigger: 'auto', preTokens: 150000, postTokens: 30000 } },
    user(39 * M, 'after compaction'),
    asst(39 * M - 9 * S, usage(3, 40, 30000, 500), {}, { stop_reason: 'end_turn' }),
    { type: 'system', subtype: 'compact_boundary', timestamp: iso(20 * M), compactMetadata: { preTokens: 80000 } },
    { type: 'system', subtype: 'compact_boundary', isSidechain: true, compactMetadata: { trigger: 'manual', preTokens: 5, postTokens: 1 } },
    { type: 'system', subtype: 'compact_boundary', timestamp: iso(19 * M), compactMetadata: { trigger: 'manual', preTokens: 60000, postTokens: 12000 } },
    user(3 * M, 'last question'),
    asst(3 * M - 20 * S, usage(2, 64, 12000, 300), {}, { stop_reason: null }),
    asst(3 * M - 25 * S, usage(2, 128, 12000, 300), {}, { stop_reason: 'end_turn' }),
    user(2 * M + 30 * S, '\u001b[1m<local-command-stdout>Set model to Opus 4.1 with xhigh effort</local-command-stdout>\u001b[22m'),
    user(2 * M, 'one more'),
    asst(90 * S, usage(1, 32, 12300, 64), {}, { stop_reason: 'end_turn' })
];

const SUBAGENT_1 = [
    { type: 'user', isSidechain: true, agentId: 'a1', timestamp: iso(64 * M + 30 * S), message: { content: 'sub task' } },
    { type: 'assistant', isSidechain: true, agentId: 'a1', timestamp: iso(64 * M + 10 * S), message: { usage: usage(100, 400) } },
    { type: 'user', isSidechain: true, agentId: 'a1', timestamp: iso(62 * M + 2 * S), message: { content: 'next' } },
    { type: 'assistant', isSidechain: true, agentId: 'a1', timestamp: iso(62 * M - 10 * S), message: { usage: usage(50, 900) } },
    { type: 'assistant', isSidechain: true, isApiErrorMessage: true, timestamp: iso(62 * M - 11 * S), message: { usage: usage(9, 9) } }
];

const SUBAGENT_2 = [
    { type: 'user', isSidechain: true, timestamp: iso(100 * S), message: { content: 'late sub' } },
    { type: 'assistant', isSidechain: true, timestamp: iso(70 * S), message: { usage: usage(20, 600) } },
    { type: 'assistant', isSidechain: true, timestamp: iso(65 * S), message: { usage: usage(20, 300) } }
];

const UNREFERENCED = [
    { type: 'user', timestamp: iso(10 * S), message: { content: 'x' } },
    { type: 'assistant', timestamp: iso(5 * S), message: { usage: usage(99999, 99999) } }
];

const mainFiles = (lines = MAIN, extra = {}) => ({
    [TP]: jsonl(lines),
    [SUB1]: jsonl(SUBAGENT_1),
    [SUB2]: jsonl(SUBAGENT_2),
    [SUB_UNREF]: jsonl(UNREFERENCED),
    ...extra
});

const baseStdin = (extra = {}) => ({
    hook_event_name: 'Status',
    session_id: 'parity-session-0001',
    transcript_path: TP_ABS,
    cwd: '${CASE_DIR}',
    model: { id: 'claude-opus-4-1-20250805', display_name: 'Opus 4.1' },
    workspace: { current_dir: '${CASE_DIR}', project_dir: '${CASE_DIR}' },
    version: '2.0.37',
    ...extra
});

const STDIN_CTX = {
    context_window: {
        context_window_size: 200000,
        total_input_tokens: 45678,
        total_output_tokens: 12345,
        current_usage: { input_tokens: 1234, output_tokens: 567, cache_creation_input_tokens: 8910, cache_read_input_tokens: 34567 }
    },
    cost: { total_cost_usd: 1.5, total_duration_ms: 3723000 },
    effort: { level: 'high' }
};

let n = 0;
const item = (type, extra = {}) => ({ id: String(++n), type, ...extra });
const config = (lines, extra = {}) => ({ version: 4, colorLevel: 0, lines, ...extra });
const cases = [];
const add = (name, tags, def) => cases.push({ name: `transcript/${name}`, tags: ['transcript', ...tags], ...def });

const TOKEN_TYPES = ['tokens-input', 'tokens-output', 'tokens-cached', 'tokens-total'];
const tokenLine = (meta = {}) => TOKEN_TYPES.map(t => item(t, meta));

// ---- token counts
add('tokens/full', TOKEN_TYPES, { files: mainFiles(), stdin: baseStdin(), config: config([tokenLine()]) });
add('tokens/raw-compact', [...TOKEN_TYPES, 'rawValue', 'numberFormat'], {
    files: mainFiles(),
    stdin: baseStdin(),
    config: config([TOKEN_TYPES.map(t => item(t, { rawValue: true, numberFormat: { style: 'compact', decimals: 2 } }))])
});
add('tokens/with-stdin-context', TOKEN_TYPES, { files: mainFiles(), stdin: baseStdin(STDIN_CTX), config: config([tokenLine()]) });
add('tokens/global-number-format', TOKEN_TYPES, {
    files: mainFiles(),
    stdin: baseStdin(),
    config: config([tokenLine()], { numberFormat: { token: { style: 'whole' } } })
});
add('tokens/missing-transcript', TOKEN_TYPES, { stdin: baseStdin(STDIN_CTX), config: config([tokenLine(), tokenLine({ hide: 'zero' })]) });
add('tokens/empty-transcript', TOKEN_TYPES, {
    files: { [TP]: '' },
    stdin: baseStdin(STDIN_CTX),
    config: config([tokenLine(), tokenLine({ hide: 'zero' })])
});
add('tokens/transcript-is-directory', TOKEN_TYPES, {
    files: { [TP + '/x']: '' },
    stdin: baseStdin(STDIN_CTX),
    config: config([tokenLine()])
});
add('tokens/empty-transcript-path', TOKEN_TYPES, {
    stdin: baseStdin({ ...STDIN_CTX, transcript_path: '' }),
    config: config([tokenLine()])
});
add('tokens/relative-transcript-path', TOKEN_TYPES, {
    files: { '${CASE_DIR}/rel/t.jsonl': jsonl(MAIN) },
    stdin: baseStdin({ transcript_path: 'rel/t.jsonl' }),
    config: config([tokenLine()])
});

// with and without transcript_path: cached/total come only from the transcript
for (const t of ['tokens-cached', 'tokens-total']) {
    add(`tokens/${t}-with-transcript-path`, [t], { files: mainFiles(), stdin: baseStdin(STDIN_CTX), config: config([[item(t), item(t, { rawValue: true })]]) });
    add(`tokens/${t}-without-transcript-path`, [t], {
        files: mainFiles(),
        stdin: baseStdin({ ...STDIN_CTX, transcript_path: undefined }),
        config: config([[item(t), item(t, { rawValue: true }), item('context-window')]])
    });
}
add('tokens/all-without-transcript-path', TOKEN_TYPES, {
    stdin: baseStdin({ ...STDIN_CTX, transcript_path: undefined }),
    config: config([tokenLine(), tokenLine({ hide: 'zero' })])
});

// stop_reason variants
const NO_STOP = [
    user(10 * M, 'a'),
    asst(10 * M - S, usage(1, 2, 3, 4)),
    asst(10 * M - 2 * S, usage(10, 20, 30, 40), { isSidechain: true }),
    asst(10 * M - 3 * S, usage(100, 200, 300, 400))
];
add('tokens/no-stop-reason', TOKEN_TYPES.concat('context-length'), {
    files: { [TP]: jsonl(NO_STOP) },
    stdin: baseStdin(),
    config: config([[...tokenLine(), item('context-length')]])
});
const LAST_NULL = [
    user(10 * M, 'a'),
    asst(10 * M - S, usage(1, 2, 3, 4), {}, { stop_reason: 'end_turn' }),
    asst(10 * M - 2 * S, usage(10, 20, 30, 40), {}, { stop_reason: null }),
    asst(10 * M - 3 * S, usage(100, 200, 300, 400), {}, { stop_reason: null })
];
add('tokens/last-stop-null', TOKEN_TYPES.concat('context-length'), {
    files: { [TP]: jsonl(LAST_NULL) },
    stdin: baseStdin(),
    config: config([[...tokenLine(), item('context-length')]])
});
const FALSY_STOP = [
    asst(10 * M - S, usage(1, 2, 3, 4), {}, { stop_reason: '' }),
    asst(10 * M - 2 * S, usage(10, 20, 30, 40), {}, { stop_reason: 0 }),
    asst(10 * M - 3 * S, usage(100, 200, 300, 400), {}, { stop_reason: false }),
    asst(10 * M - 4 * S, usage(1000, 2000, 3000, 4000), {}, { stop_reason: ['x'] })
];
add('tokens/falsy-stop-reasons', TOKEN_TYPES, { files: { [TP]: jsonl(FALSY_STOP) }, stdin: baseStdin(), config: config([tokenLine()]) });

// odd records
const ODD = [
    'null',
    '[]',
    '[{"agentId":"a1"}]',
    '42',
    '"just a string"',
    '{"type":"assistant","message":"not an object","timestamp":"2026-10-21T11:00:00.000Z"}',
    '{"type":"assistant","timestamp":"2026-10-21T11:00:01.000Z","message":{"usage":"string usage"}}',
    '{"type":"assistant","timestamp":"2026-10-21T11:00:02.000Z","message":{"usage":{"input_tokens":-5,"output_tokens":"7","cache_read_input_tokens":1e400,"cache_creation_input_tokens":2.5}}}',
    '{"type":"assistant","timestamp":"2026-10-21T11:00:03.000Z","message":{"usage":{"input_tokens":1,"output_tokens":1}},"message":{"usage":{"input_tokens":1000,"output_tokens":3}}}',
    '{"type":"assistant","timestamp":"not a date","message":{"usage":{"input_tokens":5,"output_tokens":5}}}',
    '{"type":"assistant","timestamp":1792583000000,"message":{"usage":{"input_tokens":5,"output_tokens":5}}}',
    '{"type":"custom-title","customTitle":"surrogate \\ud83d\\ude00 and lone \\ud800 end"}',
    '{"type":"custom-title","customTitle":""}',
    '{"type":"custom-title","customTitle":42}',
    '   ',
    '{"type":"assistant","timestamp":"2026-10-21T11:00:04.000Z","message":{"usage":{"input_tokens":11,"output_tokens":11}}} trailing',
    ' {"type":"assistant","timestamp":"2026-10-21T11:00:05.000Z","message":{"usage":{"input_tokens":13,"output_tokens":13}}}',
    '{"type":"assistant","timestamp":"2026-10-21T11:00:06.000Z","message":{"usage":{"input_tokens":17,"output_tokens":17},"stop_reason":null},"isSidechain":"true"}'
];
add('odd/records', [...TOKEN_TYPES, 'session-name', 'context-length', 'session-clock'], {
    files: { [TP]: jsonl(ODD) },
    stdin: baseStdin(),
    config: config([[...tokenLine(), item('context-length')], [item('session-name'), item('session-clock')]])
});
add('odd/bom-crlf', [...TOKEN_TYPES, 'session-name', 'session-clock'], {
    files: { [TP]: '﻿' + MAIN.map(l => (typeof l === 'string' ? l : J(l)) + '\r\n').join('') },
    stdin: baseStdin(),
    config: config([tokenLine(), [item('session-name'), item('session-clock'), item('compaction-counter')]])
});
add('odd/bom-not-first-line', [...TOKEN_TYPES], {
    files: { [TP]: jsonl([MAIN[0]]) + '﻿' + jsonl([MAIN[1]]) + '\n\n' + jsonl([MAIN[2]]) },
    stdin: baseStdin(),
    config: config([tokenLine()])
});
add('odd/unterminated-last-line', [...TOKEN_TYPES, 'session-name', 'compaction-counter'], {
    files: { [TP]: jsonl(MAIN) + J({ type: 'custom-title', customTitle: 'Unterminated' }) },
    stdin: baseStdin(),
    config: config([tokenLine(), [item('session-name'), item('compaction-counter')]])
});
add('odd/unterminated-garbage', [...TOKEN_TYPES], {
    files: { [TP]: jsonl(MAIN) + '{"type":"assistant","message":{"usage":{"input_tokens":5' },
    stdin: baseStdin(),
    config: config([tokenLine()])
});

// ---- context widgets
const CTX_TYPES = ['context-length', 'context-percentage', 'context-percentage-usable', 'context-window'];
const ctxLine = () => [
    item('context-length'),
    item('context-percentage'),
    item('context-percentage', { metadata: { inverse: 'true' } }),
    item('context-percentage', { metadata: { display: 'slider' } }),
    item('context-percentage-usable'),
    item('context-percentage-usable', { metadata: { display: 'slider-only', inverse: 'true' } }),
    item('context-window')
];
add('context/from-transcript', CTX_TYPES, { files: mainFiles(), stdin: baseStdin(), config: config([ctxLine()]) });
add('context/from-stdin', CTX_TYPES, { files: mainFiles(), stdin: baseStdin(STDIN_CTX), config: config([ctxLine()]) });
add('context/model-1m', CTX_TYPES, {
    files: mainFiles(),
    stdin: baseStdin({ model: { id: 'claude-sonnet-4-5[1m]', display_name: 'Sonnet 4.5 (1M context)' } }),
    config: config([ctxLine()])
});
add('context/boundary-after-last-usage', CTX_TYPES, {
    files: {
        [TP]: jsonl([
            user(10 * M, 'a'),
            asst(10 * M - S, usage(100, 20, 90000, 5000), {}, { stop_reason: 'end_turn' }),
            { type: 'system', subtype: 'compact_boundary', compactMetadata: { trigger: 'auto', preTokens: 95000, postTokens: 18000 } }
        ])
    },
    stdin: baseStdin(),
    config: config([ctxLine()])
});
add('context/boundary-without-post-tokens', CTX_TYPES, {
    files: {
        [TP]: jsonl([
            asst(10 * M - S, usage(100, 20, 90000, 5000), {}, { stop_reason: 'end_turn' }),
            { type: 'system', subtype: 'compact_boundary', compactMetadata: { trigger: 'auto', preTokens: 95000 } }
        ])
    },
    stdin: baseStdin(),
    config: config([ctxLine()])
});
add('context/missing-transcript', CTX_TYPES, { stdin: baseStdin(), config: config([ctxLine()]) });
add('context/flex-until-compact', ['context-percentage', 'flex-separator'], {
    files: mainFiles(),
    stdin: baseStdin(),
    env: { CCSTATUSLINE_WIDTH: '80' },
    config: config([[item('context-percentage'), item('flex-separator'), item('context-length')]], { flexMode: 'full-until-compact', compactThreshold: 5 })
});

// ---- cache widgets
const CACHE_TYPES = ['cache-hit-rate', 'cache-read', 'cache-write'];
const cacheLine = (meta) => CACHE_TYPES.map(t => item(t, { metadata: meta }));
add('cache/session-scope', CACHE_TYPES, {
    files: mainFiles(),
    stdin: baseStdin(STDIN_CTX),
    config: config([cacheLine({ cacheScopeSession: 'true' }), cacheLine({ cacheScopeSession: 'true', hide: 'empty' }), cacheLine(undefined)])
});
add('cache/session-scope-empty', CACHE_TYPES, {
    files: { [TP]: jsonl([user(M, 'x'), asst(50 * S, usage(5, 5))]) },
    stdin: baseStdin(),
    config: config([cacheLine({ cacheScopeSession: 'true' }), cacheLine({ cacheScopeSession: 'true', hide: 'empty' })])
});
add('cache/session-scope-no-transcript', CACHE_TYPES, {
    stdin: baseStdin({ transcript_path: undefined }),
    config: config([cacheLine({ cacheScopeSession: 'true' })])
});

// ---- speed
const SPEED_TYPES = ['input-speed', 'output-speed', 'total-speed'];
const speedLine = (meta) => SPEED_TYPES.map(t => item(t, { metadata: meta }));
for (const w of ['0', '30', '60', '120', '500', 'abc', '-5', '45.9']) {
    add(`speed/window-${w}`, SPEED_TYPES, {
        files: mainFiles(),
        stdin: baseStdin(),
        config: config([speedLine({ windowSeconds: w })])
    });
}
add('speed/session-average', SPEED_TYPES, { files: mainFiles(), stdin: baseStdin(), config: config([speedLine(undefined)]) });
add('speed/no-subagent-files', SPEED_TYPES, { files: { [TP]: jsonl(MAIN) }, stdin: baseStdin(), config: config([speedLine(undefined), speedLine({ windowSeconds: '120' })]) });
add('speed/number-format', [...SPEED_TYPES, 'numberFormat'], {
    files: mainFiles(),
    stdin: baseStdin(),
    config: config([SPEED_TYPES.map(t => item(t, { numberFormat: { style: 'compact', decimals: 3 } })), speedLine({ windowSeconds: '10', hide: 'no-data' })])
});
add('speed/fast-k', SPEED_TYPES, {
    files: { [TP]: jsonl([user(10 * S, 'q'), asst(9 * S, usage(5_000_000, 1_999_500))]) },
    stdin: baseStdin(),
    config: config([speedLine(undefined), SPEED_TYPES.map(t => item(t, { numberFormat: { style: 'whole' } }))])
});
add('speed/no-data', SPEED_TYPES, {
    files: { [TP]: jsonl([user(10 * S, 'q')]) },
    stdin: baseStdin(),
    config: config([speedLine(undefined), speedLine({ hide: 'no-data' }), speedLine({ windowSeconds: '30', hide: 'no-data' })])
});
add('speed/missing-transcript', SPEED_TYPES, { stdin: baseStdin(), config: config([speedLine(undefined), speedLine({ windowSeconds: '5' })]) });
add('speed/no-transcript-path', SPEED_TYPES, { stdin: baseStdin({ transcript_path: undefined }), config: config([speedLine(undefined)]) });
// overlapping and out-of-order intervals across files
add('speed/overlapping-intervals', SPEED_TYPES, {
    files: {
        [TP]: jsonl([
            user(100 * S, 'q1'),
            asst(80 * S, usage(100, 1000)),
            asst(70 * S, usage(100, 1000)),
            user(90 * S, 'out of order'),
            asst(85 * S, usage(10, 10)),
            user(60 * S, 'q2', { isSidechain: true }),
            asst(40 * S, usage(100, 1000), { isApiErrorMessage: 1 }),
            user(30 * S, 'q3'),
            asst(30 * S, usage(100, 100)),
            asst(10 * S, usage(100, 2000)),
            { type: 'system', timestamp: iso(2 * S) },
            user(5 * S, [{ type: 'tool_result', agentId: 'a1' }])
        ]),
        [SUB1]: jsonl([
            { type: 'user', isSidechain: true, timestamp: iso(95 * S) },
            { type: 'assistant', isSidechain: true, timestamp: iso(75 * S), message: { usage: usage(1, 500) } },
            { type: 'assistant', isSidechain: true, timestamp: iso(1 * S), message: { usage: usage(1, 500) } }
        ])
    },
    stdin: baseStdin(),
    config: config([speedLine(undefined), speedLine({ windowSeconds: '20' }), speedLine({ windowSeconds: '90' })])
});

// ---- session clock
add('session-clock/from-transcript', ['session-clock'], {
    files: mainFiles(),
    stdin: baseStdin(),
    config: config([[item('session-clock'), item('session-clock', { rawValue: true }), item('session-clock', { metadata: { hide: 'zero' } })]])
});
add('session-clock/from-stdin', ['session-clock'], {
    files: mainFiles(),
    stdin: baseStdin({ cost: { total_duration_ms: 59999 } }),
    config: config([[item('session-clock'), item('session-clock', { metadata: { hide: 'zero' } })]])
});
add('session-clock/short-transcript', ['session-clock'], {
    files: { [TP]: jsonl([user(5 * M, 'a'), asst(5 * M - 30 * S, usage(1, 1)), { type: 'system' }]) },
    stdin: baseStdin(),
    config: config([[item('session-clock'), item('session-clock', { metadata: { hide: 'zero' } })]])
});
add('session-clock/long-transcript', ['session-clock'], {
    files: { [TP]: jsonl([user(26 * 60 * M, 'a'), asst(60 * M, usage(1, 1)), '{"timestamp":"x"}', user(0, 'now')]) },
    stdin: baseStdin(),
    config: config([[item('session-clock')]])
});
add('session-clock/missing-transcript', ['session-clock'], {
    stdin: baseStdin(),
    config: config([[item('session-clock'), item('session-clock', { metadata: { hide: 'zero' } }), item('session-name')]])
});

// ---- session name
add('session-name/full', ['session-name'], {
    files: mainFiles(),
    stdin: baseStdin(),
    config: config([[item('session-name'), item('session-name', { rawValue: true }), item('session-name', { metadata: { label: 'Name » ' } })]])
});
add('session-name/none', ['session-name'], {
    files: { [TP]: jsonl(NO_STOP) },
    stdin: baseStdin(),
    config: config([[item('context-window'), item('session-name')]])
});

// ---- compaction counter
const compactionLine = () => [
    item('compaction-counter'),
    item('compaction-counter', { metadata: { format: 'text-and-number' } }),
    item('compaction-counter', { metadata: { format: 'number', nerdFont: 'true' } }),
    item('compaction-counter', { metadata: { nerdFont: 'true', showTriggers: 'true', showReclaimed: 'true' } }),
    item('compaction-counter', { metadata: { showReclaimed: 'true', symbolReclaimed: '' } }),
    item('compaction-counter', { metadata: { metric: 'auto' } }),
    item('compaction-counter', { metadata: { metric: 'manual' } }),
    item('compaction-counter', { metadata: { metric: 'unknown', hide: 'zero' } }),
    item('compaction-counter', { metadata: { metric: 'reclaimed' }, numberFormat: { style: 'compact' } })
];
add('compaction/full', ['compaction-counter'], { files: mainFiles(), stdin: baseStdin(), config: config([compactionLine()]) });
add('compaction/none', ['compaction-counter'], {
    files: { [TP]: jsonl(NO_STOP) },
    stdin: baseStdin(),
    config: config([compactionLine(), [item('compaction-counter', { metadata: { hide: 'zero' } }), item('context-window')]])
});
add('compaction/missing-transcript', ['compaction-counter'], { stdin: baseStdin(), config: config([compactionLine()]) });
add('compaction/no-transcript-path', ['compaction-counter'], { stdin: baseStdin({ transcript_path: undefined }), config: config([compactionLine()]) });

// ---- thinking effort
const effortLine = () => [[item('thinking-effort'), item('thinking-effort', { rawValue: true })]];
add('thinking/from-transcript', ['thinking-effort'], { files: mainFiles(), stdin: baseStdin(), config: config(effortLine()) });
add('thinking/from-stdin', ['thinking-effort'], { files: mainFiles(), stdin: baseStdin({ effort: { level: 'LOW' } }), config: config(effortLine()) });
add('thinking/stdin-null-level', ['thinking-effort'], { files: mainFiles(), stdin: baseStdin({ effort: { level: null } }), config: config(effortLine()) });
add('thinking/stdin-invalid-level', ['thinking-effort'], { files: mainFiles(), stdin: baseStdin({ effort: { level: 'Weird!!' } }), config: config(effortLine()) });
add('thinking/stdin-no-level', ['thinking-effort'], { files: mainFiles(), stdin: baseStdin({ effort: {} }), config: config(effortLine()) });
add('thinking/settings-fallback', ['thinking-effort'], {
    files: { [TP]: jsonl(NO_STOP), '.claude/settings.json': J({ effortLevel: 'ultra-think' }) },
    stdin: baseStdin(),
    config: config(effortLine())
});
add('thinking/model-without-effort-clears', ['thinking-effort'], {
    files: {
        [TP]: jsonl([
            user(5 * M, '<local-command-stdout>Set effort level to max</local-command-stdout>'),
            user(4 * M, '<local-command-stdout>Set model to sonnet</local-command-stdout>')
        ]),
        '.claude/settings.json': J({ effortLevel: 'low' })
    },
    stdin: baseStdin(),
    config: config(effortLine())
});
add('thinking/effort-variants', ['thinking-effort'], {
    files: {
        [TP]: jsonl([
            user(5 * M, '<local-command-stdout>Set effort level to super-max_</local-command-stdout>'),
            user(4 * M, { not: 'a string' }),
            user(3 * M, '  <local-command-stdout>Set effort level to x</local-command-stdout>')
        ])
    },
    stdin: baseStdin(),
    config: config(effortLine())
});
add('thinking/unknown-effort', ['thinking-effort'], {
    files: { [TP]: jsonl([user(5 * M, '<local-command-stdout>Set effort level to Turbo-9 now</local-command-stdout>')]) },
    stdin: baseStdin(),
    config: config(effortLine())
});

// ---- cache timer
const timerLine = () => [[
    item('cache-timer'),
    item('cache-timer', { metadata: { ttlSeconds: '3600' } }),
    item('cache-timer', { rawValue: true, metadata: { symbolFresh: '', symbolDraining: 'D', symbolUrgent: 'U', symbolCold: 'C', symbolHot: 'H' } }),
    item('cache-timer', { metadata: { ttlSeconds: '4', hide: 'empty' } })
]];
const timerCase = (name, lines, extra = {}) => add(`cache-timer/${name}`, ['cache-timer'], {
    files: { [TP]: typeof lines === 'string' ? lines : jsonl(lines) },
    stdin: baseStdin(),
    config: config(timerLine()),
    ...extra
});
timerCase('fresh', MAIN);
timerCase('draining', [user(5 * M, 'q'), asst(3 * M, usage(1, 1, 50, 0))]);
timerCase('urgent', [user(5 * M, 'q'), asst(4 * M + 30 * S, usage(1, 1, 0, 50))]);
timerCase('cold', [user(50 * M, 'q'), asst(40 * M, usage(1, 1, 50, 0))]);
timerCase('working', [asst(3 * M, usage(1, 1, 50, 0)), user(2 * M, 'next question')]);
timerCase('interrupted', [asst(3 * M, usage(1, 1, 50, 0)), user(2 * M, [{ type: 'text', text: '[Request interrupted by user]' }])]);
timerCase('local-command', [asst(3 * M, usage(1, 1, 50, 0)), user(2 * M, '<command-name>/cost</command-name>'), user(M, '  <local-command-stdout>x</local-command-stdout>')]);
timerCase('api-error-and-no-cache', [
    user(10 * M, 'q'),
    asst(4 * M, usage(1, 1, 50, 0)),
    asst(3 * M, usage(1, 1, 0, 0)),
    asst(2 * M, usage(1, 1, 50, 0), { isApiErrorMessage: true })
]);
timerCase('sidechain-tail', [asst(2 * M, usage(1, 1, 50, 0)), user(M, 'side', { isSidechain: true })]);
timerCase('no-usage-row', [user(4 * M, 'q'), { type: 'assistant', timestamp: iso(3 * M), message: { content: 'x' } }]);
timerCase('string-usage-values', [user(4 * M, 'q'), { type: 'assistant', timestamp: iso(3 * M), message: { usage: { cache_read_input_tokens: '5' } } }]);
timerCase('numeric-timestamp', [user(4 * M, 'q'), { type: 'assistant', timestamp: NOW - 100 * S, message: { usage: usage(1, 1, 5, 0) } }]);
timerCase('bad-timestamp', [asst(4 * M, usage(1, 1, 5, 0)), { type: 'assistant', timestamp: 'garbage', message: { usage: usage(1, 1, 5, 0) } }]);
timerCase('only-system', [{ type: 'system' }, { type: 'summary' }]);
timerCase('empty', '');
timerCase('big-last-line', jsonl([user(5 * M, 'q'), asst(3 * M, usage(1, 1, 5, 0)), user(2 * M, [{ type: 'text', text: 'x'.repeat(100000) }], { isSidechain: true }), { type: 'system', content: 'y'.repeat(70000) }]));
timerCase('trailing-partial', jsonl([user(5 * M, 'q'), asst(3 * M, usage(1, 1, 5, 0))]) + '{"type":"user","message":{"content":"half');
timerCase('trailing-complete-no-newline', jsonl([asst(3 * M, usage(1, 1, 5, 0))]) + J(user(M, 'q')));
timerCase('crlf-bom', '﻿' + [user(5 * M, 'q'), asst(3 * M, usage(1, 1, 5, 0))].map(l => J(l) + '\r\n').join(''));
add('cache-timer/missing-transcript', ['cache-timer'], { stdin: baseStdin(), config: config(timerLine()) });
add('cache-timer/no-transcript-path', ['cache-timer'], { stdin: baseStdin({ transcript_path: undefined }), config: config(timerLine()) });

// ---- skills
const SKILLS_FILE = '.cache/ccstatusline/skills/skills-parity-session-0001.jsonl';
const skillsLine = (meta = {}) => [
    item('skills', { metadata: { ...meta } }),
    item('skills', { metadata: { mode: 'count', ...meta } }),
    item('skills', { metadata: { mode: 'list', ...meta } }),
    item('skills', { rawValue: true, metadata: { mode: 'list', listLimit: '2', ...meta } }),
    item('skills', { rawValue: true, metadata: { mode: 'bogus', ...meta } })
];
const skillRec = (skill, extra = {}) => J({ timestamp: iso(M), session_id: 'parity-session-0001', skill, source: 'PreToolUse', ...extra });
add('skills/log', ['skills'], {
    files: {
        [SKILLS_FILE]: [
            '﻿' + skillRec('commit'),
            'garbage',
            skillRec('review-pr'),
            '',
            J({ skill: 'no-session' }),
            skillRec(''),
            '   ',
            skillRec('commit') + '\r',
            J({ timestamp: 'x', session_id: 7, skill: 'bad-session' }),
            skillRec('deploy'),
            '[1]',
            'null'
        ].join('\n') + '\n\n'
    },
    stdin: baseStdin(),
    config: config([skillsLine(), skillsLine({ hide: 'empty' })])
});
add('skills/last-empty-name', ['skills'], {
    files: { [SKILLS_FILE]: [skillRec('commit'), skillRec('')].join('\n') },
    stdin: baseStdin(),
    config: config([skillsLine(), skillsLine({ hide: 'empty' })])
});
add('skills/none', ['skills'], { stdin: baseStdin(), config: config([skillsLine(), skillsLine({ hide: 'empty' })]) });
add('skills/unsafe-session-id', ['skills'], {
    files: { '.cache/ccstatusline/skills/skills-a.b.jsonl': skillRec('x') },
    stdin: baseStdin({ session_id: 'a.b' }),
    config: config([skillsLine()])
});

// ---- everything at once, as a real config would
add('all/kitchen-sink', ['all'], {
    files: mainFiles(),
    stdin: baseStdin(),
    config: config([
        tokenLine(),
        [...speedLine(undefined), ...speedLine({ windowSeconds: '60' })],
        [item('session-clock'), item('session-name'), item('compaction-counter'), item('thinking-effort'), item('cache-timer')],
        [...ctxLine(), ...cacheLine({ cacheScopeSession: 'true' })]
    ])
});
add('all/powerline', ['all', 'powerline'], {
    files: mainFiles(),
    stdin: baseStdin(STDIN_CTX),
    config: config([[...tokenLine(), item('context-percentage'), item('session-clock'), item('output-speed')]], {
        colorLevel: 2,
        powerline: { enabled: true },
        defaultPadding: ' '
    })
});

// write one file per case
fs.rmSync(path.join(OUT, 'cases'), { recursive: true, force: true });
for (const c of cases) {
    const file = path.join(OUT, 'cases', c.name.replace(/^transcript\//, '') + '.json');
    fs.mkdirSync(path.dirname(file), { recursive: true });
    fs.writeFileSync(file, JSON.stringify(c, null, 2) + '\n');
}
console.log(`${cases.length} cases`);
