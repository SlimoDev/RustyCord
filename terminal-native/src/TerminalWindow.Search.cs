using System.Windows;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Input;
using System.Windows.Markup;
using System.Windows.Threading;
using System.Text.RegularExpressions;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private Popup? searchFiltersPopup;
    private bool searchSuggestionsQueued, searchSuggestionsSuppressed;
    private void WireSearchFilters()
    {
        searchFiltersPopup = new Popup { PlacementTarget = search, Placement = PlacementMode.Bottom, StaysOpen = false, AllowsTransparency = false, Focusable = false, VerticalOffset = 5 };
        search.GotKeyboardFocus += (_, _) => { ApplyHeaderLayout(); QueueSearchSuggestions(); };
        search.PreviewMouseLeftButtonDown += (_, _) => QueueSearchSuggestions();
        search.TextChanged += (_, _) => { if (search.IsKeyboardFocusWithin) QueueSearchSuggestions(); };
        search.SelectionChanged += (_, _) => { if (search.IsKeyboardFocusWithin && searchFiltersPopup.IsOpen) QueueSearchSuggestions(); };
        search.LostKeyboardFocus += (_, _) => ApplyHeaderLayout();
        search.PreviewKeyDown += (_, e) =>
        {
            if (e.Key == Key.Escape) { CloseSearchSuggestions(); e.Handled = true; }
            else if (e.Key == Key.Down && searchFiltersPopup.IsOpen && searchFiltersPopup.Child is UIElement child)
            { child.MoveFocus(new TraversalRequest(FocusNavigationDirection.First)); e.Handled = true; }
        };
        search.KeyDown += (_, e) => { if (e.Key == Key.Enter) CloseSearchSuggestions(); };
        search.IsEnabledChanged += (_, _) => { if (!search.IsEnabled) CloseSearchSuggestions(); };
        search.IsVisibleChanged += (_, _) => { if (!search.IsVisible) CloseSearchSuggestions(); };
        Deactivated += (_, _) => CloseSearchSuggestions();
        Closed += (_, _) => { CloseSearchSuggestions(); searchFiltersPopup.Child = null; };
    }
    private void CloseSearchSuggestions() { searchSuggestionsSuppressed = true; if (searchFiltersPopup != null) searchFiltersPopup.IsOpen = false; }
    private void QueueSearchSuggestions()
    {
        searchSuggestionsSuppressed = false;
        if (searchSuggestionsQueued) return;
        searchSuggestionsQueued = true;
        Dispatcher.BeginInvoke(DispatcherPriority.Loaded, new Action(() =>
        {
            searchSuggestionsQueued = false;
            if (searchSuggestionsSuppressed || searchFiltersPopup == null || !IsLoaded || !IsVisible || !IsActive || !search.IsVisible || !search.IsEnabled || !search.IsKeyboardFocusWithin) return;
            ApplyHeaderLayout(); search.UpdateLayout();
            // Populate and measure BEFORE opening. Assigning Child from Opened can
            // leave the first HWND sized at zero until another activation/layout.
            var content = new Border { Width = 410, MaxHeight = 570, Background = UiTheme.Menu, BorderBrush = line, BorderThickness = new Thickness(1),
                Child = new ScrollViewer { Content = BuildSearchSuggestions(), VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled } };
            content.Measure(new Size(410, 570));
            searchFiltersPopup.Child = content;
            searchFiltersPopup.HorizontalOffset = Math.Min(0, search.ActualWidth - content.Width);
            searchFiltersPopup.IsOpen = true;
            content.UpdateLayout();
        }));
    }
    private SearchContext CurrentSearchContext()
    {
        var known = state.SearchPeople.Where(p => p.Id.Length > 0).DistinctBy(p => p.Id).Take(2000).ToArray();
        return new(state.CurrentUser?.Id ?? "", known, state.SelectedGuildId.Length > 0 ? state.Channels.Where(c => c.Access?.CanView == true).ToArray() : Array.Empty<NavItem>());
    }
    private UIElement BuildSearchSuggestions()
    {
        var panel = new StackPanel { Margin = new Thickness(10) }; panel.Resources.MergedDictionaries.Add(Resources);
        var token = SearchCompletion.At(search.Text, search.CaretIndex);
        panel.Children.Add(new TextBlock { Text = token == null ? "Filtros" : "Valor de " + token.Key + ":", Foreground = Muted, FontSize = 13, Margin = new Thickness(7, 3, 0, 6) });
        var suggestions = SearchCompletion.Suggest(search.Text, search.CaretIndex, CurrentSearchContext());
        foreach (var suggestion in suggestions)
        {
            var caption = new StackPanel(); caption.Children.Add(Text(suggestion.Title, 14));
            caption.Children.Add(new TextBlock { Text = suggestion.Description, Foreground = Muted, FontSize = 12, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 2, 0, 0) });
            var choose = Button("", () => CompleteSearchTag(suggestion.Key, suggestion.Value)); choose.Content = caption;
            choose.HorizontalContentAlignment = HorizontalAlignment.Left; choose.Margin = new Thickness(0, 1, 0, 1); choose.Padding = new Thickness(8, 6, 8, 6); panel.Children.Add(choose);
        }
        if (suggestions.Count == 0) panel.Children.Add(new TextBlock { Text = "Escribe el valor completo o usa las opciones de abajo. No se han consultado personas fuera de esta sesión.", Foreground = Muted, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(7, 6, 7, 10) });
        var advanced = new Expander { Header = "Más filtros · fechas y opciones", Foreground = Ink, Margin = new Thickness(4, 10, 4, 0) };
        advanced.Expanded += (_, _) => { if (advanced.Content == null) advanced.Content = BuildSearchFilters(); };
        panel.Children.Add(advanced);
        panel.Children.Add(new TextBlock { Text = "Combina filtros y texto. Intro busca · ↓ elige sugerencia · Esc cierra", Foreground = Muted, FontSize = 11, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(7, 10, 7, 1) });
        return panel;
    }
    private void CompleteSearchTag(string key, string value)
    {
        var completed = SearchCompletion.Complete(search.Text, search.CaretIndex, key, value);
        search.Text = completed.Text; search.Focus(); search.CaretIndex = completed.Caret; QueueSearchSuggestions();
    }
    private void BuildChoiceStyles()
    {
        Resources["ThemeInput"] = UiTheme.Input; Resources["ThemeMenu"] = UiTheme.Menu; Resources["ThemeInk"] = Ink; Resources["ThemeLine"] = line;
        var combo = (Style)XamlReader.Parse("""
        <Style xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" TargetType="ComboBox">
          <Setter Property="Foreground" Value="{DynamicResource ThemeInk}"/><Setter Property="Background" Value="{DynamicResource ThemeInput}"/><Setter Property="BorderBrush" Value="{DynamicResource ThemeLine}"/><Setter Property="MinHeight" Value="34"/><Setter Property="Padding" Value="8,5"/>
          <Setter Property="Template"><Setter.Value><ControlTemplate TargetType="ComboBox"><Grid>
            <ToggleButton Focusable="False" IsChecked="{Binding IsDropDownOpen,RelativeSource={RelativeSource TemplatedParent},Mode=TwoWay}">
              <ToggleButton.Template><ControlTemplate TargetType="ToggleButton"><Border Background="{DynamicResource ThemeInput}" BorderBrush="{DynamicResource ThemeLine}" BorderThickness="1" CornerRadius="4"><Grid><TextBlock Text="⌄" Foreground="{DynamicResource ThemeInk}" HorizontalAlignment="Right" VerticalAlignment="Center" Margin="0,0,9,0"/></Grid></Border></ControlTemplate></ToggleButton.Template>
            </ToggleButton>
            <ContentPresenter Margin="9,5,27,5" VerticalAlignment="Center" IsHitTestVisible="False" Content="{TemplateBinding SelectionBoxItem}" ContentTemplate="{TemplateBinding SelectionBoxItemTemplate}" ContentTemplateSelector="{TemplateBinding ItemTemplateSelector}" ContentStringFormat="{TemplateBinding SelectionBoxItemStringFormat}"/>
            <Popup x:Name="PART_Popup" Placement="Bottom" IsOpen="{TemplateBinding IsDropDownOpen}" AllowsTransparency="False" Focusable="False"><Border Background="{DynamicResource ThemeMenu}" BorderBrush="{DynamicResource ThemeLine}" BorderThickness="1" MinWidth="{Binding ActualWidth,RelativeSource={RelativeSource TemplatedParent}}" MaxHeight="270"><ScrollViewer CanContentScroll="True" VerticalScrollBarVisibility="Auto"><ItemsPresenter/></ScrollViewer></Border></Popup>
          </Grid></ControlTemplate></Setter.Value></Setter>
        </Style>
        """);
        Resources[typeof(ComboBox)] = combo;
        combo.Setters.Add(new Setter(ItemsControl.ItemsPanelProperty, new ItemsPanelTemplate(new FrameworkElementFactory(typeof(VirtualizingStackPanel)))));
        combo.Setters.Add(new Setter(VirtualizingPanel.IsVirtualizingProperty, true)); combo.Setters.Add(new Setter(VirtualizingPanel.VirtualizationModeProperty, VirtualizationMode.Recycling));
        var item = new Style(typeof(ComboBoxItem)); item.Setters.Add(new Setter(Control.ForegroundProperty, Ink)); item.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Menu)); item.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(9, 6, 9, 6))); Resources[typeof(ComboBoxItem)] = item;
        var tab = new Style(typeof(TabItem)); tab.Setters.Add(new Setter(Control.ForegroundProperty, Ink)); tab.Setters.Add(new Setter(Control.BackgroundProperty, bg)); tab.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(12, 8, 12, 8)));
        var tabTemplate = new ControlTemplate(typeof(TabItem)); var tabBorder = new FrameworkElementFactory(typeof(Border)); tabBorder.Name = "TabBackground"; tabBorder.SetValue(Border.BackgroundProperty, bg); tabBorder.SetValue(Border.BorderBrushProperty, line); tabBorder.SetValue(Border.BorderThicknessProperty, new Thickness(0, 0, 0, 2));
        var caption = new FrameworkElementFactory(typeof(ContentPresenter)); caption.SetBinding(ContentPresenter.ContentProperty, new System.Windows.Data.Binding("Header") { RelativeSource = new(System.Windows.Data.RelativeSourceMode.TemplatedParent) }); caption.SetValue(MarginProperty, new Thickness(12, 8, 12, 8)); tabBorder.AppendChild(caption); tabTemplate.VisualTree = tabBorder;
        var selected = new Trigger { Property = TabItem.IsSelectedProperty, Value = true }; selected.Setters.Add(new Setter(Border.BorderBrushProperty, Accent, "TabBackground")); tabTemplate.Triggers.Add(selected); tab.Setters.Add(new Setter(Control.TemplateProperty, tabTemplate)); Resources[typeof(TabItem)] = tab;
    }

    private UIElement BuildSearchFilters()
    {
        var panel = new StackPanel { Margin = new Thickness(16) }; panel.Resources.MergedDictionaries.Add(Resources); panel.Children.Add(Text("Filtros de búsqueda", 18));
        panel.Children.Add(new TextBlock { Text = "Combina texto con filtros. Selecciona un valor o escribe la etiqueta directamente.", TextWrapping = TextWrapping.Wrap, Foreground = Muted, Margin = new Thickness(0, 7, 0, 9) });
        var people = state.SearchPeople.Where(p => p.Id.Length > 0).GroupBy(p => p.Id).Select(g => g.First()).OrderBy(p => p.Name).Take(500).ToArray();
        void Choice(string caption, string tag, IEnumerable<(string Label, string Value)> options)
        {
            panel.Children.Add(Label(caption));
            var choices = options.Select(x => new SearchChoice(x.Label, x.Value)).ToArray();
            var combo = new ComboBox { DisplayMemberPath = nameof(SearchChoice.Label), ItemsSource = choices, Margin = new Thickness(0, 3, 0, 1), MaxDropDownHeight = 230 };
            combo.SelectionChanged += (_, _) => { if (combo.SelectedItem is SearchChoice pick) SetSearchTag(tag, pick.Value); };
            panel.Children.Add(combo);
        }
        Choice("De una persona · from:", "from", new[] { ("Yo", "me") }.Concat(people.Select(p => (p.Name + (p.Username.Length > 0 ? " · @" + p.Username : ""), p.Id))));
        Choice("Menciona a una persona · mentions:", "mentions", new[] { ("Yo", "me") }.Concat(people.Select(p => (p.Name, p.Id))));
        Choice("Contiene · has:", "has", new[] { ("Enlace", "link"), ("Imagen", "image"), ("Vídeo", "video"), ("Archivo", "file"), ("Contenido incrustado", "embed"), ("Audio", "sound"), ("Encuesta", "poll"), ("Reenvío", "forward") });
        if (state.SelectedGuildId.Length > 0 && state.Channels.Count > 0) Choice("En un canal · in:", "in", state.Channels.Where(c => c.Access?.CanView == true && c.Kind is "text" or "thread").Select(c => ("# " + c.Name, c.Id)));
        var dates = new WrapPanel();
        foreach (var (title, tag) in new[] { ("Antes de", "before"), ("Después de", "after"), ("Durante el día", "during") })
        {
            var column = new StackPanel { Width = 150, Margin = new Thickness(0, 0, 8, 0) }; column.Children.Add(Label(title));
            var picker = new DatePicker { Background = UiTheme.Input, Foreground = Ink, ToolTip = tag + ":AAAA-MM-DD" }; picker.SelectedDateChanged += (_, _) => { if (picker.SelectedDate.HasValue) SetSearchTag(tag, picker.SelectedDate.Value.ToString("yyyy-MM-dd")); }; column.Children.Add(picker); dates.Children.Add(column);
        }
        panel.Children.Add(dates);
        Choice("Mensajes fijados", "pinned", new[] { ("Sí", "true"), ("No", "false") });
        Choice("Tipo de autor · filtro de los resultados cargados", "author", new[] { ("Personas", "user"), ("Bots", "bot"), ("Webhooks", "webhook") });
        var searchButton = Button("Buscar", () => { if (search.Text.Trim().Length > 0) { CloseSearchSuggestions(); Emit("search", text: search.Text); } }); searchButton.Margin = new Thickness(0, 15, 0, 0); panel.Children.Add(searchButton);
        return panel;
    }
    private void SetSearchTag(string key, string value)
    {
        CompleteSearchTag(key, value);
    }
    private sealed record SearchChoice(string Label, string Value);
}
