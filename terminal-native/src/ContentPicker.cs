using System.Windows;
using System.IO;
using System.Windows.Controls;
using System.Windows.Controls.Primitives;
using System.Windows.Data;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Threading;

namespace NotDiscord.Terminal;

/// <summary>Native pickers: only visible rows create controls or request static thumbnails.</summary>
public static class ContentPicker
{
    public static void ShowEmoji(Window owner, AppSnapshot snapshot,
        Func<string, CancellationToken, Task<IReadOnlyList<CustomEmoji>>> loadGuild, Action<string> insert, string dataDirectory)
        => new PickerWindow(owner, snapshot, dataDirectory, insert, loadGuild, null).ShowDialog();
    public static void ShowGif(Window owner, AppSnapshot snapshot,
        Func<GifQuery, CancellationToken, Task<GifPage>> load, Action<string> insert, string dataDirectory)
        => new PickerWindow(owner, snapshot, dataDirectory, insert, null, load).ShowDialog();

    private sealed record Category(string Key, string Name) { public override string ToString() => Name; }
    private sealed class PickerWindow : Window
    {
        private readonly bool gif;
        private readonly PickerHistory history;
        private readonly Action<string> insert;
        private readonly Func<string, CancellationToken, Task<IReadOnlyList<CustomEmoji>>>? loadGuild;
        private readonly Func<GifQuery, CancellationToken, Task<GifPage>>? loadGifs;
        private readonly ComboBox category = new();
        private readonly TextBox search = new();
        private readonly ListBox rows = new();
        private readonly TextBlock note = new();
        private readonly Button more = new();
        private readonly DispatcherTimer debounce = new() { Interval = TimeSpan.FromMilliseconds(300) };
        private CancellationTokenSource? work;
        private IReadOnlyList<CustomEmoji> custom = Array.Empty<CustomEmoji>();
        private string customGuild = "";
        private int limit = 20;
        private bool ready, closed;

        public PickerWindow(Window owner, AppSnapshot snapshot, string directory, Action<string> insert,
            Func<string, CancellationToken, Task<IReadOnlyList<CustomEmoji>>>? loadGuild,
            Func<GifQuery, CancellationToken, Task<GifPage>>? loadGifs)
        {
            this.insert = insert; this.loadGuild = loadGuild; this.loadGifs = loadGifs; gif = loadGifs != null;
            history = new(directory, snapshot.CurrentUser?.Id ?? "demo");
            Owner = owner; Title = gif ? "GIF" : "Emojis"; Width = gif ? 620 : 610; Height = 580;
            MinWidth = 540; MinHeight = 420; WindowStartupLocation = WindowStartupLocation.CenterOwner;
            Background = UiTheme.Panel; Foreground = UiTheme.Ink; FontFamily = new FontFamily("Segoe UI"); FontSize = 14;
            Resources[typeof(Button)] = ButtonStyle();
            var layout = new DockPanel { Margin = new Thickness(14) }; Content = layout;
            var top = new StackPanel(); DockPanel.SetDock(top, Dock.Top); layout.Children.Add(top);
            var title = new TextBlock { Text = gif ? "Busca un GIF" : "Elige un emoji", FontSize = 21, FontWeight = FontWeights.SemiBold, Margin = new Thickness(0, 0, 0, 10) }; top.Children.Add(title);
            if (gif)
            {
                var import = new Button { Content = "Importar enlaces de GIF…", HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 0, 0, 8), ToolTip = "Alternativa local para favoritos de Discord que el servicio no entregue" };
                import.Click += async (_, _) => { if (ImportGifLinks()) { bool refresh = (category.SelectedItem as Category)?.Key == "favorites"; category.SelectedItem = category.Items.OfType<Category>().First(c => c.Key == "favorites"); if (refresh) await LoadAsync(); } };
                top.Children.Add(import);
            }
            if (!gif)
            {
                var recent = new StackPanel { Name = "RecentEmojiSection", Margin = new Thickness(0, 0, 0, 10) };
                recent.Children.Add(new TextBlock { Text = "USADOS RECIENTEMENTE", Foreground = UiTheme.Muted, FontSize = 11, Margin = new Thickness(3, 0, 0, 4) });
                if (history.Data.EmojiRecent.Count > 0)
                    recent.Children.Add(new ContentPickerRow { DataContext = new PickerRowModel(history.Data.EmojiRecent.Take(10).Select(v => new PickerCard(v, false, () => Choose(v), null, false)).ToArray(), 10) });
                else recent.Children.Add(new TextBlock { Text = "Aquí aparecerán los últimos 10 emojis que uses.", Foreground = UiTheme.Muted, FontSize = 12, Margin = new Thickness(3, 4, 0, 8) });
                top.Children.Add(recent);
            }
            var filters = new Grid { Margin = new Thickness(0, 0, 0, 8) }; filters.ColumnDefinitions.Add(new() { Width = new GridLength(185) }); filters.ColumnDefinitions.Add(new());
            filters.Name = "PickerCategoryFilters";
            category.Foreground = UiTheme.Ink; category.Background = UiTheme.Input; category.Padding = new Thickness(5); category.Margin = new Thickness(0, 0, 8, 0);
            category.Style = CategoryStyle();
            category.ItemsPanel = new ItemsPanelTemplate(new FrameworkElementFactory(typeof(VirtualizingStackPanel)));
            VirtualizingPanel.SetIsVirtualizing(category, true); VirtualizingPanel.SetVirtualizationMode(category, VirtualizationMode.Recycling);
            var categoryStyle = new Style(typeof(ComboBoxItem)); categoryStyle.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Input)); categoryStyle.Setters.Add(new Setter(Control.ForegroundProperty, UiTheme.Ink)); categoryStyle.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(8, 5, 8, 5)));
            var selectedCategory = new Trigger { Property = ComboBoxItem.IsSelectedProperty, Value = true }; selectedCategory.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Hover)); categoryStyle.Triggers.Add(selectedCategory); category.ItemContainerStyle = categoryStyle;
            filters.Children.Add(category);
            search.Background = UiTheme.Input; search.Foreground = UiTheme.Ink; search.CaretBrush = UiTheme.Ink; search.BorderBrush = UiTheme.Line; search.Padding = new Thickness(9, 6, 9, 6);
            search.ToolTip = gif ? "Busca GIF por palabra o categoría" : "Busca por nombre en español o inglés, o pega un emoji";
            Grid.SetColumn(search, 1); filters.Children.Add(search); top.Children.Add(filters);
            var footer = new DockPanel { Margin = new Thickness(0, 8, 0, 0) }; DockPanel.SetDock(footer, Dock.Bottom); layout.Children.Add(footer);
            more.Content = "Cargar más"; more.Visibility = Visibility.Collapsed; more.Click += async (_, _) => { limit = DiscordGifCatalog.NextLimit(limit); await LoadAsync(); }; DockPanel.SetDock(more, Dock.Right); footer.Children.Add(more);
            note.Foreground = UiTheme.Muted; note.TextWrapping = TextWrapping.Wrap; note.FontSize = 12; note.Margin = new Thickness(0, 0, 8, 0); footer.Children.Add(note);
            rows.Background = UiTheme.Panel; rows.Foreground = UiTheme.Ink; rows.BorderThickness = new Thickness(0); rows.Padding = new Thickness(0); rows.HorizontalContentAlignment = HorizontalAlignment.Stretch;
            ScrollViewer.SetHorizontalScrollBarVisibility(rows, ScrollBarVisibility.Disabled); ScrollViewer.SetVerticalScrollBarVisibility(rows, ScrollBarVisibility.Auto); ScrollViewer.SetCanContentScroll(rows, true);
            VirtualizingPanel.SetIsVirtualizing(rows, true); VirtualizingPanel.SetVirtualizationMode(rows, VirtualizationMode.Recycling); VirtualizingPanel.SetCacheLength(rows, new VirtualizationCacheLength(0));
            rows.ItemsPanel = new ItemsPanelTemplate(new FrameworkElementFactory(typeof(VirtualizingStackPanel)));
            rows.ItemTemplate = new DataTemplate { VisualTree = new FrameworkElementFactory(typeof(ContentPickerRow)) };
            var itemStyle = new Style(typeof(ListBoxItem)); itemStyle.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(0))); itemStyle.Setters.Add(new Setter(Control.BorderThicknessProperty, new Thickness(0))); itemStyle.Setters.Add(new Setter(Control.HorizontalContentAlignmentProperty, HorizontalAlignment.Stretch)); itemStyle.Setters.Add(new Setter(Control.FocusableProperty, false)); rows.ItemContainerStyle = itemStyle;
            layout.Children.Add(rows);
            if (gif)
            {
                category.Items.Add(new Category("trending", "Populares")); category.Items.Add(new Category("categories", "Categorías")); category.Items.Add(new Category("favorites", "★ Guardados")); category.Items.Add(new Category("recent", "Recientes"));
            }
            else
            {
                category.Items.Add(new Category("", "Todos · Windows"));
                foreach (var group in EmojiCatalog.Categories) category.Items.Add(new Category("unicode:" + group.Key, group.Value));
                foreach (var guild in snapshot.Guilds) category.Items.Add(new Category("guild:" + guild.Id, "Servidor · " + guild.Name));
            }
            category.SelectedIndex = 0;
            category.SelectionChanged += async (_, _) => { if (!ready) return; limit = 20; search.Text = ""; debounce.Stop(); await LoadAsync(); };
            search.TextChanged += (_, _) => { if (!ready) return; limit = 20; debounce.Stop(); debounce.Start(); };
            debounce.Tick += async (_, _) => { debounce.Stop(); await LoadAsync(); };
            PreviewKeyDown += (_, e) => { if (e.Key == Key.Escape) { Close(); e.Handled = true; } };
            Loaded += async (_, _) => { ready = true; search.Focus(); await LoadAsync(); };
            Closed += (_, _) => { closed = true; debounce.Stop(); work?.Cancel(); work?.Dispose(); work = null; rows.ItemsSource = null; };
        }

        private async Task LoadAsync()
        {
            work?.Cancel(); work?.Dispose(); var current = work = new CancellationTokenSource();
            string mode = (category.SelectedItem as Category)?.Key ?? "";
            string query = search.Text.Trim(); if (query.Length > 150) query = query[..150];
            more.Visibility = Visibility.Collapsed; note.Text = "Cargando…";
            try
            {
                IReadOnlyList<PickerValue> values;
                if (gif)
                {
                    if (mode == "recent") values = history.Data.GifRecent;
                    else
                    {
                        var request = new GifQuery(query.Length > 0 && mode is "trending" or "categories" ? "search" : mode, query, limit);
                        var result = await loadGifs!(request, current.Token);
                        if (Stale(current)) return;
                        if (mode == "categories" && query.Length == 0)
                        {
                            var cards = (result.Categories ?? Array.Empty<GifCategory>()).Select(c => new PickerCard(new(c.Name, c.Name, c.ThumbnailUrl), false,
                                () => { search.Text = c.Name; }, null, false)).ToArray();
                            Render(cards, 3); note.Text = cards.Length == 0 ? "No hay categorías disponibles." : "Elige una categoría para buscar sus GIF."; return;
                        }
                        var remote = result.Items.Select(i => new PickerValue(i.Url, i.Title, i.ThumbnailUrl));
                        values = mode == "favorites" ? history.Data.GifFavorites.Concat(remote).DistinctBy(v => v.Value).Take(400).ToArray() : remote.ToArray();
                        more.Visibility = result.HasMore ? Visibility.Visible : Visibility.Collapsed;
                        note.Text = result.Note;
                    }
                    if (query.Length > 0 && mode is "favorites" or "recent") values = values.Where(v => Matches(v, query)).ToArray();
                }
                else
                {
                    if (mode.StartsWith("guild:", StringComparison.Ordinal))
                    {
                        string id = mode[6..];
                        if (customGuild != id) { custom = await loadGuild!(id, current.Token); if (Stale(current)) return; customGuild = id; }
                        values = custom.Where(e => EmojiCatalog.Normalize(e.Name).Contains(EmojiCatalog.Normalize(query), StringComparison.Ordinal))
                            .Select(e => new PickerValue($"<{(e.Animated ? "a" : "")}:{e.Name}:{e.Id}>", e.Name, $"https://cdn.discordapp.com/emojis/{e.Id}.png?size=48&animated=false")).ToArray();
                    }
                    else values = EmojiCatalog.Search(mode.StartsWith("unicode:", StringComparison.Ordinal) ? mode[8..] : "", query).Select(e => new PickerValue(e.Value, e.Name)).ToArray();
                }
                if (Stale(current)) return;
                var favorites = gif ? history.Data.GifFavorites.Select(v => v.Value).ToHashSet(StringComparer.Ordinal) : null;
                Render(values.Select(v => new PickerCard(v, gif, () => Choose(v), gif ? () => ToggleFavorite(v) : null, favorites?.Contains(v.Value) == true)).ToArray(), gif ? 3 : 8);
                if (note.Text == "Cargando…" || note.Text.Length == 0) note.Text = gif
                    ? "Elige para insertar en el borrador. ★ guarda sólo en NotDiscord; ninguna selección se envía automáticamente."
                    : "Emojis de Windows · clic inserta en el borrador y actualiza los 10 recientes. Servidores: elige uno en la lista.";
                if (values.Count == 0) note.Text = gif && mode is "recent" or "favorites" ? "Todavía no hay elementos aquí. Guarda con ★ o el botón derecho." : "No se encontraron resultados.";
            }
            catch (OperationCanceledException) { }
            catch (Exception e) when (e is ServiceException or System.Net.Http.HttpRequestException or IOException or ArgumentException or System.Text.Json.JsonException or UnauthorizedAccessException)
            {
                if (Stale(current)) return;
                if (gif && mode == "favorites") Render(history.Data.GifFavorites.Select(v => new PickerCard(v, true, () => Choose(v), () => ToggleFavorite(v), true)).ToArray(), 3);
                else rows.ItemsSource = null;
                note.Text = e is ServiceException service ? service.Message : "No se pudo cargar el catálogo. Puedes volver a abrirlo para intentarlo de nuevo.";
            }
        }
        private bool Stale(CancellationTokenSource request) => closed || request.IsCancellationRequested || !ReferenceEquals(work, request);
        private static bool Matches(PickerValue v, string query) => query.Length == 0 || EmojiCatalog.Normalize(v.Name).Contains(EmojiCatalog.Normalize(query), StringComparison.Ordinal) || v.Value.Contains(query, StringComparison.OrdinalIgnoreCase);
        private void Choose(PickerValue value)
        {
            try { history.Recent(value, gif); } catch (Exception e) when (e is IOException or UnauthorizedAccessException) { }
            insert(value.Value); Close();
        }
        private void ToggleFavorite(PickerValue value)
        {
            try { bool saved = history.FavoriteGif(value); note.Text = saved ? "Guardado en NotDiscord para esta cuenta." : "Quitado de los guardados de NotDiscord."; }
            catch (Exception e) when (e is IOException or UnauthorizedAccessException) { note.Text = "No se pudo guardar la preferencia en disco."; }
        }
        private bool ImportGifLinks()
        {
            bool imported = false;
            var dialog = new Window { Owner = this, Title = "Importar GIF guardados", Width = 540, Height = 460, MinWidth = 430, MinHeight = 350, WindowStartupLocation = WindowStartupLocation.CenterOwner, Background = UiTheme.Panel, Foreground = UiTheme.Ink, FontFamily = FontFamily };
            var layout = new DockPanel { Margin = new Thickness(16) };
            var explanation = new TextBlock { Text = "Pega un enlace por línea o abre un archivo .txt. Se importan como guardados locales; no modifican tus favoritos de Discord ni envían mensajes. Máximo 100 guardados locales.", TextWrapping = TextWrapping.Wrap, Foreground = UiTheme.Muted, Margin = new Thickness(0, 0, 0, 12) }; DockPanel.SetDock(explanation, Dock.Top); layout.Children.Add(explanation);
            var input = new TextBox { AcceptsReturn = true, TextWrapping = TextWrapping.NoWrap, MaxLength = 256 * 1024, VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, Foreground = UiTheme.Ink, Background = UiTheme.Input };
            var buttons = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 12, 0, 0) }; DockPanel.SetDock(buttons, Dock.Bottom); layout.Children.Add(buttons);
            var file = new Button { Content = "Abrir .txt", Margin = new Thickness(0, 0, 8, 0) }; file.Click += (_, _) =>
            {
                var picker = new Microsoft.Win32.OpenFileDialog { Title = "Lista de enlaces GIF", Filter = "Texto (*.txt)|*.txt", CheckFileExists = true };
                if (picker.ShowDialog(dialog) != true) return;
                try { var info = new FileInfo(picker.FileName); if (info.Length > 256 * 1024) throw new ArgumentException("La lista supera 256 KiB."); input.Text = File.ReadAllText(info.FullName); }
                catch (Exception error) when (error is IOException or UnauthorizedAccessException or ArgumentException) { explanation.Text = "No se pudo leer la lista. Usa texto de hasta 256 KiB."; }
            }; buttons.Children.Add(file);
            var accept = new Button { Content = "Importar" }; accept.Click += (_, _) =>
            {
                try { var result = history.ImportGifLinks(input.Text); explanation.Text = $"Importados: {result.Imported}. Ya guardados: {result.Existing}. No admitidos o fuera del límite: {result.Rejected}."; imported |= result.Imported > 0; input.Clear(); }
                catch (Exception error) when (error is IOException or UnauthorizedAccessException or ArgumentException) { explanation.Text = "No se pudo importar. Revisa la lista y el acceso al almacenamiento local."; }
            }; buttons.Children.Add(accept);
            var close = new Button { Content = "Cerrar", Margin = new Thickness(8, 0, 0, 0) }; close.Click += (_, _) => dialog.Close(); buttons.Children.Add(close);
            layout.Children.Add(input); dialog.Content = layout; dialog.Resources[typeof(Button)] = ButtonStyle(); dialog.ShowDialog(); return imported;
        }
        private void Render(IReadOnlyList<PickerCard> values, int columns)
        {
            rows.ItemsSource = values.Chunk(columns).Select(chunk => new PickerRowModel(chunk, columns)).ToArray();
            if (rows.Items.Count > 0) rows.ScrollIntoView(rows.Items[0]);
        }
        private static Style ButtonStyle()
        {
            var style = new Style(typeof(Button)); style.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Surface)); style.Setters.Add(new Setter(Control.ForegroundProperty, UiTheme.Ink)); style.Setters.Add(new Setter(Control.BorderThicknessProperty, new Thickness(0))); style.Setters.Add(new Setter(Control.PaddingProperty, new Thickness(6))); style.Setters.Add(new Setter(Control.CursorProperty, Cursors.Hand));
            var border = new FrameworkElementFactory(typeof(Border)); border.SetBinding(Border.BackgroundProperty, new Binding("Background") { RelativeSource = RelativeSource.TemplatedParent }); border.SetValue(Border.CornerRadiusProperty, new CornerRadius(6)); border.SetBinding(Border.PaddingProperty, new Binding("Padding") { RelativeSource = RelativeSource.TemplatedParent });
            var presenter = new FrameworkElementFactory(typeof(ContentPresenter)); presenter.SetValue(FrameworkElement.HorizontalAlignmentProperty, HorizontalAlignment.Center); presenter.SetValue(FrameworkElement.VerticalAlignmentProperty, VerticalAlignment.Center); border.AppendChild(presenter);
            style.Setters.Add(new Setter(Control.TemplateProperty, new ControlTemplate(typeof(Button)) { VisualTree = border }));
            var hover = new Trigger { Property = UIElement.IsMouseOverProperty, Value = true }; hover.Setters.Add(new Setter(Control.BackgroundProperty, UiTheme.Hover)); style.Triggers.Add(hover); return style;
        }
        private static Style CategoryStyle()
        {
            var style = new Style(typeof(ComboBox));
            var root = new FrameworkElementFactory(typeof(Grid));
            var toggle = new FrameworkElementFactory(typeof(ToggleButton)); toggle.SetValue(Control.FocusableProperty, false);
            toggle.SetBinding(ToggleButton.IsCheckedProperty, new Binding("IsDropDownOpen") { RelativeSource = RelativeSource.TemplatedParent, Mode = BindingMode.TwoWay });
            var frame = new FrameworkElementFactory(typeof(Border)); frame.SetValue(Border.BackgroundProperty, UiTheme.Input); frame.SetValue(Border.BorderBrushProperty, UiTheme.Line); frame.SetValue(Border.BorderThicknessProperty, new Thickness(1)); frame.SetValue(Border.CornerRadiusProperty, new CornerRadius(4)); frame.SetValue(Border.PaddingProperty, new Thickness(8, 6, 8, 6));
            var dock = new FrameworkElementFactory(typeof(DockPanel));
            var arrow = new FrameworkElementFactory(typeof(TextBlock)); arrow.SetValue(TextBlock.TextProperty, "⌄"); arrow.SetValue(TextBlock.ForegroundProperty, UiTheme.Ink); arrow.SetValue(DockPanel.DockProperty, Dock.Right); dock.AppendChild(arrow);
            var label = new FrameworkElementFactory(typeof(TextBlock)); label.SetBinding(TextBlock.TextProperty, new Binding("SelectedItem") { RelativeSource = new RelativeSource(RelativeSourceMode.FindAncestor, typeof(ComboBox), 1) }); label.SetValue(TextBlock.ForegroundProperty, UiTheme.Ink); label.SetValue(TextBlock.TextTrimmingProperty, TextTrimming.CharacterEllipsis); label.SetValue(FrameworkElement.MarginProperty, new Thickness(0, 0, 7, 0)); dock.AppendChild(label); frame.AppendChild(dock);
            toggle.SetValue(Control.TemplateProperty, new ControlTemplate(typeof(ToggleButton)) { VisualTree = frame }); root.AppendChild(toggle);
            var popup = new FrameworkElementFactory(typeof(Popup), "PART_Popup"); popup.SetValue(Popup.PlacementProperty, PlacementMode.Bottom); popup.SetValue(Popup.AllowsTransparencyProperty, true); popup.SetValue(Popup.StaysOpenProperty, false); popup.SetBinding(Popup.IsOpenProperty, new Binding("IsDropDownOpen") { RelativeSource = RelativeSource.TemplatedParent, Mode = BindingMode.TwoWay });
            var popupFrame = new FrameworkElementFactory(typeof(Border)); popupFrame.SetValue(Border.BackgroundProperty, UiTheme.Input); popupFrame.SetValue(Border.BorderBrushProperty, UiTheme.Line); popupFrame.SetValue(Border.BorderThicknessProperty, new Thickness(1)); popupFrame.SetValue(Border.PaddingProperty, new Thickness(2)); popupFrame.SetBinding(FrameworkElement.MinWidthProperty, new Binding("ActualWidth") { RelativeSource = RelativeSource.TemplatedParent });
            var scroll = new FrameworkElementFactory(typeof(ScrollViewer)); scroll.SetValue(FrameworkElement.MaxHeightProperty, 300d); scroll.SetValue(ScrollViewer.VerticalScrollBarVisibilityProperty, ScrollBarVisibility.Auto); scroll.SetValue(ScrollViewer.HorizontalScrollBarVisibilityProperty, ScrollBarVisibility.Disabled); scroll.SetValue(ScrollViewer.CanContentScrollProperty, true); scroll.AppendChild(new FrameworkElementFactory(typeof(ItemsPresenter))); popupFrame.AppendChild(scroll); popup.AppendChild(popupFrame); root.AppendChild(popup);
            style.Setters.Add(new Setter(Control.TemplateProperty, new ControlTemplate(typeof(ComboBox)) { VisualTree = root })); return style;
        }
    }
}

public sealed record PickerCard(PickerValue Value, bool Gif, Action Choose, Action? Favorite, bool Saved);
public sealed record PickerRowModel(IReadOnlyList<PickerCard> Items, int Columns);
public sealed class ContentPickerRow : Grid
{
    public ContentPickerRow() { DataContextChanged += (_, _) => Render(); }
    private void Render()
    {
        Children.Clear(); ColumnDefinitions.Clear();
        if (DataContext is not PickerRowModel row) return;
        for (int i = 0; i < row.Columns; i++) ColumnDefinitions.Add(new());
        Height = row.Items.FirstOrDefault()?.Gif == true || row.Columns == 3 ? 132 : row.Columns == 10 ? 46 : 54;
        for (int i = 0; i < row.Items.Count; i++)
        {
            var item = row.Items[i];
            var button = new Button { Margin = new Thickness(3), ToolTip = item.Value.Name, HorizontalContentAlignment = HorizontalAlignment.Stretch, VerticalContentAlignment = VerticalAlignment.Stretch };
            if (row.Columns == 10) { button.Margin = new Thickness(2); button.Padding = new Thickness(2); }
            if (item.Value.ImageUrl.Length > 0)
            {
                var image = new UiMediaImage(item.Value.ImageUrl, item.Gif || row.Columns == 3 ? 180 : 48) { Stretch = item.Gif || row.Columns == 3 ? Stretch.Uniform : Stretch.Uniform, MaxHeight = item.Gif || row.Columns == 3 ? 104 : row.Columns == 10 ? 28 : 40, MaxWidth = item.Gif || row.Columns == 3 ? 170 : row.Columns == 10 ? 28 : 42 };
                var content = new Grid(); content.Children.Add(new TextBlock { Text = item.Gif ? "GIF" : row.Columns == 3 ? item.Value.Name : ":" + item.Value.Name + ":", Foreground = UiTheme.Muted, FontSize = 11, TextWrapping = TextWrapping.Wrap, HorizontalAlignment = HorizontalAlignment.Center, VerticalAlignment = VerticalAlignment.Center }); content.Children.Add(image);
                image.ImageLoaded += success => { if (success) ((TextBlock)content.Children[0]).Visibility = Visibility.Collapsed; }; button.Content = content;
            }
            else if (!item.Gif && row.Columns != 3) button.Content = new SystemEmojiImage(item.Value.Value, row.Columns == 10 ? 32 : 36);
            else button.Content = new TextBlock { Text = item.Value.Value, FontFamily = new FontFamily("Segoe UI"), FontSize = 14, TextWrapping = TextWrapping.Wrap, Foreground = UiTheme.Ink, TextAlignment = TextAlignment.Center, VerticalAlignment = VerticalAlignment.Center };
            button.Click += (_, _) => item.Choose();
            if (item.Favorite != null)
            {
                var menu = new ContextMenu { Background = UiTheme.Menu, Foreground = UiTheme.Ink }; var favorite = new MenuItem { Header = item.Saved ? "Quitar de favoritos de NotDiscord" : "★ Guardar en NotDiscord" }; favorite.Click += (_, _) => item.Favorite(); menu.Items.Add(favorite); button.ContextMenu = menu;
            }
            var cell = new Grid(); cell.Children.Add(button);
            if (item.Gif && item.Favorite != null)
            {
                var save = new Button { Content = item.Saved ? "★" : "☆", HorizontalAlignment = HorizontalAlignment.Right, VerticalAlignment = VerticalAlignment.Top, Margin = new Thickness(0, 6, 6, 0), Padding = new Thickness(5, 1, 5, 1), FontSize = 19, ToolTip = "Guardar o quitar en NotDiscord" };
                bool saved = item.Saved; save.Click += (_, _) => { item.Favorite(); saved = !saved; save.Content = saved ? "★" : "☆"; }; cell.Children.Add(save);
            }
            SetColumn(cell, i); Children.Add(cell);
        }
    }
}
