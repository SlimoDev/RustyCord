// Isolated behavior test: no browser, network, account, or real YouTube is accessed.
"use strict";

const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");

const projectRoot = path.resolve(__dirname, "..");
const sourcePath = path.join(projectRoot, "src", "NotDiscord", "YoutubeEmbedCompatibility.cs");
const source = fs.readFileSync(sourcePath, "utf8");
const match = source.match(/internal const string Script = """\r?\n([\s\S]*?)\r?\n([ \t]*)""";/);
assert.ok(match, "C# raw Script constant must be present");
const indentation = match[2];
const script = match[1].split(/\r?\n/).map(line => {
    if (line.trim().length === 0) return "";
    assert.ok(line.startsWith(indentation), "C# raw string indentation must be valid");
    return line.slice(indentation.length);
}).join("\n");

const expectedSelector =
    'div.ytp-error-content-wrap-subreason a[href*="www.youtube.com/watch?v="]';
const ownPrefix = "notdiscord.youtubeEmbedCompatibility.retry.v1:";
const testResults = [];

function fixture(options = {}) {
    const url = new URL(options.url || "https://www.youtube.com/embed/test-video?autoplay=1");
    const storage = options.storage || new Map();
    const state = {
        observers: [], timers: [], listeners: new Map(), storageAccess: [],
        querySelectors: [], reloadCount: 0, errorPresent: Boolean(options.errorPresent)
    };
    const frameWindow = {};
    frameWindow.self = frameWindow;
    frameWindow.top = options.topLevel ? frameWindow : {};
    const location = {
        protocol: url.protocol, hostname: url.hostname, pathname: url.pathname,
        href: url.href, reload() { state.reloadCount++; }
    };
    const document = {
        readyState: options.readyState || "complete",
        body: {},
        querySelector(selector) {
            state.querySelectors.push(selector);
            assert.equal(selector, expectedSelector);
            return state.errorPresent ? {} : null;
        },
        addEventListener(name, callback, eventOptions) {
            assert.equal(name, "DOMContentLoaded");
            assert.equal(eventOptions.once, true);
            state.listeners.set(name, callback);
        }
    };
    class MutationObserver {
        constructor(callback) {
            this.callback = callback;
            this.disconnected = false;
            state.observers.push(this);
        }
        observe(body, observerOptions) {
            assert.equal(body, document.body);
            assert.equal(observerOptions.childList, true);
            assert.equal(observerOptions.subtree, true);
        }
        disconnect() { this.disconnected = true; }
    }
    const sessionStorage = {
        getItem(key) {
            assert.ok(key.startsWith(ownPrefix), "Only the shim's own key can be read");
            state.storageAccess.push(["get", key]);
            if (options.storageReadFails) throw new Error("Storage unavailable");
            return storage.has(key) ? storage.get(key) : null;
        },
        setItem(key, value) {
            assert.ok(key.startsWith(ownPrefix), "Only the shim's own key can be written");
            state.storageAccess.push(["set", key, value]);
            if (options.storageWriteFails) throw new Error("Storage unavailable");
            storage.set(key, value);
        }
    };
    const context = {
        window: frameWindow, location, document, MutationObserver, sessionStorage,
        setTimeout(callback, delay) {
            state.timers.push({ callback, delay });
            return state.timers.length;
        }
    };
    vm.runInNewContext(script, context, { timeout: 1000, filename: "YoutubeEmbedCompatibility.Script" });
    return {
        state, storage, location, document,
        domReady() {
            document.readyState = "interactive";
            const callback = state.listeners.get("DOMContentLoaded");
            state.listeners.delete("DOMContentLoaded");
            if (callback) callback();
        },
        mutate() {
            for (const observer of state.observers) {
                if (!observer.disconnected) observer.callback([]);
            }
        },
        flushTimers() {
            const pending = state.timers.splice(0);
            for (const timer of pending) {
                assert.equal(timer.delay, 250);
                timer.callback();
            }
        }
    };
}

function run(name, check) {
    check();
    testResults.push({ name, passed: true });
}

for (const [name, options] of [
    ["other domain ignored", { url: "https://example.com/embed/test", errorPresent: true }],
    ["lookalike suffix ignored", { url: "https://www.youtube.com.example.com/embed/test", errorPresent: true }],
    ["nocookie domain is not implicitly enabled", { url: "https://www.youtube-nocookie.com/embed/test", errorPresent: true }],
    ["top-level YouTube ignored", { topLevel: true, errorPresent: true }],
    ["HTTP ignored", { url: "http://www.youtube.com/embed/test", errorPresent: true }],
    ["non-embed path ignored", { url: "https://www.youtube.com/watch?v=test", errorPresent: true }]
]) {
    run(name, () => {
        const test = fixture(options);
        assert.equal(test.state.observers.length, 0);
        assert.equal(test.state.timers.length, 0);
        assert.equal(test.state.storageAccess.length, 0);
        assert.equal(test.state.querySelectors.length, 0);
        assert.equal(test.state.reloadCount, 0);
    });
}

run("waits for DOMContentLoaded and reacts to a later error", () => {
    const test = fixture({ readyState: "loading" });
    assert.equal(test.state.observers.length, 0);
    test.domReady();
    assert.equal(test.state.observers.length, 1);
    assert.equal(test.state.reloadCount, 0);
    test.state.errorPresent = true;
    test.mutate();
    assert.equal(test.state.observers[0].disconnected, true);
    assert.equal(test.state.timers.length, 1);
    test.flushTimers();
    assert.equal(test.state.reloadCount, 1);
    test.mutate();
    test.flushTimers();
    assert.equal(test.state.reloadCount, 1);
});

run("existing error reloads once and repeat navigation cannot loop", () => {
    const storage = new Map([["unrelated-private-key", "untouched"]]);
    const first = fixture({ storage, errorPresent: true });
    assert.equal(first.state.timers.length, 1);
    assert.equal(first.state.observers[0].disconnected, true);
    first.flushTimers();
    assert.equal(first.state.reloadCount, 1);

    const repeated = fixture({ storage, errorPresent: true });
    repeated.mutate();
    repeated.flushTimers();
    assert.equal(repeated.state.reloadCount, 0);
    assert.equal(repeated.state.observers[0].disconnected, true);
    assert.equal(storage.get("unrelated-private-key"), "untouched");
    assert.ok([...first.state.storageAccess, ...repeated.state.storageAccess]
        .every(access => access[1] === ownPrefix + first.location.href));
});

run("retry limit is per exact URL", () => {
    const storage = new Map();
    const first = fixture({ storage, errorPresent: true });
    first.flushTimers();
    const second = fixture({ storage, url: "https://www.youtube.com/embed/second-video", errorPresent: true });
    second.flushTimers();
    assert.equal(first.state.reloadCount, 1);
    assert.equal(second.state.reloadCount, 1);
    assert.equal(storage.size, 2);
});

run("no error does not read storage or reload", () => {
    const test = fixture();
    for (let index = 0; index < 5; index++) test.mutate();
    test.flushTimers();
    assert.equal(test.state.reloadCount, 0);
    assert.equal(test.state.storageAccess.length, 0);
    assert.equal(test.state.observers[0].disconnected, false);
});

for (const [name, options] of [
    ["storage read failure prevents reload", { storageReadFails: true }],
    ["storage write failure prevents reload", { storageWriteFails: true }]
]) {
    run(name, () => {
        const test = fixture({ ...options, errorPresent: true });
        test.mutate();
        test.flushTimers();
        assert.equal(test.state.reloadCount, 0);
        assert.equal(test.state.observers[0].disconnected, true);
        assert.equal(test.state.timers.length, 0);
    });
}

run("navigation during delay does not reload the next URL", () => {
    const test = fixture({ errorPresent: true });
    test.location.href = "https://www.youtube.com/embed/different-video";
    test.flushTimers();
    assert.equal(test.state.reloadCount, 0);
});

const outputDirectory = path.join(__dirname, "test-output", "youtube-compatibility");
fs.mkdirSync(outputDirectory, { recursive: true });
const outputPath = path.join(outputDirectory, "validation.json");
const report = {
    schemaVersion: 1, passed: true, testedUtc: new Date().toISOString(),
    sourcePath, nodeVersion: process.version,
    kind: "Isolated Node VM simulation; no actual WebView2 or YouTube playback tested",
    scriptBytes: Buffer.byteLength(script, "utf8"),
    tests: testResults
};
fs.writeFileSync(outputPath, JSON.stringify(report, null, 2) + "\n", "utf8");
process.stdout.write(JSON.stringify({
    passed: true, testCount: testResults.length, validationPath: outputPath
}, null, 2) + "\n");

