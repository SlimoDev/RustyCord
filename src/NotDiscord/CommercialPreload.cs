namespace NotDiscord;

/// <summary>
/// Suppresses only the observed automatic Nitro preload through Vencord's patch API.
/// Register before navigating. No requests, account state, DOM content or module exports are collected.
/// A changed factory is left intact. The normal lazy route and hover/click loads remain available.
/// </summary>
internal static class CommercialPreload
{
    internal const string Script = """
    (() => {
        "use strict";
        if (location.origin !== "https://discord.com" || window.top !== window.self) return;
        const stateName = "__notDiscordCommercialPreloadState";
        if (Object.getOwnPropertyDescriptor(window, stateName)?.value?.installed === true) return;
        const state = {
            installed: true, registered: false, matched: false, applied: false,
            error: false, late: false, expired: false
        };
        try {
            Object.defineProperty(window, stateName, { value: state, configurable: true });
        } catch { return; }

        // Public web.4d4b2f75fdea5470.js, factory 926456, UTF-16 offset 1632882.
        // Keep the effect slot, but acknowledge execution instead of scheduling Nitro's preload.
        const originalEffect = "eI.useEffect(()=>{(0,ra.t)(()=>re.preload())},[])";
        const replacementEffect = "eI.useEffect(()=>{try{window.__notDiscordCommercialPreloadState.applied=true}catch{}},[])";
        function register(vencord) {
            try {
                if (typeof vencord?.Plugins?.addPatch !== "function") {
                    state.error = true;
                    return;
                }
                // If this observed factory has run already, its closures cannot be safely rewritten.
                if (vencord.Webpack?.wreq?.c &&
                    Object.prototype.hasOwnProperty.call(vencord.Webpack.wreq.c, "926456")) {
                    state.late = true;
                    return;
                }
                vencord.Plugins.addPatch({
                    find: "nitroTabButtonRef:",
                    replacement: {
                        match: /^[\s\S]*$/,
                        replace(source) {
                            const first = source.indexOf(originalEffect);
                            if (!source.includes('location:"NitroTabButton"') ||
                                !source.includes("webpackId:338249") || first < 0 ||
                                source.indexOf(originalEffect, first + originalEffect.length) >= 0) {
                                state.error = true;
                                return source;
                            }
                            state.matched = true;
                            // applied becomes true only if the patched effect actually executes.
                            return source.slice(0, first) + replacementEffect +
                                source.slice(first + originalEffect.length);
                        }
                    }
                }, "NotDiscordLean");
                state.registered = true;
            } catch { state.error = true; }
        }

        // Vencord's shipped MAIN-world script assigns `var Vencord = (() => {...})()`.
        // Observe that single own-property assignment; never modify a prototype or poll indefinitely.
        const previous = Object.getOwnPropertyDescriptor(window, "Vencord");
        if (previous && !("value" in previous)) { state.error = true; return; }
        if (previous?.value != null) { register(previous.value); return; }
        if (previous && (!previous.configurable || !previous.writable)) {
            state.error = true;
            return;
        }
        let timer = null;
        let finished = false;
        let assignedValue = previous?.value;
        function cleanup() {
            if (timer !== null) { clearTimeout(timer); timer = null; }
            document.removeEventListener("DOMContentLoaded", expire);
        }
        function restore(assigned, value) {
            const current = Object.getOwnPropertyDescriptor(window, "Vencord");
            if (current?.set !== assignedVencord) return false;
            if (assigned) {
                Object.defineProperty(window, "Vencord", previous ?
                    { ...previous, value } :
                    { value, configurable: true, enumerable: true, writable: true });
            } else if (previous) {
                Object.defineProperty(window, "Vencord", previous);
            } else {
                return Reflect.deleteProperty(window, "Vencord");
            }
            return true;
        }
        function assignedVencord(value) {
            // If another script locks this accessor before assignment, its getter must still
            // preserve Vencord's value and future writes even when restoration is impossible.
            assignedValue = value;
            if (finished) return;
            finished = true;
            try {
                cleanup();
                // Restore normal global access before addPatch can call any Vencord code.
                if (!restore(true, value)) { state.error = true; return; }
                register(value);
            } catch { state.error = true; }
        }
        function expire() {
            if (finished) return;
            finished = true;
            cleanup();
            state.expired = true;
            try { if (!restore(false)) state.error = true; } catch { state.error = true; }
        }
        try {
            Object.defineProperty(window, "Vencord", {
                configurable: true,
                enumerable: previous?.enumerable ?? true,
                get: () => assignedValue,
                set: assignedVencord
            });
            timer = setTimeout(expire, 15000);
            document.addEventListener("DOMContentLoaded", expire, { once: true });
            if (document.readyState !== "loading") expire();
        } catch {
            state.error = true;
            expire();
        }
    })();
    """;

    internal const string StateScript = """
    (() => {
        "use strict";
        if (location.origin !== "https://discord.com" || window.top !== window.self)
            return { supported: false };
        const state = Object.getOwnPropertyDescriptor(window, "__notDiscordCommercialPreloadState")?.value;
        return {
            supported: true,
            installed: state?.installed === true,
            registered: state?.registered === true,
            matched: state?.matched === true,
            applied: state?.applied === true,
            error: state?.error === true,
            late: state?.late === true,
            expired: state?.expired === true
        };
    })();
    """;
}
