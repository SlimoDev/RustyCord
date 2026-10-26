using System.Net.Http;
using System.Text.Json;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private async Task ForwardManyAsync(UiAction action, DiscordSession active, int generation)
    {
        RequireSelectedSocialChannel(action); RequirePermission("history");
        var source = cache.Get(currentChannel, action.Id) ?? throw new ServiceException("El mensaje ya no está en la vista. No se ha reenviado nada.");
        if (source.Deleted || source.MessageType is not (0 or 19) || source.Forwarded) throw new ServiceException("Sólo se pueden reenviar mensajes originales disponibles.");
        string[] targets;
        try { targets = action.Kind == "forward" ? new[] { Snowflake(action.Text) } : (JsonSerializer.Deserialize<string[]>(action.Text) ?? Array.Empty<string>()).Select(Snowflake).Distinct().Take(11).ToArray(); }
        catch (JsonException) { throw new ServiceException("La lista de destinos no es válida."); }
        if (targets.Length is < 1 or > 10) throw new ServiceException("Selecciona entre uno y diez destinos.");
        string originGuild = currentGuild;
        var guildChecks = new Dictionary<string, (GuildAccess Access, Dictionary<string, JsonElement> Channels)>();
        foreach (string target in targets)
        {
            if (state.DirectMessages.Any(c => c.Id == target && c.Kind is "dm" or "group")) continue;
            string guildId = forwardAccess.TryGetValue(target, out var saved) ? saved.Guild : state.Channels.Any(c => c.Id == target) ? currentGuild : "";
            if (guildId.Length == 0 || !state.Guilds.Any(g => g.Id == guildId)) throw new ServiceException("Uno de los destinos ya no está disponible. No se ha reenviado nada.");
            if (!guildChecks.TryGetValue(guildId, out var check))
            {
                var access = await FetchGuildAccessAsync(active, generation, guildId, sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                if (access == null) throw new ServiceException("No se pudieron confirmar los permisos de un destino. No se ha reenviado nada.");
                var response = await active.GetAsync($"guilds/{guildId}/channels", sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                check = (access, Items(response).Take(2000).ToDictionary(c => Str(c, "id"), c => c.Clone(), StringComparer.Ordinal)); guildChecks[guildId] = check;
            }
            if (!check.Channels.TryGetValue(target, out var channel) || ParseChannel(channel).Kind is not ("text" or "thread")) throw new ServiceException("Un destino no es un canal de texto disponible. No se ha reenviado nada.");
            var permission = AccessFor(channel, check.Access, check.Channels);
            if (!permission.CanView || !permission.CanSend || source.Attachments.Any(a => !a.Id.StartsWith("embed-", StringComparison.Ordinal)) && !permission.CanAttach || source.Attachments.Any(a => a.Id.StartsWith("embed-", StringComparison.Ordinal)) && !permission.CanEmbed)
                throw new ServiceException("No puedes enviar este contenido a «" + ParseChannel(channel).Name + "». No se ha reenviado nada.");
        }
        var completed = new List<string>();
        foreach (string target in targets)
        {
            if (!IsCurrent(active, generation)) return;
            try
            {
                var forwarded = await active.SendAsync(HttpMethod.Post, $"channels/{target}/messages", ForwardPayload(source, originGuild), sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                completed.Add(target); if (cache.ContainsChannel(target)) cache.Upsert(ParseMessage(forwarded));
            }
            catch (ServiceException)
            { UpdateMessages(); throw new ServiceException(completed.Count == 0 ? "Discord rechazó el reenvío. No se completó ningún destino." : $"Reenvío parcial: {completed.Count} de {targets.Length} destinos completados. No se reintentó para evitar duplicados."); }
        }
        UpdateMessages(); Status($"Mensaje reenviado a {completed.Count} destino(s)");
    }
}
