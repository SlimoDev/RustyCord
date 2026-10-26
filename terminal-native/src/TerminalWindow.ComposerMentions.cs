using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Media;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private readonly Popup composerMentionPopup = new() { Placement = PlacementMode.Top, AllowsTransparency = true, StaysOpen = true, Focusable = false };
    private readonly ListBox composerMentionList = new() { MaxHeight = 365, BorderThickness = new Thickness(0), Focusable = false, IsTabStop = false };
    private IReadOnlyList<ComposerMention> composerMentions = Array.Empty<ComposerMention>();
    private string composerMentionText = "";
    private Person[] composerMentionCandidates = Array.Empty<Person>();
    private ComposerMentionQuery? composerMentionQuery;
    private bool changingComposerMention;
    private string? dismissedComposerMentionText;
    private int dismissedComposerMentionCaret = -1;

    private void WireComposerMentions()
    {
        composerMentionList.Background = UiTheme.Panel; composerMentionList.Foreground = Ink;
        composerMentionList.ItemTemplate = CellTemplate<UiComposerMentionCell>();
        var itemStyle = new Style(typeof(ListBoxItem));
        itemStyle.Setters.Add(new Setter(Control.HorizontalContentAlignmentProperty, HorizontalAlignment.Stretch));
        itemStyle.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(2)));
        itemStyle.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Panel));
        itemStyle.Setters.Add(new Setter(UIElement.FocusableProperty, false));
        var selected = new Trigger { Property = ListBoxItem.IsSelectedProperty, Value = true }; selected.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Selected)); itemStyle.Triggers.Add(selected);
        composerMentionList.ItemContainerStyle = itemStyle;
        var layout = new DockPanel(); var caption = Text("MENCIONAR · ↑ ↓ · Enter o Tab", 11, Muted); caption.Margin = new Thickness(10, 7, 8, 5); DockPanel.SetDock(caption, Dock.Top); layout.Children.Add(caption); layout.Children.Add(composerMentionList);
        composerMentionPopup.PlacementTarget = composer;
        composerMentionPopup.Child = new Border { Width = 325, MaxHeight = 410, Background = UiTheme.Panel, BorderBrush = line, BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(7), Padding = new Thickness(3), Child = layout };
        composer.SelectionChanged += (_, _) => { if (!restoringDraft && !changingComposerMention) RefreshComposerMentions(); };
        composer.GotKeyboardFocus += (_, _) => RefreshComposerMentions();
        composer.LostKeyboardFocus += (_, _) => CloseComposerMentions();
        Deactivated += (_, _) => CloseComposerMentions();
        Closed += (_, _) => CloseComposerMentions();
    }
    private void UpdateComposerMentionText()
    {
        if (restoringDraft || changingComposerMention) return;
        composerMentions = ComposerMentionText.Adjust(composerMentionText, composer.Text, composerMentions);
        composerMentionText = composer.Text;
        RefreshComposerMentions();
    }
    private void LoadComposerMentionDraft(ComposerDraft draft)
    {
        CloseComposerMentions(); composerMentionText = draft.Text;
        composerMentions = ComposerMentionText.Valid(draft.Text, draft.Mentions);
        dismissedComposerMentionText = null; dismissedComposerMentionCaret = -1;
    }
    private void RefreshComposerMentions()
    {
        if (restoringDraft || changingComposerMention) return;
        var query = ComposerMentionText.FindQuery(composer.Text, composer.CaretIndex, composer.SelectionLength);
        if (query == null || dismissedComposerMentionText == composer.Text && dismissedComposerMentionCaret == composer.CaretIndex) { CloseComposerMentions(); return; }
        var candidates = ComposerMentionText.Candidates(state, query.Query);
        if (candidates.Length == 0) { CloseComposerMentions(); return; }
        string? selectedId = composerMentionList.SelectedItem is UiComposerMentionChoice old ? old.Person.Id : null;
        composerMentionQuery = query; composerMentionCandidates = candidates;
        composerMentionList.ItemsSource = candidates.Select(person => new UiComposerMentionChoice(person, CommitComposerMention)).ToArray();
        int selected = Array.FindIndex(candidates, person => person.Id == selectedId); composerMentionList.SelectedIndex = Math.Max(0, selected);
        // Unloaded WPF fixtures and background snapshots must not create a native
        // popup. Only the focused, visible editor can display suggestions.
        if (composer.IsKeyboardFocusWithin && PresentationSource.FromVisual(composer) != null) composerMentionPopup.IsOpen = true;
    }
    private void CloseComposerMentions(bool dismiss = false)
    {
        if (dismiss) { dismissedComposerMentionText = composer.Text; dismissedComposerMentionCaret = composer.CaretIndex; }
        composerMentionPopup.IsOpen = false; composerMentionQuery = null; composerMentionCandidates = Array.Empty<Person>(); composerMentionList.ItemsSource = null;
    }
    private bool HandleComposerMentionKey(KeyEventArgs e)
    {
        if (!composerMentionPopup.IsOpen || composerMentionCandidates.Length == 0) return false;
        if (e.Key is Key.Enter or Key.Tab && !IsComposerMentionSelectionKey(e.Key, Keyboard.Modifiers)) { CloseComposerMentions(); return false; }
        if (e.Key == Key.Escape) { CloseComposerMentions(dismiss: true); e.Handled = true; return true; }
        if (e.Key is Key.Up or Key.Down)
        {
            int current = Math.Max(0, composerMentionList.SelectedIndex), count = composerMentionCandidates.Length;
            composerMentionList.SelectedIndex = (current + (e.Key == Key.Down ? 1 : count - 1)) % count;
            if (composerMentionList.SelectedItem != null) composerMentionList.ScrollIntoView(composerMentionList.SelectedItem);
            e.Handled = true; return true;
        }
        if (e.Key is Key.Enter or Key.Tab)
        {
            var person = composerMentionCandidates[Math.Clamp(composerMentionList.SelectedIndex, 0, composerMentionCandidates.Length - 1)];
            CommitComposerMention(person); e.Handled = true; return true;
        }
        return false;
    }
    private static bool IsComposerMentionSelectionKey(Key key, ModifierKeys modifiers) => key is Key.Enter or Key.Tab && modifiers == ModifierKeys.None;
    private void CommitComposerMention(Person person)
    {
        var query = ComposerMentionText.FindQuery(composer.Text, composer.CaretIndex, composer.SelectionLength);
        if (query == null || !ComposerMentionText.Candidates(state, query.Query).Any(candidate => candidate.Id == person.Id)) { CloseComposerMentions(); return; }
        if (composerMentions.Count >= 100) { status.Text = "Máximo cien menciones seleccionadas por borrador."; CloseComposerMentions(); return; }
        string name = ComposerMentionText.Label(person).Replace('\r', ' ').Replace('\n', ' ').Trim();
        if (name.Length == 0) name = person.Username.Length > 0 ? person.Username : "usuario";
        string label = "@" + name;
        string current = composer.Text;
        string insertion = label + (query.Start + query.Length >= current.Length || !char.IsWhiteSpace(current[query.Start + query.Length]) ? " " : "");
        string next = current.Remove(query.Start, query.Length).Insert(query.Start, insertion);
        if (next.Length > composer.MaxLength) { status.Text = "No hay espacio para insertar la mención; el borrador se conserva."; CloseComposerMentions(); return; }
        var shifted = ComposerMentionText.Adjust(current, next, composerMentions);
        composerMentions = ComposerMentionText.Valid(next, shifted.Append(new ComposerMention(query.Start, label.Length, person.Id, label)).ToArray());
        changingComposerMention = true;
        try { composer.Text = next; composer.CaretIndex = query.Start + insertion.Length; composerMentionText = next; }
        finally { changingComposerMention = false; }
        CloseComposerMentions(); StoreActiveDraft(); RefreshComposerSendEligibility(); composer.Focus();
    }
}

public sealed record UiComposerMentionChoice(Person Person, Action<Person> Select);
public sealed class UiComposerMentionCell : Border
{
    public UiComposerMentionCell()
    {
        Padding = new Thickness(7, 5, 7, 5); Cursor = Cursors.Hand;
        DataContextChanged += (_, _) => Render();
        PreviewMouseLeftButtonDown += (_, e) => { if (DataContext is UiComposerMentionChoice choice) { e.Handled = true; choice.Select(choice.Person); } };
    }
    private void Render()
    {
        if (DataContext is not UiComposerMentionChoice choice) { Child = null; return; }
        var person = choice.Person; var row = new DockPanel(); var avatar = TerminalWindow.RoundAvatar(person, 30); avatar.Margin = new Thickness(0, 0, 9, 0); DockPanel.SetDock(avatar, Dock.Left); row.Children.Add(avatar);
        var labels = new StackPanel(); var name = TerminalWindow.Text(ComposerMentionText.Label(person), 14); name.TextWrapping = TextWrapping.NoWrap; name.TextTrimming = TextTrimming.CharacterEllipsis; labels.Children.Add(name);
        if (person.Username.Length > 0) { var username = TerminalWindow.Text("@" + person.Username, 11, TerminalWindow.Muted); username.TextTrimming = TextTrimming.CharacterEllipsis; labels.Children.Add(username); }
        row.Children.Add(labels); Child = row; ToolTip = person.Username.Length > 0 ? "@" + person.Username : person.Name;
    }
}
