#!/usr/bin/env node
// Records what upstream's widget registry says about each widget in the editor, for the Rust editor's tests:
// display name, category, description, editor display, raw-value and number-format support, hideable states,
// label prefix and key bindings, for a tree of items reached by applying each widget's own editor actions
// (handleEditorAction) to a few seed items, with every resulting item kept with its key order.
//
//   npm ci --prefix parity/upstream --ignore-scripts
//   node tools/gen-editor-meta.mjs > src/widgets/testdata/editor-meta.json
import * as path from 'node:path';
import { fileURLToPath, pathToFileURL } from 'node:url';
import * as fs from 'node:fs';

const REPO = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..');
const DIST = path.join(REPO, 'parity', 'upstream', 'node_modules', 'ccstatusline', 'dist');
const chunk = fs.readdirSync(DIST).find(f => fs.readFileSync(path.join(DIST, f), 'utf8').includes('function getWidgetCatalog('));
const m = await import(pathToFileURL(path.join(DIST, chunk)).href);

const MAX_ITEMS_PER_TYPE = 24;
const settings = { powerline: { enabled: false } };

function describe(widget, item) {
    const keybinds = widget.getCustomKeybinds ? widget.getCustomKeybinds(item) : [];
    const actions = {};
    for (const kb of keybinds) {
        if (widget.handleEditorAction) {
            actions[kb.action] = widget.handleEditorAction(kb.action, item);
        }
    }
    return {
        item,
        display: widget.getEditorDisplay(item),
        supportsColors: widget.supportsColors(item),
        labelPrefix: widget.getLabelPrefix ? widget.getLabelPrefix(item) : null,
        keybinds,
        actions,
        hideModifier: m.getHideModifierText(item, widget.getHideableStates?.() ?? []) ?? null,
        labelModifier: m.getLabelModifierText(item) ?? null,
        numberFormatModifier: m.getNumberFormatModifierText(item) ?? null
    };
}

const out = [];
for (const entry of m.getWidgetCatalog(settings)) {
    const widget = m.getWidget(entry.type);
    const record = {
        type: entry.type,
        displayName: entry.displayName,
        category: entry.category,
        description: entry.description,
        known: Boolean(widget)
    };
    if (widget) {
        record.supportsRawValue = widget.supportsRawValue();
        record.supportsNumberFormat = widget.supportsNumberFormat?.() ?? false;
        record.hideableStates = widget.getHideableStates?.() ?? [];
        // Each item is used in its saved (JSON) form, as the Rust side reads it.
        const seeds = [
            { id: '1', type: entry.type },
            { id: '1', type: entry.type, rawValue: true },
            { id: '1', type: entry.type, merge: true, metadata: { label: 'X: ' } }
        ];
        const seen = new Set();
        const queue = [...seeds];
        record.items = [];
        while (queue.length && record.items.length < MAX_ITEMS_PER_TYPE) {
            const item = queue.shift();
            const key = JSON.stringify(item);
            if (seen.has(key)) {
                continue;
            }
            seen.add(key);
            const d = describe(widget, item);
            record.items.push(d);
            // Continue from the saved form, as the Rust side sees it: JSON drops `undefined` properties.
            for (const next of Object.values(d.actions)) {
                if (next) {
                    queue.push(JSON.parse(JSON.stringify(next)));
                }
            }
        }
    }
    out.push(record);
}
// The picker's search: filtered types per (category, query), and match segments per display name.
const catalog = m.getWidgetCatalog(settings);
const categories = ['All', ...m.getWidgetCatalogCategories(catalog)];
const queries = ['', 'g', 'git', 'gb', 'Git B', 'tok', 'ctx', 'con', 'xyz', 'm', 'se', 'usage', 'jj', 'c l', 'br', 'week', 'cost', ' t ', 'sep', 'flex'];
const search = [];
for (const category of categories) {
    for (const query of queries) {
        search.push({ category, query, types: m.filterWidgetCatalog(catalog, category, query).map(e => e.type) });
    }
}
const segments = [];
for (const entry of catalog) {
    for (const query of ['g', 'git', 'gb', 'tok', 'ctx', 'se', 'c l', 'week']) {
        segments.push({ text: entry.displayName, query, segments: m.getMatchSegments(entry.displayName, query) });
    }
}
const withPowerline = m.getWidgetCatalog({ powerline: { enabled: true } }).map(e => e.type);
console.log(JSON.stringify({ catalog: catalog.map(e => e.type), catalogWithPowerline: withPowerline, categories: m.getWidgetCatalogCategories(catalog), search, segments, widgets: out }, null, 1));
