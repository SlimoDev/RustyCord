namespace NotDiscord.Terminal;

public sealed partial class Controller
{
    // Compatibility cleanup only. Visibility never starts detection or reads its old preferences.
    private NativeGameActivity? gameActivity;
    private bool activityVisibilityChanging;
    private void SyncActivityState()
    {
        bool? visible = session?.ShowCurrentGame;
        state = state with { GameActivityConfigured = visible.HasValue, GameActivityLabel = "Visibilidad de actividad",
            GameActivityEnabled = visible == true, GameActivityAvailable = visible.HasValue && state.Connection == "En línea",
            GameActivityRunning = false, CustomActivityLabel = manualActivity };
        Refresh();
    }
    private async Task ConnectionStatusAsync(DiscordSession active, int generation, string status)
    {
        if (!IsCurrent(active, generation)) return;
        state = state with { Connection = status, SelfPresence = active.SelfPresence }; SyncActivityState();
        await StopGameActivityAsync();
        if (status == "En línea" && IsCurrent(active, generation)) await SubscribeVisibleGuildAsync(active, generation);
    }
    private Task StartGameActivityAsync(DiscordSession active, int generation)
    {
        if (IsCurrent(active, generation)) SyncActivityState();
        return Task.CompletedTask;
    }
    private async Task StopGameActivityAsync()
    {
        if (gameActivity != null) await gameActivity.StopAsync();
        state = state with { GameActivityRunning = false }; Refresh();
    }
    private async Task HandleGameActivityActionAsync(UiAction action, DiscordSession active, int generation)
    {
        if (action.Kind == "configure-game-activity") throw new ServiceException("La visibilidad de actividad ya no requiere configurar un ejecutable.");
        if (action.Kind == "toggle-game-activity")
        {
            if (activityVisibilityChanging) return;
            activityVisibilityChanging = true;
            try
            {
                await StopGameActivityAsync();
                if (!IsCurrent(active, generation)) return;
                if (!active.ShowCurrentGame.HasValue) await active.RefreshActivityVisibilityAsync(sessionWork.Token);
                if (!IsCurrent(active, generation)) return;
                bool visible = !(active.ShowCurrentGame ?? throw new ServiceException("No se conoce la visibilidad de tu actividad."));
                try { await active.SetActivityVisibilityAsync(visible, sessionWork.Token); }
                finally { if (IsCurrent(active, generation)) SyncActivityState(); }
                if (IsCurrent(active, generation)) Status(visible ? "Visibilidad de actividad activada y confirmada por Discord." : "Visibilidad de actividad desactivada y confirmada por Discord.");
            }
            finally { activityVisibilityChanging = false; }
            return;
        }
        string? activity = action.Text.Trim();
        if (activity.Length == 0) activity = null;
        if (activity is { Length: > 128 } || activity?.Any(char.IsControl) == true) throw new ServiceException("La actividad personalizada admite hasta 128 caracteres sin controles.");
        await StopGameActivityAsync();
        if (!IsCurrent(active, generation)) return;
        await active.SetActivityAsync(activity);
        if (!IsCurrent(active, generation)) return;
        manualActivity = activity ?? ""; SyncActivityState();
        Status(activity == null ? "Actividad personalizada eliminada" : active.ShowCurrentGame == false ? "Actividad personalizada guardada para esta sesión; su visibilidad está desactivada." : "Actividad personalizada enviada al Gateway.");
    }
}
