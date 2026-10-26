@echo off
setlocal
call "C:\Program Files\Microsoft Visual Studio\2022\Community\Common7\Tools\VsDevCmd.bat" -arch=x64 -host_arch=x64
if errorlevel 1 exit /b 1
cd /d "%~dp0"
if not exist bin mkdir bin
cl.exe /nologo /LD /MT /O2 /EHsc /std:c++17 /W4 /Fo:bin\NotDiscordEmoji.obj NotDiscordEmoji.cpp /link /OUT:bin\NotDiscord.Emoji.dll /IMPLIB:bin\NotDiscord.Emoji.lib d2d1.lib dwrite.lib windowscodecs.lib ole32.lib
exit /b %errorlevel%
