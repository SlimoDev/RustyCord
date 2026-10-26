using System.Net.Http;
using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private async Task SendPreparedAsync(UiAction action, DiscordSession active, int generation)
    {
        RequirePermission("send");
        PreparedSend prepared;
        try { prepared = JsonSerializer.Deserialize<PreparedSend>(action.Extra) ?? throw new JsonException(); }
        catch (JsonException) { throw new ServiceException("No se pudo leer el borrador preparado. No se ha enviado nada."); }
        if (prepared.SubmissionId is not { Length: 32 } || prepared.SubmissionId.Any(c => !Uri.IsHexDigit(c)) || prepared.Paths == null || prepared.Paths.Length > 10 || prepared.Paths.Any(p => string.IsNullOrWhiteSpace(p)) || prepared.ReplyId == null || action.Text.Length > 4000) throw new ServiceException("El borrador preparado no es válido.");
        string replyId = prepared.ReplyId.Length > 0 ? Snowflake(prepared.ReplyId) : "";
        if (prepared.Paths.Length == 0 && string.IsNullOrWhiteSpace(action.Text)) return;
        if (prepared.Paths.Length > 0) RequirePermission("attach");
        long total = 0;
        foreach (string path in prepared.Paths)
        {
            var file = new FileInfo(path);
            if (!Path.IsPathFullyQualified(path) || !file.Exists || file.Length > 25 * 1024 * 1024) throw new ServiceException("Uno de los archivos ya no existe o supera 25 MiB. El borrador se conserva.");
            total += file.Length;
        }
        if (total > 100 * 1024 * 1024) throw new ServiceException("Los adjuntos del envío superan 100 MiB.");
        string channel = currentChannel, nonce = prepared.SubmissionId[..24];
        JsonElement sent;
        if (prepared.Paths.Length > 0) sent = await active.UploadManyAsync(channel, prepared.Paths, PrepareContent(action.Text), replyId, nonce, sessionWork.Token);
        else
        {
            var body = new Dictionary<string, object?> { ["content"] = PrepareContent(action.Text), ["nonce"] = nonce, ["enforce_nonce"] = true, ["allowed_mentions"] = new { parse = new[] { "users", "roles" }, replied_user = false } };
            if (replyId.Length > 0) body["message_reference"] = new { message_id = replyId, channel_id = channel, fail_if_not_exists = true };
            sent = await active.SendAsync(HttpMethod.Post, $"channels/{Snowflake(channel)}/messages", body, sessionWork.Token);
        }
        if (!IsCurrent(active, generation)) return;
        if (cache.ContainsChannel(channel)) cache.Upsert(ParseMessage(sent));
        window.AcknowledgePreparedSend(channel, action.Text, replyId, prepared.SubmissionId);
        UpdateMessages(); Status("Mensaje enviado");
    }
}
