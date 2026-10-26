using System.Text.Json;
using Microsoft.Web.WebView2.Core;

namespace NotDiscord;

/// <summary>
/// Synthetic local WebRTC smoke test. It never opens capture devices, plays audio,
/// reads Discord state, uses remote ICE servers, or returns SDP/candidate addresses.
/// This is not a test of Discord calls, screen capture, device permissions or QoS.
/// </summary>
internal static class MediaSelfTest
{
    internal static async Task<JsonElement> RunAsync(CoreWebView2 core)
    {
        try
        {
            string response = await core.CallDevToolsProtocolMethodAsync("Runtime.evaluate",
                JsonSerializer.Serialize(new
                {
                    expression = Script,
                    awaitPromise = true,
                    returnByValue = true,
                    userGesture = true,
                    timeout = 12000
                }));
            using var parsed = JsonDocument.Parse(response);
            if (!parsed.RootElement.TryGetProperty("exceptionDetails", out _) &&
                parsed.RootElement.TryGetProperty("result", out var result) &&
                result.TryGetProperty("value", out var value) &&
                value.ValueKind == JsonValueKind.Object)
                return value.Clone();
            return JsonSerializer.SerializeToElement(new { success = false, error = "cdpEvaluationFailed" });
        }
        catch (Exception ex)
        {
            return JsonSerializer.SerializeToElement(new { success = false, error = ex.GetType().Name });
        }
    }

    internal const string Script = """
    (async () => {
      if (location.origin !== "https://discord.com")
        return { success: false, error: "unexpectedOrigin" };
      const begin = performance.now();
      const report = {
        success: false, synthetic: true, remoteIceServers: 0,
        microphoneUsed: false, screenCaptureUsed: false, speakerOutput: false,
        audioContextRunning: false, connected: false,
        audioPacketsReceived: 0, audioSamplesReceived: 0,
        videoPacketsReceived: 0, videoFramesDecoded: 0,
        videoFramesEncoded: 0, cleanupComplete: false
      };
      let sender, receiver, context, oscillator, destination, canvasStream, sink;
      let frameTimer, timeoutTimer, closed = false;
      const receivedTracks = [];
      const timeout = new Promise((_, reject) => {
        timeoutTimer = setTimeout(() => {
          const error = new Error("deadline"); error.name = "TimeoutError"; reject(error);
        }, 9500);
      });
      // Every asynchronous browser operation is bounded by the same deadline.
      const bounded = promise => Promise.race([promise, timeout]);
      const delay = ms => bounded(new Promise(resolve => setTimeout(resolve, ms)));
      try {
        if (typeof RTCPeerConnection !== "function" ||
            typeof AudioContext !== "function" ||
            typeof HTMLCanvasElement.prototype.captureStream !== "function")
          throw Object.assign(new Error("unavailable"), { name: "NotSupportedError" });
        context = new AudioContext({ sampleRate: 48000 });
        destination = context.createMediaStreamDestination();
        oscillator = context.createOscillator();
        oscillator.frequency.value = 440;
        oscillator.connect(destination); // Deliberately never connect to context.destination.
        oscillator.start();
        await bounded(context.resume());
        report.audioContextRunning = context.state === "running";

        const canvas = document.createElement("canvas");
        canvas.width = 160; canvas.height = 90;
        const drawing = canvas.getContext("2d", { alpha: false });
        if (!drawing) throw Object.assign(new Error("canvas"), { name: "NotSupportedError" });
        canvasStream = canvas.captureStream(0);
        const videoTrack = canvasStream.getVideoTracks()[0];
        let frame = 0;
        const paint = () => {
          if (closed) return;
          drawing.fillStyle = frame++ % 2 ? "#3377cc" : "#ee8844";
          drawing.fillRect(0, 0, 160, 90);
          drawing.fillStyle = "#ffffff";
          drawing.fillRect((frame * 7) % 140, 20, 20, 40);
          videoTrack.requestFrame();
        };
        paint();
        frameTimer = setInterval(paint, 100);

        // No STUN or TURN URLs; signaling and candidate exchange stay inside this closure.
        sender = new RTCPeerConnection({ iceServers: [], iceCandidatePoolSize: 0 });
        receiver = new RTCPeerConnection({ iceServers: [], iceCandidatePoolSize: 0 });
        const pendingSender = [], pendingReceiver = [];
        const candidateHandler = (other, pending) => event => {
          if (closed || !event.candidate) return;
          if (other.remoteDescription) other.addIceCandidate(event.candidate).catch(() => {});
          else pending.push(event.candidate);
        };
        sender.onicecandidate = candidateHandler(receiver, pendingReceiver);
        receiver.onicecandidate = candidateHandler(sender, pendingSender);
        const receivedStream = new MediaStream();
        sink = document.createElement("video");
        sink.muted = true; sink.autoplay = true; sink.playsInline = true;
        sink.width = 2; sink.height = 2;
        sink.style.cssText = "position:fixed;width:2px;height:2px;left:0;top:0;opacity:0;pointer-events:none";
        sink.srcObject = receivedStream;
        document.body.appendChild(sink);
        receiver.ontrack = event => {
          if (closed) { event.track.stop(); return; }
          receivedTracks.push(event.track);
          receivedStream.addTrack(event.track);
          sink.play().catch(() => {});
        };
        for (const track of destination.stream.getTracks()) sender.addTrack(track, destination.stream);
        sender.addTrack(videoTrack, canvasStream);
        await bounded(sender.setLocalDescription(await bounded(sender.createOffer())));
        await bounded(receiver.setRemoteDescription(sender.localDescription));
        for (const candidate of pendingReceiver.splice(0)) await bounded(receiver.addIceCandidate(candidate));
        await bounded(receiver.setLocalDescription(await bounded(receiver.createAnswer())));
        await bounded(sender.setRemoteDescription(receiver.localDescription));
        for (const candidate of pendingSender.splice(0)) await bounded(sender.addIceCandidate(candidate));
        // Gather only counters from these two temporary connections. No identifiers,
        // addresses, SDP, candidate details, device names or Discord calls are read.
        while (performance.now() - begin < 9000) {
          await delay(400);
          const incoming = await bounded(receiver.getStats());
          const outgoing = await bounded(sender.getStats());
          report.connected = sender.connectionState === "connected" && receiver.connectionState === "connected";
          for (const stat of incoming.values()) {
            if (stat.type !== "inbound-rtp") continue;
            if (stat.kind === "audio") {
              report.audioPacketsReceived = stat.packetsReceived || 0;
              report.audioSamplesReceived = stat.totalSamplesReceived || 0;
            } else if (stat.kind === "video") {
              report.videoPacketsReceived = stat.packetsReceived || 0;
              report.videoFramesDecoded = stat.framesDecoded || 0;
            }
          }
          for (const stat of outgoing.values()) {
            if (stat.type === "outbound-rtp" && stat.kind === "video")
              report.videoFramesEncoded = stat.framesEncoded || 0;
          }
          if (report.audioContextRunning && report.connected &&
              report.audioPacketsReceived >= 5 && report.audioSamplesReceived >= 960 &&
              report.videoPacketsReceived >= 3 && report.videoFramesDecoded >= 3 &&
              report.videoFramesEncoded >= 3) {
            report.success = true; break;
          }
        }
        if (!report.success) report.error = "mediaCountersNotConfirmed";
      } catch (error) {
        const allowed = ["TimeoutError", "NotSupportedError", "NotAllowedError",
                         "InvalidStateError", "OperationError", "AbortError"];
        report.error = allowed.includes(error?.name) ? error.name : "mediaTestFailed";
      } finally {
        closed = true;
        clearInterval(frameTimer); clearTimeout(timeoutTimer);
        if (sender) { sender.onicecandidate = null; sender.close(); }
        if (receiver) { receiver.onicecandidate = null; receiver.ontrack = null; receiver.close(); }
        for (const track of receivedTracks) track.stop();
        if (canvasStream) for (const track of canvasStream.getTracks()) track.stop();
        if (destination) for (const track of destination.stream.getTracks()) track.stop();
        if (oscillator) { try { oscillator.stop(); } catch {} oscillator.disconnect(); }
        if (destination) destination.disconnect();
        if (sink) { sink.pause(); sink.srcObject = null; sink.remove(); }
        if (context) {
          try {
            await Promise.race([context.close(), new Promise(resolve => setTimeout(resolve, 500))]);
          } catch {}
        }
        report.cleanupComplete = (!sender || sender.connectionState === "closed") &&
          (!receiver || receiver.connectionState === "closed") &&
          (!context || context.state === "closed") &&
          (!canvasStream || canvasStream.getTracks().every(track => track.readyState === "ended")) &&
          (!destination || destination.stream.getTracks().every(track => track.readyState === "ended")) &&
          receivedTracks.every(track => track.readyState === "ended") && (!sink || !sink.isConnected);
        report.success = report.success && report.cleanupComplete;
        report.elapsedMs = Math.round(performance.now() - begin);
      }
      return report;
    })()
    """;
}
