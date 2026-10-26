using System.Net.Http;

namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    private readonly HashSet<string> friendRequestsInFlight = new(StringComparer.OrdinalIgnoreCase);
    private async Task AddFriendAsync(UiAction action, DiscordSession active, int generation)
    {
        string username = action.Text.Trim(), requestKey;
        if (action.Id.Length > 0)
        {
            string id = Snowflake(action.Id);
            if (id == state.CurrentUser?.Id) throw new ServiceException("No puedes enviarte una solicitud de amistad.");
            var target = state.ProfileDetails?.User.Id == id ? state.ProfileDetails.User : knownPeople.GetValueOrDefault(id);
            target ??= state.GuildMembers.Concat(state.GroupMembers).Select(m => m.User).FirstOrDefault(p => p.Id == id);
            if (target == null) throw new ServiceException("Abre el perfil o la lista de miembros para confirmar esta persona.");
            if (target.IsBot) throw new ServiceException("No se pueden enviar solicitudes de amistad a bots.");
            int relationship = state.Relationships.FirstOrDefault(r => r.User.Id == id)?.Type ?? 0;
            if (relationship is 1 or 2 or 4) throw new ServiceException(relationship == 1 ? "Esta persona ya es tu amiga." : relationship == 2 ? "No puedes enviar una solicitud a una persona bloqueada." : "Ya hay una solicitud de amistad enviada.");
            username = target.Username.Trim(); requestKey = id;
        }
        else requestKey = username;
        if (username.Length is < 1 or > 128) throw new ServiceException("No está disponible el nombre de usuario para enviar la solicitud.");
        if (friendRequestsInFlight.Count >= 16 || !friendRequestsInFlight.Add(requestKey)) throw new ServiceException("Espera a que termine la solicitud de amistad en curso.");
        try
        {
            await active.SendAsync(HttpMethod.Post, "users/@me/relationships", new { username, discriminator = (string?)null }, sessionWork.Token);
            if (!IsCurrent(active, generation)) return;
            await RefreshRelationshipsAsync();
            if (IsCurrent(active, generation)) Status("Solicitud enviada");
        }
        finally { friendRequestsInFlight.Remove(requestKey); }
    }
}
