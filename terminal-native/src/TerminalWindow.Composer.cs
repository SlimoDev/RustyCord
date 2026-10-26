using System.Collections.Specialized;
using System.IO;
using System.Text.Json;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Input;
using System.Windows.Media;
using System.Windows.Media.Imaging;
using Microsoft.Win32;

namespace NotDiscord.Terminal;

public sealed partial class TerminalWindow
{
    private readonly ComposerDraftStore composerDrafts = new();
    private readonly WrapPanel stagedAttachmentView = new() { Margin = new Thickness(4, 5, 4, 5) };
    private readonly ScrollViewer stagedAttachmentScroll = new() { MaxHeight = 175, HorizontalScrollBarVisibility = ScrollBarVisibility.Auto, VerticalScrollBarVisibility = ScrollBarVisibility.Disabled, Visibility = Visibility.Collapsed };
    private string composerChannel = "", composerAccount = "";
    private IReadOnlyList<PendingAttachment> stagedAttachments = Array.Empty<PendingAttachment>();
    private bool restoringDraft;
    private string? attachmentSessionFolder;
    private const long MaximumAttachment = 25 * 1024 * 1024, MaximumBatch = 100 * 1024 * 1024;

    private void WireComposerDrafts()
    {
        WireComposerMentions();
        composer.MaxLength = 4000;
        stagedAttachmentScroll.Content = stagedAttachmentView; composerPane.Children.Insert(0, stagedAttachmentScroll);
        composer.TextChanged += (_, _) => { UpdateComposerMentionText(); if (!restoringDraft && !changingComposerMention) StoreActiveDraft(); RefreshComposerSendEligibility(); };
        composer.PreviewKeyDown += (_, e) =>
        {
            // TextBox's standard Paste command may be disabled for a bitmap-only
            // clipboard, so intercept Ctrl+V before relying on its text command.
            if (e.Key != Key.V || (Keyboard.Modifiers & ModifierKeys.Control) == 0 || (Keyboard.Modifiers & ModifierKeys.Alt) != 0) return;
            try
            {
                if (Clipboard.ContainsFileDropList()) { var paths = Clipboard.GetFileDropList().Cast<string>().ToArray(); e.Handled = true; StageFiles(paths); }
                else if (Clipboard.ContainsImage() && Clipboard.GetImage() is BitmapSource bitmap) { e.Handled = true; StageClipboardImage(bitmap); }
                else if (Clipboard.ContainsData("PNG")) { e.Handled = true; StageClipboardPng(Clipboard.GetData("PNG")); }
            }
            catch (Exception error) when (error is System.Runtime.InteropServices.COMException or System.Threading.ThreadStateException or InvalidOperationException or NotSupportedException) { e.Handled = true; status.Text = "No se pudo leer el portapapeles; vuelve a intentar pegar."; }
        };
        DataObject.AddPastingHandler(composer, PasteComposerAttachment);
        composerFrame.AllowDrop = true;
        composerFrame.PreviewDragOver += (_, e) => { if (e.Data.GetDataPresent(DataFormats.FileDrop) || e.Data.GetDataPresent(DataFormats.Bitmap)) { e.Effects = CanStageAttachment() ? DragDropEffects.Copy : DragDropEffects.None; e.Handled = true; } };
        composerFrame.PreviewDrop += (_, e) =>
        {
            if (e.Data.GetDataPresent(DataFormats.FileDrop) && e.Data.GetData(DataFormats.FileDrop) is string[] paths) { StageFiles(paths); e.Handled = true; }
            else if (e.Data.GetDataPresent(DataFormats.Bitmap) && e.Data.GetData(DataFormats.Bitmap) is BitmapSource image) { StageClipboardImage(image); e.Handled = true; }
        };
        var discard = Button("Borrar borrador", ClearCurrentDraft); discard.FontSize = 11; discard.Padding = new Thickness(5, 2, 5, 2); discard.HorizontalAlignment = HorizontalAlignment.Right; discard.ToolTip = "Vaciar el texto, la respuesta y los adjuntos de esta conversación";
        composerPane.Children.Add(discard);
        Closed += (_, _) => { composerDrafts.Clear(); CleanupClipboardSession(); };
    }
    private bool CanStageAttachment() => state.SignedIn && !state.Demo && !state.Busy && state.View == "chat" && state.SelectedChannelId.Length > 0 && state.ChannelAccess.CanSend && state.ChannelAccess.CanAttach;
    private void StoreActiveDraft()
    {
        if (restoringDraft || composerChannel.Length == 0) return;
        try { composerDrafts.Put(composerChannel, composer.Text, replyId, reply.Text, stagedAttachments, composerMentions); }
        catch (InvalidOperationException error) { status.Text = error.Message; }
    }
    private void BeforeApplyComposer(AppSnapshot incoming)
    {
        if (incoming.SelectedChannelId != state.SelectedChannelId || incoming.View != "chat" || !incoming.ChannelAccess.CanSend || !incoming.ChannelAccess.CanView) CloseComposerMentions();
        RememberChatPosition();
        string nextAccount = incoming.CurrentUser?.Id ?? "";
        if (composerAccount.Length > 0 && composerAccount != nextAccount)
        { composerDrafts.Clear(); stagedAttachments = Array.Empty<PendingAttachment>(); composerChannel = ""; CleanupClipboardSession(); LoadActiveDraft(); }
        else StoreActiveDraft();
        composerAccount = nextAccount;
        // The main Apply method derives send/attachment button permissions from
        // this collection, so select the incoming draft before those controls run.
        if (composerChannel != incoming.SelectedChannelId)
        { composerChannel = incoming.SelectedChannelId; LoadActiveDraft(); }
        PrepareChatPositionRestore(incoming);
    }
    private void AfterApplyComposer()
    {
        if (composerChannel != state.SelectedChannelId)
        { composerChannel = state.SelectedChannelId; LoadActiveDraft(); }
        // Removing local prepared files remains possible if permissions change.
        stagedAttachmentView.IsEnabled = state.SignedIn && !state.Demo;
        RefreshComposerSendEligibility();
        if (composerMentionPopup.IsOpen) RefreshComposerMentions();
        RestoreChatPosition();
    }
    private void LoadActiveDraft()
    {
        var draft = composerDrafts.Get(composerChannel);
        restoringDraft = true;
        try
        {
            LoadComposerMentionDraft(draft);
            composer.Text = draft.Text; composer.CaretIndex = composer.Text.Length; replyId = draft.ReplyId; reply.Text = draft.ReplyLabel;
            replyRow.Visibility = replyId.Length > 0 ? Visibility.Visible : Visibility.Collapsed;
            stagedAttachments = draft.Attachments; RenderStagedAttachments();
        }
        finally { restoringDraft = false; }
    }
    private void ChooseComposerFiles()
    {
        if (!CanStageAttachment()) { status.Text = state.ChannelAccess.Reason.Length > 0 ? state.ChannelAccess.Reason : "Este canal no permite enviar archivos."; return; }
        var picker = new OpenFileDialog { Title = "Preparar archivos adjuntos", CheckFileExists = true, Multiselect = true };
        if (picker.ShowDialog(this) == true) StageFiles(picker.FileNames);
    }
    private void StageFiles(IEnumerable<string> paths)
    {
        if (!CanStageAttachment()) { status.Text = "Este canal no permite enviar archivos."; return; }
        var next = stagedAttachments.ToList();
        foreach (string path in paths.Take(11))
        {
            try
            {
                var file = new FileInfo(path);
                if (!file.Exists || (file.Attributes & FileAttributes.Directory) != 0) continue;
                if (next.Any(a => string.Equals(a.Path, file.FullName, StringComparison.OrdinalIgnoreCase))) continue;
                if (file.Length > MaximumAttachment || next.Count >= 10 || next.Sum(a => a.Length) + file.Length > MaximumBatch)
                { status.Text = "Máximo diez adjuntos: 25 MiB por archivo y 100 MiB por mensaje. No se ha enviado nada."; break; }
                next.Add(new(Guid.NewGuid().ToString("N"), file.FullName, file.Name, file.Length));
            }
            catch (Exception error) when (error is IOException or UnauthorizedAccessException or ArgumentException or NotSupportedException) { status.Text = "No se pudo preparar uno de los archivos. No se ha enviado nada."; }
        }
        stagedAttachments = next.ToArray(); StoreActiveDraft(); RenderStagedAttachments();
    }
    private void PasteComposerAttachment(object sender, DataObjectPastingEventArgs e)
    {
        if (e.DataObject.GetDataPresent(DataFormats.FileDrop) && e.DataObject.GetData(DataFormats.FileDrop) is string[] paths)
        { e.CancelCommand(); StageFiles(paths); return; }
        if (e.DataObject.GetDataPresent(DataFormats.Bitmap) && e.DataObject.GetData(DataFormats.Bitmap) is BitmapSource source)
        { e.CancelCommand(); StageClipboardImage(source); return; }
        // Some clipboard image providers expose PNG but not the WPF Bitmap format.
        if (e.DataObject.GetDataPresent("PNG"))
        {
            e.CancelCommand(); StageClipboardPng(e.DataObject.GetData("PNG"));
        }
    }
    private void StageClipboardPng(object? data)
    {
        if (!CanStageAttachment()) { status.Text = "Este canal no permite enviar archivos."; return; }
        try
        {
            if (data is not Stream stream || !stream.CanSeek || stream.Length > MaximumAttachment) { status.Text = "No se pudo leer la imagen PNG del portapapeles."; return; }
            stream.Position = 0; var header = BitmapDecoder.Create(stream, BitmapCreateOptions.DelayCreation | BitmapCreateOptions.IgnoreColorProfile, BitmapCacheOption.None).Frames.FirstOrDefault();
            if (header == null || header.PixelWidth > 8192 || header.PixelHeight > 8192 || (long)header.PixelWidth * header.PixelHeight > 24_000_000) { status.Text = "La imagen del portapapeles es demasiado grande."; return; }
            stream.Position = 0; var frame = BitmapDecoder.Create(stream, BitmapCreateOptions.IgnoreColorProfile, BitmapCacheOption.OnLoad).Frames.FirstOrDefault(); if (frame != null) StageClipboardImage(frame);
        }
        catch (Exception error) when (error is IOException or NotSupportedException or FileFormatException or ArgumentException) { status.Text = "No se pudo leer la imagen del portapapeles."; }
    }
    private void StageClipboardImage(BitmapSource source)
    {
        if (!CanStageAttachment()) { status.Text = "Este canal no permite enviar archivos."; return; }
        if ((long)source.PixelWidth * source.PixelHeight > 24_000_000 || source.PixelWidth > 8192 || source.PixelHeight > 8192 || stagedAttachments.Count >= 10)
        { status.Text = "La imagen del portapapeles es demasiado grande o ya hay diez adjuntos."; return; }
        try
        {
            attachmentSessionFolder ??= Path.Combine(Path.GetDirectoryName(preferencePath)!, "session-attachments", Guid.NewGuid().ToString("N"));
            Directory.CreateDirectory(attachmentSessionFolder);
            string path = Path.Combine(attachmentSessionFolder, Guid.NewGuid().ToString("N") + ".png");
            var encoder = new PngBitmapEncoder(); encoder.Frames.Add(BitmapFrame.Create(source)); using (var output = File.Create(path)) encoder.Save(output);
            var file = new FileInfo(path);
            if (file.Length > MaximumAttachment || stagedAttachments.Sum(a => a.Length) + file.Length > MaximumBatch) { File.Delete(path); status.Text = "La imagen supera el límite de tamaño del mensaje."; return; }
            stagedAttachments = stagedAttachments.Append(new PendingAttachment(Guid.NewGuid().ToString("N"), path, "Imagen pegada.png", file.Length, true)).ToArray();
            StoreActiveDraft(); RenderStagedAttachments();
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or NotSupportedException) { status.Text = "No se pudo preparar la imagen del portapapeles."; }
    }
    private void RenderStagedAttachments()
    {
        stagedAttachmentView.Children.Clear(); stagedAttachmentScroll.Visibility = stagedAttachments.Count == 0 ? Visibility.Collapsed : Visibility.Visible;
        foreach (var file in stagedAttachments)
        {
            var card = new Border { Width = 155, Background = UiTheme.Panel, BorderBrush = line, BorderThickness = new Thickness(1), CornerRadius = new CornerRadius(6), Padding = new Thickness(7), Margin = new Thickness(3) };
            var column = new StackPanel();
            var preview = ReadLocalThumbnail(file.Path);
            if (preview != null) column.Children.Add(new Image { Source = preview, Width = 138, Height = 76, Stretch = Stretch.Uniform });
            var name = Text(file.Name, 12); name.TextTrimming = TextTrimming.CharacterEllipsis; name.ToolTip = file.Name; column.Children.Add(name);
            column.Children.Add(Text((file.Length / 1024d).ToString("N0") + " KiB", 11, Muted));
            var remove = Button("Quitar", () => RemoveStaged(file.Id)); remove.FontSize = 11; remove.Padding = new Thickness(4, 2, 4, 2); column.Children.Add(remove); card.Child = column; stagedAttachmentView.Children.Add(card);
        }
        RefreshComposerSendEligibility();
    }
    private void RefreshComposerSendEligibility()
        => send.IsEnabled = state.SignedIn && !state.Demo && !state.Busy && state.View == "chat" && state.ChannelAccess.CanSend &&
            (!string.IsNullOrWhiteSpace(composer.Text) || stagedAttachments.Count > 0) && (stagedAttachments.Count == 0 || state.ChannelAccess.CanAttach);
    private static BitmapSource? ReadLocalThumbnail(string path)
    {
        if (!new[] { ".png", ".jpg", ".jpeg", ".gif", ".bmp", ".webp" }.Contains(Path.GetExtension(path).ToLowerInvariant())) return null;
        try
        {
            using var input = File.OpenRead(path);
            var header = BitmapDecoder.Create(input, BitmapCreateOptions.DelayCreation | BitmapCreateOptions.IgnoreColorProfile, BitmapCacheOption.None);
            if (header.Frames.Count == 0 || (long)header.Frames[0].PixelWidth * header.Frames[0].PixelHeight > 24_000_000) return null;
            input.Position = 0; var image = new BitmapImage(); image.BeginInit(); image.CacheOption = BitmapCacheOption.OnLoad; image.DecodePixelWidth = 180; image.StreamSource = input; image.EndInit(); image.Freeze(); return image;
        }
        catch (Exception error) when (error is IOException or UnauthorizedAccessException or NotSupportedException or ArgumentException or FileFormatException) { return null; }
    }
    private void RemoveStaged(string id)
    { var removed = stagedAttachments.FirstOrDefault(a => a.Id == id); stagedAttachments = stagedAttachments.Where(a => a.Id != id).ToArray(); DeleteTemporary(removed); StoreActiveDraft(); RenderStagedAttachments(); }
    private void ClearCurrentDraft()
    {
        foreach (var file in composerDrafts.Remove(composerChannel).Attachments) DeleteTemporary(file);
        restoringDraft = true;
        try { LoadComposerMentionDraft(ComposerDraft.Empty); composer.Clear(); replyId = ""; reply.Text = ""; replyRow.Visibility = Visibility.Collapsed; stagedAttachments = Array.Empty<PendingAttachment>(); RenderStagedAttachments(); }
        finally { restoringDraft = false; }
    }
    private void SendPreparedMessage()
    {
        if (!state.SignedIn || state.Demo || state.Busy || state.View != "chat" || state.SelectedChannelId.Length == 0) return;
        if (!state.ChannelAccess.CanSend || stagedAttachments.Count > 0 && !state.ChannelAccess.CanAttach) { status.Text = state.ChannelAccess.Reason.Length > 0 ? state.ChannelAccess.Reason : "No tienes permiso para enviar este contenido."; return; }
        StoreActiveDraft();
        try
        {
            string content = composerDrafts.TransportText(composerChannel);
            var prepared = composerDrafts.Submit(composerChannel);
            CloseComposerMentions();
            Handle(new("send-prepared", Text: content, Extra: JsonSerializer.Serialize(prepared), ChannelId: composerChannel));
        }
        catch (InvalidOperationException error) { status.Text = error.Message; }
    }
    public void AcknowledgePreparedSend(string channelId, string sentText, string replyMessageId, string submissionId)
    {
        if (!Dispatcher.CheckAccess()) { Dispatcher.Invoke(() => AcknowledgePreparedSend(channelId, sentText, replyMessageId, submissionId)); return; }
        StoreActiveDraft();
        foreach (var file in composerDrafts.Acknowledge(channelId, sentText, replyMessageId, submissionId)) DeleteTemporary(file);
        if (composerChannel == channelId) LoadActiveDraft();
    }
    private static void DeleteTemporary(PendingAttachment? file) { if (file?.Temporary == true) try { File.Delete(file.Path); } catch (Exception error) when (error is IOException or UnauthorizedAccessException) { } }
    private void CleanupClipboardSession()
    {
        if (attachmentSessionFolder == null) return;
        // The directory was created from our private random session name; never delete a user-selected path.
        string root = Path.GetFullPath(Path.Combine(Path.GetDirectoryName(preferencePath)!, "session-attachments")) + Path.DirectorySeparatorChar;
        string target = Path.GetFullPath(attachmentSessionFolder);
        if (target.StartsWith(root, StringComparison.OrdinalIgnoreCase)) try { Directory.Delete(target, recursive: true); } catch (Exception error) when (error is IOException or UnauthorizedAccessException) { }
        attachmentSessionFolder = null;
    }
}
