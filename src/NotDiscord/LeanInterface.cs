namespace NotDiscord;

/// <summary>
/// Optional interface cleanup using locally verified routes and Vencord settings.
/// CSS is visual cleanup; it does not establish any RAM saving or unload React modules.
/// The host decides whether and when to reload after RestartNeeded is reported.
/// </summary>
internal static class LeanInterface
{
    internal const string Script = """
    (() => {
        "use strict";
        if (location.origin !== "https://discord.com" || window.top !== window.self) {
            return { supported: false };
        }
        if (window.__notDiscordLeanInterfaceState?.version === 1) {
            return window.__notDiscordLeanInterfaceState;
        }

        // Verified route constants in public fast-connect.7c1098d4887bc4b0.js.
        const paths = ["/shop", "/store", "/quest-home", "/discovery/quests"];
        const styleId = "notdiscord-lean-interface-style";
        const state = {
            version: 1,
            supported: true,
            styleInstalled: false,
            settingsReady: false,
            restartNeeded: false,
            settingChanges: [],
            unsupportedSettings: [],
            settingsAttempts: 0,
            removedPrefetchLinks: 0,
            removedSpeculationUrls: 0,
            commercialPaths: paths.slice()
        };
        window.__notDiscordLeanInterfaceState = state;

        function exactCommercialUrl(value) {
            if (typeof value !== "string") return false;
            try {
                const url = new URL(value, location.href);
                return url.origin === "https://discord.com" &&
                    paths.includes(url.pathname) && url.search === "" && url.hash === "";
            } catch {
                return false;
            }
        }

        function installStyle() {
            if (!document.head || document.getElementById(styleId)) return;
            const selectors = [];
            for (const route of paths) {
                for (const href of [route, "https://discord.com" + route]) {
                    selectors.push('nav a[href="' + href + '"]');
                    selectors.push('[role="navigation"] a[href="' + href + '"]');
                }
            }
            const style = document.createElement("style");
            style.id = styleId;
            style.textContent = selectors.join(",\n") + " { display: none !important; }";
            document.head.appendChild(style);
            state.styleInstalled = true;
        }

        function pruneCommercialHints() {
            // Exact document destinations only; hashed JS/CSS, media and APIs are untouched.
            for (const link of document.querySelectorAll(
                'link[rel="prefetch"][href],link[rel="prerender"][href]'
            )) {
                if (exactCommercialUrl(link.getAttribute("href"))) {
                    link.remove();
                    state.removedPrefetchLinks++;
                }
            }
            for (const element of document.querySelectorAll('script[type="speculationrules"]')) {
                let rules;
                try { rules = JSON.parse(element.textContent); } catch { continue; }
                if (!rules || typeof rules !== "object" || Array.isArray(rules)) continue;
                let removed = 0;
                const replacement = { ...rules };
                for (const kind of ["prefetch", "prerender"]) {
                    if (!Array.isArray(rules[kind])) continue;
                    replacement[kind] = rules[kind].flatMap(rule => {
                        if (!rule || rule.source !== "list" || !Array.isArray(rule.urls)) {
                            return [rule];
                        }
                        const urls = rule.urls.filter(value => {
                            if (!exactCommercialUrl(value)) return true;
                            removed++;
                            return false;
                        });
                        return urls.length ? [{ ...rule, urls }] : [];
                    });
                }
                if (removed > 0) {
                    // Replacing updates the rules; requests already sent cannot be undone.
                    const next = element.cloneNode(false);
                    next.textContent = JSON.stringify(replacement);
                    element.replaceWith(next);
                    state.removedSpeculationUrls += removed;
                }
            }
        }

        function setIfChanged(settings, plugin, key, value) {
            if (settings[key] === value) return;
            settings[key] = value;
            state.settingChanges.push(plugin + "." + key);
            state.restartNeeded = true;
        }

        function configureKnownPlugins() {
            state.settingsAttempts++;
            const v = window.Vencord;
            const available = v?.Plugins?.plugins;
            if (!v?.Settings?.plugins || !available) {
                if (state.settingsAttempts < 30) setTimeout(configureKnownPlugins, 500);
                return;
            }

            const tracking = available.NoTrack;
            if (tracking?.settings?.def?.disableAnalytics) {
                const settings = v.Settings.plugins.NoTrack;
                setIfChanged(settings, "NoTrack", "enabled", true);
                setIfChanged(settings, "NoTrack", "disableAnalytics", true);
            } else {
                state.unsupportedSettings.push("NoTrack.disableAnalytics");
            }

            const offers = available.NoPendingCount;
            const definitions = offers?.settings?.def;
            if (definitions?.hidePremiumOffersCount &&
                definitions?.hideFriendRequestsCount &&
                definitions?.hideMessageRequestsCount) {
                const settings = v.Settings.plugins.NoPendingCount;
                setIfChanged(settings, "NoPendingCount", "enabled", true);
                setIfChanged(settings, "NoPendingCount", "hidePremiumOffersCount", true);
                setIfChanged(settings, "NoPendingCount", "hideFriendRequestsCount", false);
                setIfChanged(settings, "NoPendingCount", "hideMessageRequestsCount", false);
            } else {
                state.unsupportedSettings.push("NoPendingCount.offerOnlySettings");
            }
            state.settingsReady = true;
        }

        function initializeDocument() {
            installStyle();
            pruneCommercialHints();
            // One bounded follow-up covers hints inserted during initial hydration.
            setTimeout(pruneCommercialHints, 2000);
        }
        if (document.readyState === "loading") {
            document.addEventListener("DOMContentLoaded", initializeDocument, { once: true });
        } else {
            initializeDocument();
        }
        configureKnownPlugins();
        return state;
    })();
    """;

    internal const string StateScript = """
    (() => {
        "use strict";
        if (location.origin !== "https://discord.com" || window.top !== window.self) {
            return { supported: false };
        }
        const v = window.Vencord;
        // PlainSettings avoids proxy getters that materialize default values.
        const plain = v?.PlainSettings?.plugins;
        const state = window.__notDiscordLeanInterfaceState;
        return {
            supported: true,
            installed: state?.version === 1,
            styleInstalled: !!document.getElementById("notdiscord-lean-interface-style"),
            settingsReady: state?.settingsReady === true,
            restartNeeded: state?.restartNeeded === true,
            settingsAttempts: state?.settingsAttempts ?? 0,
            settingChanges: state?.settingChanges?.slice() ?? [],
            unsupportedSettings: state?.unsupportedSettings?.slice() ?? [],
            removedPrefetchLinks: state?.removedPrefetchLinks ?? 0,
            removedSpeculationUrls: state?.removedSpeculationUrls ?? 0,
            commercialPaths: state?.commercialPaths?.slice() ?? [],
            noTrack: {
                available: !!v?.Plugins?.plugins?.NoTrack,
                enabled: plain?.NoTrack?.enabled ?? null,
                disableAnalytics: plain?.NoTrack?.disableAnalytics ?? null
            },
            premiumOfferBadges: {
                available: !!v?.Plugins?.plugins?.NoPendingCount,
                enabled: plain?.NoPendingCount?.enabled ?? null,
                hidePremiumOffersCount: plain?.NoPendingCount?.hidePremiumOffersCount ?? null,
                hideFriendRequestsCount: plain?.NoPendingCount?.hideFriendRequestsCount ?? null,
                hideMessageRequestsCount: plain?.NoPendingCount?.hideMessageRequestsCount ?? null
            },
            ramSavingsMeasuredByThisScript: false
        };
    })();
    """;
}

