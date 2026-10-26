using System.Windows.Controls;

namespace NotDiscord.Terminal;

internal static class UiMemberMenus
{
    internal static ContextMenu Build(UiMemberModel model)
    {
        var person = model.Member.User; var menu = UiMenus.Create();
        UiMenus.Add(menu, "Ver perfil", () => model.Action(new("profile", person.Id)), model.CanAct);
        if (!model.IsSelf)
        {
            UiMenus.Add(menu, "Enviar mensaje", () => model.Action(new("open-dm", person.Id)), model.CanAct);
            if (!person.IsBot)
            {
                if (model.RelationshipType == 3) UiMenus.Add(menu, "Aceptar solicitud", () => model.Action(new("accept-friend", person.Id)), model.CanAct);
                else if (model.RelationshipType == 4) UiMenus.Add(menu, "Solicitud enviada", () => { }, false);
                else if (model.RelationshipType == 2) UiMenus.Add(menu, "Usuario bloqueado", () => { }, false);
                else if (model.RelationshipType == 0)
                {
                    UiMenus.Add(menu, "Añadir amigo", () => model.Action(new("add-friend", person.Id, person.Username)), model.CanAct && person.Username.Length > 0);
                    if (person.Username.Length == 0) { var item = (MenuItem)menu.Items[^1]; item.ToolTip = "Abre el perfil para cargar el nombre de usuario."; ToolTipService.SetShowOnDisabled(item, true); }
                }
            }
        }
        UiMenus.Add(menu, "Copiar ID de usuario", () => model.Action(new("copy-id", person.Id)));
        return menu;
    }
}
