using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Media;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private async void ShowForward(UiAction source)
    {
        if (attachedController == null) return;
        var sourceMessage = state.Messages.FirstOrDefault(m => m.Id == source.Id);
        var dialog = Dialog("Reenviar mensaje", 540, 650);
        var layout = new DockPanel { Margin = new Thickness(16) };
        var description = Text("Selecciona hasta diez destinos y confirma el reenvío.", 14, Muted); description.TextWrapping = TextWrapping.Wrap;
        description.Margin = new Thickness(0, 0, 0, 10); DockPanel.SetDock(description, Dock.Top); layout.Children.Add(description);
        var scopes = new ComboBox { ItemTemplate = CellTemplate<UiForwardScopeCell>(), ItemsSource = new[] { new NavItem("", "Conversaciones", "section") }.Concat(state.Guilds).ToArray(), SelectedIndex = 0, Margin = new Thickness(0, 0, 0, 10) };
        DockPanel.SetDock(scopes, Dock.Top); layout.Children.Add(scopes);
        var query = new TextBox { ToolTip = "Buscar una conversación o canal", MaxLength = 100, Margin = new Thickness(0, 0, 0, 8) }; DockPanel.SetDock(query, Dock.Top); layout.Children.Add(query);
        var selected = new Dictionary<string, NavItem>(StringComparer.Ordinal);
        var list = new ListBox { ItemTemplate = CellTemplate<UiForwardTargetCell>() }; Virtual(list);
        var footer = new StackPanel(); var counter = Text("Ningún destino seleccionado", 12, Muted); counter.Margin = new Thickness(0, 5, 0, 8); footer.Children.Add(counter);
        var forward = Button("Reenviar", () =>
        {
            if (selected.Count == 0) return;
            if (!Confirm("Confirmar reenvío", "¿Reenviar el mensaje a " + selected.Count + " destino(s)?\n\n" + string.Join("\n", selected.Values.Select(n => n.Name)))) return;
            string payload = JsonSerializer.Serialize(selected.Keys.ToArray()); dialog.Close(); Handle(source with { Kind = "forward-multiple", Text = payload });
        }); forward.IsEnabled = false; footer.Children.Add(forward); DockPanel.SetDock(footer, Dock.Bottom); layout.Children.Add(footer);
        IReadOnlyList<NavItem> targets = state.DirectMessages;
        bool Toggle(NavItem target, bool check)
        {
            if (!check) selected.Remove(target.Id);
            else if (selected.Count >= 10 && !selected.ContainsKey(target.Id)) { description.Text = "Máximo diez destinos. Quita uno para elegir otro."; return false; }
            else selected[target.Id] = target;
            counter.Text = selected.Count == 0 ? "Ningún destino seleccionado" : selected.Count + " seleccionado(s): " + string.Join(", ", selected.Values.Select(n => n.Name));
            counter.TextTrimming = TextTrimming.CharacterEllipsis; forward.IsEnabled = selected.Count > 0; return true;
        }
        void Filter() => list.ItemsSource = targets.Where(n => n.Name.Contains(query.Text, StringComparison.OrdinalIgnoreCase)).Take(500)
            .Select(n => new UiForwardTargetModel(n, selected.ContainsKey(n.Id), ForwardReason(n, sourceMessage), Toggle)).ToArray();
        Filter(); query.TextChanged += (_, _) => Filter();
        using var lifetime = new CancellationTokenSource(); int version = 0;
        dialog.Closed += (_, _) => lifetime.Cancel();
        scopes.SelectionChanged += async (_, _) =>
        {
            int attempt = ++version; if (scopes.SelectedItem is not NavItem scope) return;
            try
            {
                targets = Array.Empty<NavItem>(); Filter(); description.Text = "Cargando canales visibles…";
                var fetched = scope.Id.Length == 0 ? state.DirectMessages : await attachedController.GetForwardChannelsAsync(scope.Id, lifetime.Token);
                if (lifetime.IsCancellationRequested || version != attempt) return;
                targets = fetched; Filter(); description.Text = "Selecciona hasta diez destinos. Los canales sin permiso de envío se muestran deshabilitados.";
            }
            catch (OperationCanceledException) { }
            catch (Exception error) when (error is ServiceException or System.Net.Http.HttpRequestException or System.IO.IOException)
            { if (!lifetime.IsCancellationRequested) description.Text = "No se pudieron cargar los canales de ese servidor."; }
        };
        layout.Children.Add(list); dialog.Content = layout;
        await Dispatcher.InvokeAsync(() => dialog.ShowDialog());
    }
    private static string ForwardReason(NavItem item, ChatMessage? source)
    {
        var access = item.Access ?? (item.Kind is "dm" or "group" ? ChannelAccess.Private : ChannelAccess.Unknown);
        if (!access.Known || !access.CanView) return access.Reason.Length > 0 ? access.Reason : "No se ha confirmado el acceso al canal.";
        if (!access.CanSend) return "No tienes permiso para enviar mensajes en este canal.";
        if (source?.Attachments.Any(a => !a.Id.StartsWith("embed-", StringComparison.Ordinal)) == true && !access.CanAttach) return "No tienes permiso para reenviar archivos en este canal.";
        if (source?.Attachments.Any(a => a.Id.StartsWith("embed-", StringComparison.Ordinal)) == true && !access.CanEmbed) return "No tienes permiso para reenviar contenido incrustado en este canal.";
        return "";
    }
}

public sealed class UiForwardScopeCell : Border
{
    public UiForwardScopeCell() { DataContextChanged += (_, _) => Render(); Padding = new Thickness(2); }
    private void Render()
    {
        if (DataContext is not NavItem item) { Child = null; return; }
        var row = new DockPanel(); FrameworkElement icon = item.Id.Length == 0 ? TerminalWindow.DiscordLogo(24) : TerminalWindow.NavAvatar(item, 24);
        icon.Margin = new Thickness(0, 0, 8, 0); DockPanel.SetDock(icon, Dock.Left); row.Children.Add(icon);
        var name = TerminalWindow.Text(item.Name, 14); name.TextTrimming = TextTrimming.CharacterEllipsis; row.Children.Add(name); Child = row;
    }
}
public sealed record UiForwardTargetModel(NavItem Target, bool Selected, string DisabledReason, Func<NavItem, bool, bool> Toggle);
public sealed class UiForwardTargetCell : Border
{
    public UiForwardTargetCell() { Padding = new Thickness(3, 4, 3, 4); DataContextChanged += (_, _) => Render(); }
    private void Render()
    {
        if (DataContext is not UiForwardTargetModel model) { Child = null; return; }
        var row = new DockPanel(); var icon = TerminalWindow.NavAvatar(model.Target, 36); icon.Margin = new Thickness(8, 0, 10, 0); DockPanel.SetDock(icon, Dock.Left); row.Children.Add(icon);
        var labels = new StackPanel(); var name = TerminalWindow.Text((model.Target.Kind is "text" or "thread" ? "# " : "") + model.Target.Name, 15); name.TextTrimming = TextTrimming.CharacterEllipsis; labels.Children.Add(name);
        if (model.DisabledReason.Length > 0) { var reason = TerminalWindow.Text(model.DisabledReason, 11, TerminalWindow.Muted); reason.TextWrapping = TextWrapping.Wrap; labels.Children.Add(reason); }
        row.Children.Add(labels);
        var check = new CheckBox { Content = row, IsChecked = model.Selected, IsEnabled = model.DisabledReason.Length == 0, Foreground = TerminalWindow.Ink, HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Center, ToolTip = model.DisabledReason.Length > 0 ? model.DisabledReason : "Seleccionar destino" };
        ToolTipService.SetShowOnDisabled(check, true); bool updating = false;
        void Changed() { if (updating) return; bool next = check.IsChecked == true; if (!model.Toggle(model.Target, next)) { updating = true; check.IsChecked = false; updating = false; } }
        check.Checked += (_, _) => Changed(); check.Unchecked += (_, _) => Changed(); Child = check;
    }
}
