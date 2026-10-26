namespace NotDiscord;

/// <summary>
/// NotDiscord's compatibility shim for the observed FixYoutubeEmbeds behavior.
/// This is not an official Vencord plugin and does not certify real YouTube playback.
/// The host must register this script only when FixYoutubeEmbeds is requested.
/// </summary>
internal static class YoutubeEmbedCompatibility
{
    internal const string Script = """
    (() => {
        "use strict";

        if (window.top === window.self) return;
        if (location.protocol !== "https:" ||
            location.hostname !== "www.youtube.com" ||
            !location.pathname.startsWith("/embed/")) return;

        const originalUrl = location.href;
        const retryKey = "notdiscord.youtubeEmbedCompatibility.retry.v1:" + originalUrl;
        const errorSelector =
            'div.ytp-error-content-wrap-subreason a[href*="www.youtube.com/watch?v="]';
        let observer = null;
        let retryScheduled = false;

        function stopObserving() {
            if (observer) observer.disconnect();
        }

        function checkForError() {
            if (retryScheduled || !document.querySelector(errorSelector)) return;

            try {
                // Read and write only this shim's own key for the exact embed URL.
                if (sessionStorage.getItem(retryKey) !== null) {
                    stopObserving();
                    return;
                }
                sessionStorage.setItem(retryKey, "1");
            } catch {
                // Without a persistent retry marker, reloading could form a loop.
                stopObserving();
                return;
            }

            retryScheduled = true;
            stopObserving();
            setTimeout(() => {
                if (location.href === originalUrl) location.reload();
            }, 250);
        }

        function startObserving() {
            if (!document.body) return;
            observer = new MutationObserver(checkForError);
            observer.observe(document.body, { childList: true, subtree: true });
            checkForError();
        }

        if (document.readyState === "loading") {
            document.addEventListener("DOMContentLoaded", startObserving, { once: true });
        } else {
            startObserving();
        }
    })();
    """;
}

