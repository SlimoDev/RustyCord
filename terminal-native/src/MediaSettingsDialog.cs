using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Markup;

namespace NotDiscord.Terminal;

public sealed class MediaSettingsDialog : Window
{
    private readonly ComboBox input, output, camera, key;
    private readonly Slider inputVolume, outputVolume;
    private readonly CheckBox noise, echo, ptt;
    private readonly Button micTest = null!, cameraTest = null!, apply = null!;
    private readonly ProgressBar meter;
    private readonly TextBlock feedback = null!;
    private readonly Func<MediaPreferences, bool, Task> applySettings;
    private readonly Func<bool, Task> toggleMicTest;
    private readonly Func<MediaPreferences, Task> toggleCamera;
    private readonly bool canTestMic, canTestVideo;
    private readonly SemaphoreSlim operations = new(1, 1);
    private bool micActive, busy, closing, stopped;
    private readonly MediaPreferences original;
    public MediaSettingsDialog(Window owner, MediaPreferences preferences, MediaDeviceInventory devices,
        Func<MediaPreferences, bool, Task> applySettings, Func<bool, Task> toggleMicTest, Func<MediaPreferences, Task> toggleCamera,
        bool canTestMicrophone, bool canTestCamera, string focus = "")
    {
        Owner = owner; Title = "NotDiscord · Voz y vídeo"; Width = 760; Height = 740; MinWidth = 580; MinHeight = 530;
        WindowStartupLocation = WindowStartupLocation.CenterOwner; Background = UiTheme.Background; Foreground = UiTheme.Ink;
        FontFamily = owner.FontFamily; FontSize = 14;
        foreach (var resourceKey in owner.Resources.Keys) Resources[resourceKey] = owner.Resources[resourceKey];
        Resources["MediaInput"] = UiTheme.Input; Resources["MediaInk"] = UiTheme.Ink; Resources["MediaLine"] = UiTheme.Line; Resources["MediaHover"] = UiTheme.Hover;
        Resources[typeof(ComboBox)] = XamlReader.Parse("""
            <Style xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" xmlns:x="http://schemas.microsoft.com/winfx/2006/xaml" TargetType="ComboBox">
              <Setter Property="Background" Value="{DynamicResource MediaInput}"/><Setter Property="Foreground" Value="{DynamicResource MediaInk}"/><Setter Property="BorderBrush" Value="{DynamicResource MediaLine}"/>
              <Setter Property="Template"><Setter.Value><ControlTemplate TargetType="ComboBox">
                <Grid><ToggleButton Focusable="False" Background="{TemplateBinding Background}" BorderBrush="{TemplateBinding BorderBrush}" Foreground="{TemplateBinding Foreground}" IsChecked="{Binding IsDropDownOpen, RelativeSource={RelativeSource TemplatedParent}, Mode=TwoWay}">
                  <ToggleButton.Template><ControlTemplate TargetType="ToggleButton"><Border Background="{TemplateBinding Background}" BorderBrush="{TemplateBinding BorderBrush}" BorderThickness="1" CornerRadius="5" Padding="10,8"><Grid><Grid.ColumnDefinitions><ColumnDefinition/><ColumnDefinition Width="22"/></Grid.ColumnDefinitions><ContentPresenter Content="{Binding SelectionBoxItem, RelativeSource={RelativeSource AncestorType=ComboBox}}" VerticalAlignment="Center"/><TextBlock Grid.Column="1" Text="⌄" VerticalAlignment="Center" HorizontalAlignment="Right" Foreground="{TemplateBinding Foreground}"/></Grid></Border></ControlTemplate></ToggleButton.Template>
                </ToggleButton><Popup Name="PART_Popup" IsOpen="{TemplateBinding IsDropDownOpen}" Placement="Bottom" AllowsTransparency="True" Focusable="False" PopupAnimation="None"><Border Background="{DynamicResource MediaInput}" BorderBrush="{DynamicResource MediaLine}" BorderThickness="1" MinWidth="{Binding ActualWidth, RelativeSource={RelativeSource TemplatedParent}}" MaxHeight="260"><ScrollViewer CanContentScroll="True"><ItemsPresenter/></ScrollViewer></Border></Popup></Grid>
              </ControlTemplate></Setter.Value></Setter>
            </Style>
            """);
        Resources[typeof(ComboBoxItem)] = XamlReader.Parse("""
            <Style xmlns="http://schemas.microsoft.com/winfx/2006/xaml/presentation" TargetType="ComboBoxItem"><Setter Property="Foreground" Value="{DynamicResource MediaInk}"/><Setter Property="Padding" Value="10,8"/><Setter Property="Template"><Setter.Value><ControlTemplate TargetType="ComboBoxItem"><Border Name="Row" Background="Transparent" Padding="{TemplateBinding Padding}"><ContentPresenter/></Border><ControlTemplate.Triggers><Trigger Property="IsHighlighted" Value="True"><Setter TargetName="Row" Property="Background" Value="{DynamicResource MediaHover}"/></Trigger></ControlTemplate.Triggers></ControlTemplate></Setter.Value></Setter></Style>
            """);
        this.original = preferences; this.applySettings = applySettings; this.toggleMicTest = toggleMicTest; this.toggleCamera = toggleCamera;
        canTestMic = canTestMicrophone; canTestVideo = canTestCamera && devices.Cameras.Count > 0;
        var layout = new DockPanel { Margin = new Thickness(24) }; Content = layout;
        var footer = new StackPanel { Orientation = Orientation.Horizontal, HorizontalAlignment = HorizontalAlignment.Right, Margin = new Thickness(0, 16, 0, 0) };
        DockPanel.SetDock(footer, Dock.Bottom); layout.Children.Add(footer);
        footer.Children.Add(TerminalWindow.Button("Cerrar", Close));
        apply = TerminalWindow.Button("Aplicar cambios", () => _ = Run(async () => { await StopTest(); await applySettings(Read(), true); feedback.Text = "Preferencias aplicadas y guardadas."; }));
        apply.Padding = new Thickness(16, 8, 16, 8); footer.Children.Add(apply);
        var scroll = new ScrollViewer { VerticalScrollBarVisibility = ScrollBarVisibility.Auto, HorizontalScrollBarVisibility = ScrollBarVisibility.Disabled }; layout.Children.Add(scroll);
        var panel = new StackPanel(); scroll.Content = panel;
        panel.Children.Add(Text("Voz y vídeo", 25));
        panel.Children.Add(Text("Los dispositivos se enumeran al abrir esta ventana. La captura empieza al llamar o pulsar una prueba.", 12, true));
        var devicesRow = new Grid { Margin = new Thickness(0, 16, 0, 0) }; devicesRow.ColumnDefinitions.Add(new()); devicesRow.ColumnDefinitions.Add(new()); panel.Children.Add(devicesRow);
        var left = new StackPanel { Margin = new Thickness(0, 0, 10, 0) }; var right = new StackPanel { Margin = new Thickness(10, 0, 0, 0) }; Grid.SetColumn(right, 1); devicesRow.Children.Add(left); devicesRow.Children.Add(right);
        left.Children.Add(Text("Micrófono")); input = DeviceBox(devices.Input, preferences.InputId, true); left.Children.Add(input);
        right.Children.Add(Text("Salida de audio")); output = DeviceBox(devices.Output, preferences.OutputId, true); right.Children.Add(output);
        inputVolume = Volume(left, "Volumen del micrófono", 200, preferences.InputVolume); outputVolume = Volume(right, "Volumen de salida", 400, preferences.OutputVolume);
        panel.Children.Add(Text("Por encima de 100 % puede saturar. Los dispositivos retirados usan el predeterminado de Windows.", 12, true));
        noise = Check("Supresión de ruido estándar", preferences.NoiseSuppression); echo = Check("Cancelación de eco", preferences.EchoCancellation);
        panel.Children.Add(noise); panel.Children.Add(echo);
        ptt = Check("Pulsar para hablar", preferences.PushToTalk); panel.Children.Add(ptt);
        key = new ComboBox { ItemsSource = MediaPreferences.PttKeys.Select(k => k.Label).ToArray(), SelectedIndex = Array.FindIndex(MediaPreferences.PttKeys, k => k.Key == preferences.PttVirtualKey), Margin = new Thickness(0, 0, 0, 8), Width = 200, HorizontalAlignment = HorizontalAlignment.Left, IsEnabled = ptt.IsChecked == true };
        ptt.Checked += (_, _) => key.IsEnabled = true; ptt.Unchecked += (_, _) => key.IsEnabled = false; panel.Children.Add(key);
        panel.Children.Add(Text("La tecla funciona también fuera de NotDiscord durante la llamada. Soltarla detiene la transmisión; silenciar prevalece sobre PTT.", 12, true));
        micTest = TerminalWindow.Button("Probar micrófono · escucharme", () => _ = Run(async () =>
        {
            if (micActive) { await StopTest(); return; }
            await applySettings(Read(), false); await toggleMicTest(true); micActive = true; micTest.Content = "Detener prueba";
            feedback.Text = "Prueba local activa. Escucharás tu micrófono; se detiene en 30 segundos o al cerrar.";
        }));
        micTest.IsEnabled = canTestMicrophone; micTest.Margin = new Thickness(0, 12, 0, 6); panel.Children.Add(micTest);
        meter = new ProgressBar { Minimum = 0, Maximum = 100, Height = 8, Foreground = UiTheme.AccentSurface, Background = UiTheme.Input, Margin = new Thickness(0, 0, 0, 8) }; panel.Children.Add(meter);
        if (!canTestMicrophone) panel.Children.Add(Text("Desconecta la llamada para probar el micrófono localmente.", 12, true));
        panel.Children.Add(new Separator { Background = UiTheme.Line, Margin = new Thickness(0, 14, 0, 14) });
        panel.Children.Add(Text("Cámara", 20)); camera = DeviceBox(devices.Cameras, preferences.CameraId, false); panel.Children.Add(camera);
        panel.Children.Add(Text("La selección no abre la cámara. La prueba saliente necesita una llamada y transmite a sus participantes. La previsualización local y el vídeo recibido siguen pendientes.", 12, true));
        cameraTest = TerminalWindow.Button("Activar / detener vídeo en la llamada", () => _ = Run(async () => { await StopTest(); await toggleCamera(Read()); feedback.Text = "Vídeo actualizado. Comprueba la imagen desde el receptor."; }));
        cameraTest.IsEnabled = canTestVideo; cameraTest.Margin = new Thickness(0, 8, 0, 8); panel.Children.Add(cameraTest);
        feedback = Text("", 12, true); panel.Children.Add(feedback);
        Loaded += (_, _) => (focus == "output" ? output : focus == "camera" ? camera : input).Focus();
        Closing += OnClosing;
    }
    public void UpdateMicTest(bool active, double level)
    {
        micActive = active; meter.Value = Math.Clamp(level * 100, 0, 100); micTest.Content = active ? "Detener prueba" : "Probar micrófono · escucharme";
        if (!active) feedback.Text = "Prueba local detenida.";
    }
    private async Task Run(Func<Task> work)
    {
        if (busy || closing) return; busy = true; await operations.WaitAsync(); apply.IsEnabled = micTest.IsEnabled = cameraTest.IsEnabled = false;
        try { await work(); }
        catch (Exception error) { feedback.Text = error is ServiceException ? error.Message : "No se pudo aplicar el cambio de dispositivo. Vuelve a abrir los ajustes."; }
        finally { busy = false; operations.Release(); if (!closing) { apply.IsEnabled = true; micTest.IsEnabled = canTestMic; cameraTest.IsEnabled = canTestVideo; } }
    }
    private async Task StopTest() { await toggleMicTest(false); micActive = false; meter.Value = 0; micTest.Content = "Probar micrófono · escucharme"; }
    private async void OnClosing(object? sender, CancelEventArgs e)
    {
        if (stopped) return; e.Cancel = true; if (closing) return; closing = true;
        await operations.WaitAsync();
        try { await toggleMicTest(false); }
        catch { }
        finally { operations.Release(); stopped = true; _ = Dispatcher.BeginInvoke(new Action(Close)); }
    }
    private MediaPreferences Read() => (original with
    {
        InputId = Selected(input, "default"), OutputId = Selected(output, "default"), CameraId = Selected(camera, ""),
        InputVolume = (int)Math.Round(inputVolume.Value), OutputVolume = (int)Math.Round(outputVolume.Value),
        NoiseSuppression = noise.IsChecked == true, EchoCancellation = echo.IsChecked == true, PushToTalk = ptt.IsChecked == true,
        PttVirtualKey = MediaPreferences.PttKeys[Math.Clamp(key.SelectedIndex, 0, MediaPreferences.PttKeys.Length - 1)].Key
    }).Normalize();
    private static string Selected(ComboBox box, string fallback) => (box.SelectedItem as ComboBoxItem)?.Tag as string ?? fallback;
    private static ComboBox DeviceBox(IReadOnlyList<MediaDevice> devices, string selected, bool withDefault)
    {
        var box = new ComboBox { Margin = new Thickness(0, 4, 0, 10), MinHeight = 34, HorizontalContentAlignment = HorizontalAlignment.Stretch, Background = UiTheme.Input, Foreground = UiTheme.Ink, BorderBrush = UiTheme.Line };
        if (withDefault) box.Items.Add(new ComboBoxItem { Content = "Predeterminado de Windows", Tag = "default" });
        foreach (var device in devices.Where(d => !withDefault || d.Id != "default").Take(128)) box.Items.Add(new ComboBoxItem { Content = device.Name, Tag = device.Id });
        box.SelectedItem = box.Items.OfType<ComboBoxItem>().FirstOrDefault(i => (string?)i.Tag == selected) ?? box.Items.OfType<ComboBoxItem>().FirstOrDefault();
        return box;
    }
    private static CheckBox Check(string text, bool value) => new() { Content = text, IsChecked = value, Foreground = UiTheme.Ink, Margin = new Thickness(0, 10, 0, 4), FontSize = 14 };
    private static TextBlock Text(string text, double size = 14, bool muted = false) => new() { Text = text, FontSize = size, Foreground = muted ? UiTheme.Muted : UiTheme.Ink, TextWrapping = TextWrapping.Wrap, Margin = new Thickness(0, 6, 0, 6) };
    private static Slider Volume(Panel panel, string label, double maximum, double value)
    {
        var title = Text($"{label} · {value:0} %", 12); panel.Children.Add(title);
        var slider = new Slider { Minimum = 0, Maximum = maximum, Value = value, TickFrequency = 5, Margin = new Thickness(0, 0, 0, 10) };
        slider.ValueChanged += (_, _) => title.Text = $"{label} · {slider.Value:0} %"; panel.Children.Add(slider); return slider;
    }
}
