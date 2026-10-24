#!/usr/bin/env node
// Renders Box/Text trees through the Ink bundled in pinned upstream, at chalk level 2, and records each frame,
// for the Rust layout engine's test (src/tui/ink/layout_oracle_tests.rs). Trees use only the props upstream's
// screens use; some are written by hand, the rest are random (seeded, so the output is stable).
//
//   npm ci --prefix parity/upstream --ignore-scripts
//   node tools/gen-ink-layout.mjs > src/tui/testdata/ink-layout.json
import * as fs from 'node:fs';
import * as path from 'node:path';
import { PassThrough } from 'node:stream';
import { fileURLToPath, pathToFileURL } from 'node:url';

// Ink's chalk detects its level when the bundle loads: pin it to 2 whatever terminal runs this.
for (const key of ['COLORTERM', 'TERM_PROGRAM', 'TERM_PROGRAM_VERSION', 'CI', 'TF_BUILD', 'TEAMCITY_VERSION']) {
    delete process.env[key];
}
process.env.TERM = 'xterm-256color';
process.env.FORCE_COLOR = '2';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DIST = path.join(REPO, 'parity', 'upstream', 'node_modules', 'ccstatusline', 'dist');
const chunk = fs.readdirSync(DIST).find(f => fs.readFileSync(path.join(DIST, f), 'utf8').includes('var render_default = render;'));
const m = await import(pathToFileURL(path.join(DIST, chunk)).href);
const React = m.require_react();
const h = React.createElement;

function inline(c, key) {
    return typeof c === 'string' ? c : h(m.Text, { key, ...c.nested }, ...c.content.map(inline));
}

function toElement(node, key) {
    if (node.box) {
        return h(m.Box_default, { key, ...node.box }, ...node.children.map(toElement));
    }
    if (node.text) {
        return h(m.Text, { key, ...node.text }, ...node.content.map(inline));
    }
    throw new Error(`bad node ${JSON.stringify(node)}`);
}

class FakeStdout extends PassThrough {
    constructor(columns) {
        super();
        this.columns = columns;
        this.rows = 500;
        this.isTTY = true;
        this.frames = [];
        this.on('data', d => this.frames.push(String(d)));
    }
}

async function renderTree(tree, columns) {
    const stdout = new FakeStdout(columns);
    const stdin = new PassThrough();
    stdin.isTTY = false;
    const instance = m.render_default(toElement(tree, 0), { stdout, stdin, debug: true, exitOnCtrlC: false, patchConsole: false });
    await new Promise(r => setTimeout(r, 5));
    instance.unmount();
    // debug mode writes each whole frame; the cursor show/hide writes are not frames.
    return stdout.frames.filter(f => !/^\u001b\[\?25[hl]$/.test(f)).at(-1) ?? '';
}

// --- trees -----------------------------------------------------------------------------------------------------

let seed = 7;
const rand = () => {
    seed = (seed + 0x6d2b79f5) >>> 0;
    let t = seed;
    t = Math.imul(t ^ (t >>> 15), t | 1);
    t ^= t + Math.imul(t ^ (t >>> 7), t | 61);
    return ((t ^ (t >>> 14)) >>> 0) / 4294967296;
};
const pick = arr => arr[Math.floor(rand() * arr.length)];
const WORDS = ['a', 'bb', 'ccc', 'word', 'longer', 'supercalifragilistic', '📝', '⚡', '⎇', '🌐 Global', '中文字', '▶  ', '(x)', '|', '  ', ' ', 'Model: Claude', 'Ctx: 18.6k', '…'];
const COLORS = [undefined, 'green', 'cyan', 'yellow', 'red', 'blue', 'gray', 'yellowBright', '#ff8800', 'ansi256(202)'];

function randomText() {
    const words = Array.from({ length: 1 + Math.floor(rand() * 6) }, () => pick(WORDS));
    const props = {};
    if (rand() < 0.4) props.color = pick(COLORS.slice(1));
    if (rand() < 0.2) props.dimColor = true;
    if (rand() < 0.2) props.bold = true;
    if (rand() < 0.1) props.backgroundColor = pick(['gray', 'blue']);
    if (rand() < 0.15) props.wrap = 'truncate';
    const content = [];
    for (const w of words) {
        if (rand() < 0.25) {
            const nested = {};
            if (rand() < 0.5) nested.dimColor = true;
            if (rand() < 0.5) nested.color = pick(COLORS.slice(1));
            if (rand() < 0.3) nested.bold = true;
            content.push({ nested, content: [w] });
        } else {
            content.push(w);
        }
    }
    return { text: props, content };
}

function randomBox(depth) {
    const props = {};
    if (rand() < 0.5) props.flexDirection = 'column';
    if (rand() < 0.3) props.marginTop = 1;
    if (rand() < 0.15) props.marginBottom = 1;
    if (rand() < 0.15) props.marginLeft = 2;
    if (rand() < 0.15) props.paddingLeft = pick([1, 2]);
    if (rand() < 0.15) props.width = pick([3, 4, 10, '100%']);
    if (rand() < 0.08) props.gap = 1;
    if (rand() < 0.08) {
        props.borderStyle = 'round';
        props.borderColor = 'gray';
        if (rand() < 0.5) props.borderDimColor = true;
    }
    const n = 1 + Math.floor(rand() * 4);
    const children = Array.from({ length: n }, () => (depth < 3 && rand() < 0.35 ? randomBox(depth + 1) : randomText()));
    return { box: props, children };
}

const HAND = [
    // The items editor's widget row: a fixed-width marker box, then texts that shrink when the row overflows.
    { box: { flexDirection: 'row', flexWrap: 'nowrap' }, children: [
        { box: { width: 3 }, children: [{ text: { color: 'green' }, content: ['▶ '] }] },
        { text: { color: 'green' }, content: ['5. Git Branch'] },
        { text: { dimColor: true }, content: [' ', '(repo link, max:12)'] },
        { text: { dimColor: true }, content: [' (hide: no-git)'] },
        { text: { dimColor: true }, content: [' (raw value)'] }
    ] },
    // The flex note: two texts side by side.
    { box: { marginTop: 1 }, children: [
        { text: { color: 'yellow' }, content: ['⚠ Note: Terminal width detection is currently unavailable in your environment.'] },
        { text: { dimColor: true }, content: ['  Flex separators will act as normal separators until width detection is available.'] }
    ] },
    // The preview header.
    { box: { flexDirection: 'column' }, children: [
        { box: { borderStyle: 'round', borderColor: 'gray', borderDimColor: true, width: '100%', paddingLeft: 1 }, children: [
            { text: {}, content: ['>', { nested: { dimColor: true }, content: [' Preview  (ctrl+s to save configuration at any time)'] }] }
        ] }
    ] },
    // A list description with padding.
    { box: { marginTop: 1, paddingLeft: 2 }, children: [{ text: { dimColor: true, wrap: 'wrap' }, content: ['Configure any number of status lines with various widgets like model info, git status, and token usage'] }] },
    // Header row with a flash.
    { box: { marginBottom: 1 }, children: [
        { text: { bold: true }, content: ['CCStatusline Configuration'] },
        { text: { bold: true }, content: [' | v2.2.32'] },
        { text: { color: 'green', bold: true }, content: ['  ✓ Configuration saved'] }
    ] }
];

const cases = [];
const widths = [6, 9, 12, 17, 20, 25, 31, 40, 53, 60, 80, 100];
for (const tree of HAND) {
    for (const columns of widths) {
        cases.push({ columns, tree });
    }
}
for (let i = 0; i < 400; i++) {
    cases.push({ columns: pick(widths), tree: randomBox(0) });
}
for (const c of cases) {
    c.output = await renderTree(c.tree, c.columns);
}
console.log(JSON.stringify(cases, null, 0).replace(/\},\{"columns"/g, '},\n{"columns"'));
process.exit(0);
