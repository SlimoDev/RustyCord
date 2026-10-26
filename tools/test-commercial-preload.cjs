// Offline behavioral tests. Reads only public shipped sources; no browser, network or user profile.
"use strict";
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const vm = require("node:vm");
const crypto = require("node:crypto");
const root = path.resolve(__dirname, "..");
const source = fs.readFileSync(path.join(root, "src/NotDiscord/CommercialPreload.cs"), "utf8");
function rawConstant(name) {
    const match = source.match(new RegExp('internal const string ' + name + ' = """\\r?\\n([\\s\\S]*?)\\r?\\n([ \\t]*)""";'));
    assert.ok(match, name + " raw string must exist");
    return match[1].split(/\r?\n/).map(line => {
        if (!line.trim()) return "";
        assert.ok(line.startsWith(match[2]), "Valid C# raw string indentation");
        return line.slice(match[2].length);
    }).join("\n");
}
const script = rawConstant("Script"), stateScript = rawConstant("StateScript");
const vendorPath = path.join(root, "vendor/vencord/current/dist/Vencord.js");
const vendor = fs.readFileSync(vendorPath, "utf8");
assert.ok(vendor.includes('"use strict";var Vencord=(()=>{'), "Observed global var assignment remains valid");
assert.ok(vendor.includes("addPatch:()=>RS"), "Real vendor exports addPatch");
const normalizeStart = vendor.indexOf("function jt("), normalizeEnd = vendor.indexOf("var Hr=", normalizeStart);
const addStart = vendor.indexOf("function RS("), addEnd = vendor.indexOf("function kd(", addStart);
assert.ok(normalizeStart >= 0 && normalizeEnd > normalizeStart && addStart >= 0 && addEnd > addStart);
// These functions are copied into each VM directly from the installed official vendor bundle.
// No substituted addPatch or normalization implementation is used.
const realPatchApi = "var cs=[];" + vendor.slice(normalizeStart, normalizeEnd) + vendor.slice(addStart, addEnd) +
    ';globalThis.testVencord={Plugins:{addPatch:RS},Webpack:{wreq:undefined}};';
const originalEffect = "eI.useEffect(()=>{(0,ra.t)(()=>re.preload())},[])";
const replacementEffect = "eI.useEffect(()=>{try{window.__notDiscordCommercialPreloadState.applied=true}catch{}},[])";
const webPath = path.join(root, "evidence/public-web/web.4d4b2f75fdea5470.js");
const web = fs.readFileSync(webPath, "utf8");
const start = web.indexOf("926456(e,t,n){");
const end = web.indexOf("},329563(e,t,n){", start);
assert.ok(start >= 0 && end > start);
const realFactory = "function" + web.slice(start + "926456".length, end + 1);
new vm.Script("(" + realFactory + ")"); // Compile the real factory; never execute it.
const results = [];
function test(name, action) { action(); results.push({ name, passed: true }); }
function fixture(options = {}) {
    const timers = new Map(), listeners = new Map();
    let timerId = 0;
    // Ordinary global semantics matter for non-configurable var/accessor tests.
    const context = vm.createContext(vm.constants.DONT_CONTEXTIFY);
    Object.assign(context, {
        location: { origin: options.origin || "https://discord.com" },
        document: {
            readyState: options.readyState || "loading",
            addEventListener(name, fn) { listeners.set(name, fn); },
            removeEventListener(name, fn) { if (listeners.get(name) === fn) listeners.delete(name); }
        },
        setTimeout(fn, delay) { assert.equal(delay, 15000); timers.set(++timerId, fn); return timerId; },
        clearTimeout(id) { timers.delete(id); }
    });
    vm.runInContext("window=globalThis; window.self=window; window.top=window;", context);
    if (options.frame) vm.runInContext("window.top={};", context);
    vm.runInContext(realPatchApi, context);
    if (options.setup) vm.runInContext(options.setup, context);
    const run = code => vm.runInContext(code, context, { timeout: 1000 });
    run(script);
    return {
        context, run, timers, listeners,
        assign() { run('"use strict";var Vencord=(()=>testVencord)();'); },
        get state() { return run("window.__notDiscordCommercialPreloadState"); },
        get patches() { return run("cs"); },
        expire() { for (const callback of [...timers.values()]) callback(); },
        ready() { listeners.get("DOMContentLoaded")?.(); },
        transform(factory = realFactory) {
            assert.equal(this.patches.length, 1);
            const patch = this.patches[0];
            assert.equal(patch.plugin, "NotDiscordLean");
            assert.equal(patch.find, "nitroTabButtonRef:");
            assert.equal(patch.replacement.length, 1);
            return factory.replace(patch.replacement[0].match, patch.replacement[0].replace);
        }
    };
}
for (const origin of ["http://discord.com", "https://canary.discord.com", "https://discord.com.evil.test"]) {
    test("Reject origin " + origin, () => {
        const f = fixture({ origin });
        assert.equal(f.state, undefined); assert.equal(f.timers.size, 0);
        assert.equal(f.run('Object.hasOwn(window,"Vencord")'), false);
    });
}
test("Reject subframe", () => assert.equal(fixture({ frame: true }).state, undefined));
test("Real vendor var assignment invokes hook and restores normal data property before addPatch", () => {
    const f = fixture();
    assert.equal(f.run('typeof Object.getOwnPropertyDescriptor(window,"Vencord").set'), "function");
    f.run('const realAddPatch=testVencord.Plugins.addPatch; testVencord.Plugins.addPatch=(...args)=>{ if(Object.getOwnPropertyDescriptor(window,"Vencord").set) throw Error("Accessor retained"); return realAddPatch(...args); };');
    f.assign();
    assert.equal(f.state.registered, true); assert.equal(f.state.error, false);
    assert.equal(f.run('Object.getOwnPropertyDescriptor(window,"Vencord").value === testVencord'), true);
    assert.equal(f.timers.size, 0); assert.equal(f.listeners.size, 0);
});
test("Exact one replacement compiles full public factory and changes only automatic effect", () => {
    const f = fixture(); f.assign();
    const transformed = f.transform();
    assert.equal(transformed, realFactory.replace(originalEffect, replacementEffect));
    new vm.Script("(" + transformed + ")");
    assert.equal(f.state.matched, true); assert.equal(f.state.applied, false);
    assert.equal(f.state.error, false);
    assert.ok(transformed.includes("function eB(){re.preload(),"));
    assert.ok(transformed.includes("webpackId:338249"));
});
test("Modified effect prevents automatic load and acknowledges actual execution", () => {
    const f = fixture(); f.assign(); f.transform();
    f.run('var loadCount=0, effect=null;var eI={useEffect(fn){effect=fn}},ra={t(fn){fn()}},re={preload(){loadCount++}};');
    f.run(replacementEffect); f.run("effect()");
    assert.equal(f.run("loadCount"), 0); assert.equal(f.state.applied, true);
    f.run("re.preload()"); assert.equal(f.run("loadCount"), 1); // Route/user loads remain possible.
});
test("Original observed effect actually preloads in the same isolated fixture", () => {
    const f = fixture(); f.assign();
    f.run('var loadCount=0;var eI={useEffect(fn){fn()}},ra={t(fn){fn()}},re={preload(){loadCount++}};');
    f.run(originalEffect); assert.equal(f.run("loadCount"), 1); assert.equal(f.state.applied, false);
});
for (const [name, change] of [
    ["missing exact effect", s => s.replace(originalEffect, "eI.useEffect(()=>{},[])")],
    ["duplicate effect", s => s + originalEffect],
    ["missing Nitro context", s => s.replaceAll('location:"NitroTabButton"', 'location:"OtherButton"')],
    ["different lazy module", s => s.replaceAll("webpackId:338249", "webpackId:123456")]
]) {
    test("Fail unchanged: " + name, () => {
        const f = fixture(); f.assign(); const changed = change(realFactory);
        assert.equal(f.transform(changed), changed);
        assert.equal(f.state.error, true); assert.equal(f.state.matched, false); assert.equal(f.state.applied, false);
    });
}
test("Existing ready Vencord registers without installing a hook", () => {
    const f = fixture({ setup: 'Object.defineProperty(window,"Vencord",{value:testVencord,writable:true,configurable:false,enumerable:true});' });
    assert.equal(f.state.registered, true); assert.equal(f.timers.size, 0);
    assert.equal(f.run('Object.getOwnPropertyDescriptor(window,"Vencord").configurable'), false);
});
test("Already executed main factory fails safely without patch registration", () => {
    const f = fixture({ setup: 'testVencord.Webpack.wreq={c:{926456:{exports:{}}}};' }); f.assign();
    assert.equal(f.state.late, true); assert.equal(f.state.registered, false); assert.equal(f.patches.length, 0);
});
test("Do not invoke or overwrite someone else's global accessor", () => {
    const f = fixture({ setup: 'var reads=0;Object.defineProperty(window,"Vencord",{get(){reads++;throw Error("not allowed")},configurable:true});' });
    assert.equal(f.run("reads"), 0); assert.equal(f.state.error, true); assert.equal(f.timers.size, 0);
});
for (const descriptor of ['{value:undefined,writable:true,configurable:false}', '{value:undefined,writable:false,configurable:true}']) {
    test("Preserve incompatible own descriptor " + descriptor, () => {
        const f = fixture({ setup: 'Object.defineProperty(window,"Vencord",' + descriptor + ');' });
        assert.equal(f.state.error, true); assert.equal(f.timers.size, 0);
        assert.equal(f.run('Object.getOwnPropertyDescriptor(window,"Vencord").value'), undefined);
    });
}
test("API failure does not abort global assignment", () => {
    const f = fixture({ setup: 'testVencord.Plugins.addPatch=()=>{throw Error("fixture failure")};' }); f.assign();
    assert.equal(f.state.error, true); assert.equal(f.state.registered, false);
    assert.equal(f.run("Vencord===testVencord"), true); assert.equal(f.timers.size, 0);
});
test("Locked accessor cannot abort Vencord assignment and preserves later writes", () => {
    const f = fixture();
    f.run('Object.defineProperty(window,"Vencord",{configurable:false});');
    f.assign();
    assert.equal(f.run("Vencord===testVencord"), true);
    assert.equal(f.state.error, true); assert.equal(f.state.registered, false);
    assert.equal(f.timers.size, 0); assert.equal(f.listeners.size, 0);
    f.run("Vencord=123"); assert.equal(f.run("Vencord"), 123);
});
test("Unavailable API leaves global data intact", () => {
    const f = fixture({ setup: 'testVencord.Plugins={};' }); f.assign();
    assert.equal(f.state.error, true); assert.equal(f.run("Vencord===testVencord"), true);
});
test("Timeout removes unassigned own hook", () => {
    const f = fixture(); f.expire(); assert.equal(f.state.expired, true);
    assert.equal(f.run('Object.hasOwn(window,"Vencord")'), false);
    assert.equal(f.timers.size, 0); assert.equal(f.listeners.size, 0);
});
test("Locked unassigned accessor expires safely and preserves a later assignment", () => {
    const f = fixture();
    f.run('Object.defineProperty(window,"Vencord",{configurable:false});'); f.expire();
    assert.equal(f.state.expired, true); assert.equal(f.state.error, true);
    assert.equal(f.timers.size, 0); assert.equal(f.listeners.size, 0);
    f.assign(); assert.equal(f.run("Vencord===testVencord"), true);
    assert.equal(f.state.registered, false);
});
test("Timeout restores an existing undefined descriptor exactly", () => {
    const f = fixture({ setup: 'Object.defineProperty(window,"Vencord",{value:undefined,writable:true,enumerable:false,configurable:true});' });
    f.expire(); assert.equal(f.run('JSON.stringify(Object.getOwnPropertyDescriptor(window,"Vencord"))'), '{"writable":true,"enumerable":false,"configurable":true}');
});
test("DOMContentLoaded expires hook without a recurring timer", () => {
    const f = fixture(); f.ready(); assert.equal(f.state.expired, true); assert.equal(f.timers.size, 0);
});
test("Already complete document expires immediately", () => {
    const f = fixture({ readyState: "complete" }); assert.equal(f.state.expired, true); assert.equal(f.timers.size, 0);
});
test("Cleanup does not overwrite another owner's replacement property", () => {
    const f = fixture(); f.run('Object.defineProperty(window,"Vencord",{value:123,configurable:true});'); f.expire();
    assert.equal(f.run("Vencord"), 123);
});
test("Duplicate injection registers no second hook or patch", () => {
    const f = fixture(); f.run(script); assert.equal(f.timers.size, 1); f.assign(); f.run(script);
    assert.equal(f.patches.length, 1); assert.equal(f.timers.size, 0);
});
test("State probe returns only fixed boolean values", () => {
    const f = fixture(); f.assign(); const state = f.run(stateScript);
    assert.deepEqual(Object.keys(state), ["supported","installed","registered","matched","applied","error","late","expired"]);
    assert.ok(Object.values(state).every(v => typeof v === "boolean"));
});
console.log(JSON.stringify({
    success: true, checks: results.length,
    vendorSha256: crypto.createHash("sha256").update(vendor).digest("hex"),
    publicWebSha256: crypto.createHash("sha256").update(web).digest("hex"),
    sourceSha256: crypto.createHash("sha256").update(source).digest("hex"),
    observedEffectUtf16Offset: web.indexOf(originalEffect),
    scope: "Isolated VM; actual vendor addPatch/normalizers and public main factory compiled inert. No full app startup, account, network or memory measurement.",
    results
}, null, 2));
