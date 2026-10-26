param([Parameter(Mandatory=$true)][int]$DiscordRootProcessId,[Parameter(Mandatory=$true)][int]$NotDiscordRootProcessId)
Set-StrictMode -Version Latest
$ErrorActionPreference='Stop'
if (-not ('NotDiscord.Layout.Native' -as [type])) {
Add-Type -TypeDefinition @"
using System;
using System.Runtime.InteropServices;
namespace NotDiscord.Layout {
 public static class Native {
  [StructLayout(LayoutKind.Sequential)] public struct Rect { public int Left,Top,Right,Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h,out Rect rect);
  [DllImport("user32.dll")] public static extern bool IsIconic(IntPtr h);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
 }
}
"@
}
foreach ($taskPair in @(@{Client='Discord';Id=$DiscordRootProcessId},@{Client='NotDiscord';Id=$NotDiscordRootProcessId})) {
 $taskProcess=Get-Process -Id $taskPair.Id
 $taskHandle=$taskProcess.MainWindowHandle
 $taskRect=[NotDiscord.Layout.Native+Rect]::new()
 $taskSuccess=[NotDiscord.Layout.Native]::GetWindowRect($taskHandle,[ref]$taskRect)
 [pscustomobject]@{Client=$taskPair.Client;Pid=$taskPair.Id;Utc=(Get-Date).ToUniversalTime().ToString('o');Valid=$taskSuccess;Visible=[NotDiscord.Layout.Native]::IsWindowVisible($taskHandle);Minimized=[NotDiscord.Layout.Native]::IsIconic($taskHandle);Left=$taskRect.Left;Top=$taskRect.Top;Right=$taskRect.Right;Bottom=$taskRect.Bottom}
}
