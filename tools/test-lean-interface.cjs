"use strict";
// Local DOM/settings simulation only; does not inspect any browser profile or network.
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const assert = require("node:assert/strict");

const projectRoot = path.resolve(__dirname, "..");
const sourcePath = path.join(projectRoot, "src", "NotDiscord", "LeanInterface.cs");
const source = fs.readFileSync(sourcePath, "utf8");
function extract(name) {
    const expression = new RegExp('internal const string ' + name + ' = """\\r?\\n([\\s\\S]*?)\\r?\\n([ \\t]*)""";');
    const match = source.match(expression);
    assert.ok(match, name + " raw C# string exists");
    return match[1].split(/\r?\n/).map(line => {
        if (!line.trim()) return "";
        assert.ok(line.startsWith(match[2]), "Valid raw string indentation");
        return line.slice(match[2].length);
    }).join("\n");
}
const script = extract("Script");
const stateScript = extract("StateScript");
const results = [];

function fixture(options = {}) {
    const timers = [];
    const events = new Map();
    const createdStyles = [];
    const writes = [];
    let queryCount = 0;
    const links = (options.links || []).map(item => ({
        ...item, removed: false,
        getAttribute(name) { return this[name] ?? null; },
        remove() { this.removed = true; }
    }));
    const specs = [];
    function specNode(text) {
        return {
            textContent: text,
            cloneNode() { return specNode(""); },
            replaceWith(next) {
                const index = specs.indexOf(this);
                assert.ok(index >= 0);
                specs[index] = next;
            }
        };
    }
    for (const spec of options.specs || []) {
        specs.push(specNode(typeof spec === "string" ? spec : JSON.stringify(spec)));
    }
    const plain = options.plain || {
        NoTrack: { enabled: true, disableAnalytics: false },
        NoPendingCount: { enabled: false, hidePremiumOffersCount: false,
            hideFriendRequestsCount: true, hideMessageRequestsCount: true },
        MessageLogger: { enabled: true, sentinel: "preserve" },
        VolumeBooster: { enabled: true, multiplier: 3 },
        FakeNitro: { enabled: true }
    };
    const settingsPlugins = {};
    for (const [plugin, value] of Object.entries(plain)) {
        settingsPlugins[plugin] = new Proxy(value, {
            set(target, key, next) {
                writes.push({ plugin, key, value: next });
                target[key] = next;
                return true;
            }
        });
    }
    const available = options.available || {
        NoTrack: { settings: { def: { disableAnalytics: {} } } },
        NoPendingCount: { settings: { def: {
            hidePremiumOffersCount: {}, hideFriendRequestsCount: {}, hideMessageRequestsCount: {}
        } } }
    };
    const vencord = options.noVencord ? undefined : {
        Settings: { plugins: settingsPlugins },
        PlainSettings: { plugins: plain },
        Plugins: { plugins: available }
    };
    const frame = { Vencord: vencord };
    frame.self = frame;
    frame.top = options.iframe ? {} : frame;
    const locationUrl = new URL(options.url || "https://discord.com/channels/@me");
    const document = {
        readyState: options.loading ? "loading" : "complete",
        head: { appendChild(style) { createdStyles.push(style); } },
        getElementById(id) { return createdStyles.find(style => style.id === id) || null; },
        createElement(tag) { assert.equal(tag, "style"); return { tag, id: "", textContent: "" }; },
        addEventListener(name, callback, eventOptions) {
            assert.equal(name, "DOMContentLoaded");
            assert.equal(eventOptions.once, true);
            events.set(name, callback);
        },
        querySelectorAll(selector) {
            queryCount++;
            if (selector === 'link[rel="prefetch"][href],link[rel="prerender"][href]') {
                return links.filter(link => !link.removed && ["prefetch", "prerender"].includes(link.rel));
            }
            if (selector === 'script[type="speculationrules"]') return specs.slice();
            throw new Error("Unbounded or unexpected DOM selector: " + selector);
        }
    };
    const context = {
        window: frame, document, location: {
            origin: locationUrl.origin, href: locationUrl.href
        }, URL,
        setTimeout(callback, delay) { timers.push({ callback, delay }); return timers.length; },
        MutationObserver: class {
            constructor() { throw new Error("A continuous observer must not be installed"); }
        }
    };
    function execute(code) {
        return vm.runInNewContext(code, context, { timeout: 1000, filename: "LeanInterface" });
    }
    const initialResult = execute(script);
    return {
        initialResult, frame, context, plain, links, specs, writes, createdStyles, timers,
        queryCount: () => queryCount,
        execute,
        domReady() {
            document.readyState = "interactive";
            const callback = events.get("DOMContentLoaded");
            events.delete("DOMContentLoaded");
            if (callback) callback();
        },
        flushTimers() {
            let executions = 0;
            while (timers.length) {
                if (++executions > 100) throw new Error("Unbounded timer loop");
                timers.shift().callback();
            }
            return executions;
        }
    };
}
function run(name, action) { action(); results.push({ name, passed: true }); }

for (const [name, options] of [
    ["other origin untouched", { url: "https://example.com/channels/@me" }],
    ["subframes untouched", { iframe: true }],
    ["lookalike origin untouched", { url: "https://discord.com.example.com/channels/@me" }]
]) {
    run(name, () => {
        const test = fixture(options);
        assert.equal(test.initialResult.supported, false);
        assert.equal(test.createdStyles.length, 0);
        assert.equal(test.writes.length, 0);
        assert.equal(test.timers.length, 0);
        assert.equal(test.queryCount(), 0);
    });
}

run("sets known tracking and premium-only badge settings; preserves user plugins", () => {
    const test = fixture();
    assert.equal(test.plain.NoTrack.disableAnalytics, true);
    assert.equal(test.plain.NoTrack.enabled, true);
    assert.equal(test.plain.NoPendingCount.enabled, true);
    assert.equal(test.plain.NoPendingCount.hidePremiumOffersCount, true);
    assert.equal(test.plain.NoPendingCount.hideFriendRequestsCount, false);
    assert.equal(test.plain.NoPendingCount.hideMessageRequestsCount, false);
    assert.deepEqual(test.plain.MessageLogger, { enabled: true, sentinel: "preserve" });
    assert.deepEqual(test.plain.VolumeBooster, { enabled: true, multiplier: 3 });
    assert.deepEqual(test.plain.FakeNitro, { enabled: true });
    assert.ok(test.writes.every(write => ["NoTrack", "NoPendingCount"].includes(write.plugin)));
    assert.equal(test.frame.__notDiscordLeanInterfaceState.restartNeeded, true);
});

run("CSS uses only four verified exact routes scoped to navigation", () => {
    const test = fixture();
    const css = test.createdStyles[0].textContent;
    for (const route of ["/shop", "/store", "/quest-home", "/discovery/quests"]) {
        assert.ok(css.includes('nav a[href="' + route + '"]'));
        assert.ok(css.includes('[role="navigation"] a[href="https://discord.com' + route + '"]'));
    }
    for (const unwanted of ["/channels", "/settings", '[href^=', '[href*=']) {
        assert.equal(css.includes(unwanted), false);
    }
});

run("defers DOM work until DOMContentLoaded", () => {
    const test = fixture({ loading: true });
    assert.equal(test.createdStyles.length, 0);
    assert.equal(test.queryCount(), 0);
    test.domReady();
    assert.equal(test.createdStyles.length, 1);
    assert.equal(test.queryCount(), 2);
    test.flushTimers();
    assert.equal(test.queryCount(), 4);
});

run("prunes only exact same-origin commercial prefetch and prerender destinations", () => {
    const test = fixture({ links: [
        { rel: "prefetch", href: "/shop" },
        { rel: "prerender", href: "https://discord.com/store" },
        { rel: "prefetch", href: "/quest-home" },
        { rel: "prefetch", href: "/discovery/quests" },
        { rel: "prefetch", href: "/channels/@me" },
        { rel: "prefetch", href: "/quests/123" },
        { rel: "prefetch", href: "/shop?tab=game-shops" },
        { rel: "prefetch", href: "https://example.com/shop" },
        { rel: "preload", href: "/shop" },
        { rel: "modulepreload", href: "/assets/client.js" },
        { rel: "prefetch", href: "/assets/shop-chunk.js" }
    ] });
    assert.deepEqual(test.links.map(link => link.removed),
        [true, true, true, true, false, false, false, false, false, false, false]);
    assert.equal(test.frame.__notDiscordLeanInterfaceState.removedPrefetchLinks, 4);
    test.flushTimers();
    assert.equal(test.frame.__notDiscordLeanInterfaceState.removedPrefetchLinks, 4);
});

run("filters explicit speculation lists while preserving all other rules and destinations", () => {
    const documentRule = { source: "document", where: { href_matches: "/channels/*" } };
    const test = fixture({ specs: [{
        prefetch: [
            { source: "list", urls: ["/shop", "/channels/@me", "https://example.com/store"] },
            documentRule
        ],
        prerender: [{ source: "list", urls: ["/store"] }],
        extension: { preserve: true }
    }, "{invalid"] });
    const result = JSON.parse(test.specs[0].textContent);
    assert.deepEqual(result.prefetch[0].urls, ["/channels/@me", "https://example.com/store"]);
    assert.deepEqual(result.prefetch[1], documentRule);
    assert.deepEqual(result.prerender, []);
    assert.deepEqual(result.extension, { preserve: true });
    assert.equal(test.specs[1].textContent, "{invalid");
    assert.equal(test.frame.__notDiscordLeanInterfaceState.removedSpeculationUrls, 2);
});

run("unavailable or changed plugin definitions are not guessed", () => {
    const test = fixture({ available: { NoTrack: { settings: { def: {} } }, NoPendingCount: {} } });
    assert.equal(test.writes.length, 0);
    assert.equal(test.frame.__notDiscordLeanInterfaceState.unsupportedSettings.length, 2);
});

run("Vencord readiness polling is bounded and uses no continuous DOM observer", () => {
    const test = fixture({ noVencord: true });
    test.flushTimers();
    assert.equal(test.frame.__notDiscordLeanInterfaceState.settingsAttempts, 30);
    assert.equal(test.frame.__notDiscordLeanInterfaceState.settingsReady, false);
    assert.equal(test.queryCount(), 4);
});

run("repeat execution is idempotent", () => {
    const test = fixture();
    const writes = test.writes.length;
    const timers = test.timers.length;
    test.execute(script);
    assert.equal(test.writes.length, writes);
    assert.equal(test.createdStyles.length, 1);
    assert.equal(test.timers.length, timers);
});

run("StateScript reads only plain settings and makes no mutations", () => {
    const test = fixture();
    const before = JSON.stringify({
        plain: test.plain, state: test.frame.__notDiscordLeanInterfaceState,
        styles: test.createdStyles, writes: test.writes, timers: test.timers.length
    });
    Object.defineProperty(test.frame.Vencord, "Settings", {
        get() { throw new Error("Read-only StateScript must not access the settings proxy"); }
    });
    const result = test.execute(stateScript);
    assert.equal(result.noTrack.disableAnalytics, true);
    assert.equal(result.premiumOfferBadges.hideMessageRequestsCount, false);
    assert.equal(result.ramSavingsMeasuredByThisScript, false);
    const after = JSON.stringify({
        plain: test.plain, state: test.frame.__notDiscordLeanInterfaceState,
        styles: test.createdStyles, writes: test.writes, timers: test.timers.length
    });
    assert.equal(after, before);
});

const directory = path.join(__dirname, "test-output", "lean-interface");
fs.mkdirSync(directory, { recursive: true });
const outputPath = path.join(directory, "validation.json");
fs.writeFileSync(outputPath, JSON.stringify({
    schemaVersion: 1, passed: true, testedUtc: new Date().toISOString(),
    sourcePath, nodeVersion: process.version,
    scope: "Node VM mock DOM/settings; no live Discord, voice, screenshare or measured RAM claim",
    tests: results
}, null, 2) + "\n");
process.stdout.write(JSON.stringify({
    passed: true, testCount: results.length, validationPath: outputPath
}, null, 2) + "\n");

