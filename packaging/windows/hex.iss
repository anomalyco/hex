; Per-user Windows installer for Hex. It installs exactly the managed layout
; that .github/release/install-windows.ps1 creates and src/windows_updater.rs updates:
;
;   %APPDATA%\hex\versions\<version>\hex.exe
;   %APPDATA%\hex\versions\<version>\hex.com  (the console entry point)
;   %APPDATA%\hex\bin\hex.com  (on the user's PATH)
;   %APPDATA%\hex\current-version
;   %APPDATA%\hex\install.json  (the direct-install receipt)
;   Start Menu\Programs\Hex.lnk  (description "Hex managed install")
;
; so the app keeps updating itself from the release service afterwards. The
; uninstaller lives outside versions\ because the updater prunes old version
; directories. Settings, logs, history, and models stay on uninstall, as with
; install-windows.ps1 -Uninstall.
;
; Compile with scripts/build-windows-installer.ps1, which passes:
;   /DAppVersion=2.3.0 /DArch=x64|arm64 /DExecutable=<hex.exe> /DConsole=<hex.com>
;   /DOutputDir=<dir>
; Console is left out only to wrap a release published before hex.com.

#ifndef AppVersion
  #error Pass /DAppVersion=<version>
#endif
#ifndef Arch
  #error Pass /DArch=x64 or /DArch=arm64
#endif
#ifndef Executable
  #error Pass /DExecutable=<path to hex.exe>
#endif
#ifndef OutputDir
  #define OutputDir "."
#endif

#if Arch == "x64"
  #define AllowedArchitectures "x64os"
#elif Arch == "arm64"
  #define AllowedArchitectures "arm64"
#else
  #error Arch must be x64 or arm64
#endif

[Setup]
; Keep this AppId forever: it names the Apps & Features entry that
; src/windows_updater.rs keeps current after self-updates.
AppId={{6ED48C92-5177-41F4-B8D6-37B7B02CFDF4}
AppName=Hex
AppVersion={#AppVersion}
AppVerName=Hex {#AppVersion}
AppPublisher=Anomaly Innovations, Inc.
AppPublisherURL=https://anoma.ly/
AppSupportURL=https://github.com/anomalyco/hex/issues
AppUpdatesURL=https://github.com/anomalyco/hex/releases
PrivilegesRequired=lowest
ArchitecturesAllowed={#AllowedArchitectures}
ArchitecturesInstallIn64BitMode={#AllowedArchitectures}
DefaultDirName={userappdata}\hex
DisableDirPage=yes
UsePreviousAppDir=no
DisableProgramGroupPage=yes
DisableReadyPage=yes
DisableWelcomePage=yes
UninstallFilesDir={app}\uninstall
UninstallDisplayName=Hex
UninstallDisplayIcon={app}\uninstall\hex.ico
SetupIconFile=..\..\resources\windows\hex.ico
WizardStyle=modern
Compression=lzma2/max
SolidCompression=yes
OutputDir={#OutputDir}
OutputBaseFilename=hex-setup-{#AppVersion}-windows-{#Arch}
CloseApplications=yes
RestartApplications=no
; Adding or removing bin on PATH reaches new terminals without signing out.
ChangesEnvironment=yes
MinVersion=10.0

[Files]
Source: "{#Executable}"; DestDir: "{app}\versions\{#AppVersion}"; DestName: "hex.exe"; Flags: ignoreversion
#ifdef Console
Source: "{#Console}"; DestDir: "{app}\versions\{#AppVersion}"; DestName: "hex.com"; Flags: ignoreversion
Source: "{#Console}"; DestDir: "{app}\bin"; DestName: "hex.com"; Flags: ignoreversion
#endif
Source: "..\..\resources\windows\hex.ico"; DestDir: "{app}\uninstall"; Flags: ignoreversion

[Icons]
Name: "{userprograms}\Hex"; Filename: "{app}\versions\{#AppVersion}\hex.exe"; Parameters: "app"; WorkingDir: "{app}\versions\{#AppVersion}"; Comment: "Hex managed install"

[Run]
Filename: "{app}\versions\{#AppVersion}\hex.exe"; Parameters: "app"; Description: "Open Hex"; Flags: postinstall nowait skipifsilent

[UninstallDelete]
Type: filesandordirs; Name: "{app}\versions"
Type: filesandordirs; Name: "{app}\updates"
Type: filesandordirs; Name: "{app}\bin"
Type: files; Name: "{app}\current-version"
Type: files; Name: "{app}\current-version.partial"
Type: files; Name: "{app}\install.json"

[Code]
function InitializeSetup(): Boolean;
begin
  Result := True;
  { Installs this version whatever is installed now: versions are never
    compared, and the self-updater follows the channel's current release. }
end;

procedure StopManagedHex();
var
  ResultCode: Integer;
  Versions, Bin: String;
begin
  Versions := ExpandConstant('{app}\versions\');
  StringChangeEx(Versions, '''', '''''', True);
  Bin := ExpandConstant('{app}\bin\');
  StringChangeEx(Bin, '''', '''''', True);
  Exec(ExpandConstant('{sys}\WindowsPowerShell\v1.0\powershell.exe'),
    '-NoProfile -NonInteractive -WindowStyle Hidden -Command "Get-Process hex -ErrorAction SilentlyContinue | ' +
    'Where-Object { $_.Path -and ($_.Path.StartsWith(''' + Versions + ''', [StringComparison]::OrdinalIgnoreCase) -or ' +
    '$_.Path.StartsWith(''' + Bin + ''', [StringComparison]::OrdinalIgnoreCase)) } | ' +
    'Stop-Process -Force -ErrorAction SilentlyContinue; Start-Sleep -Milliseconds 500"',
    '', SW_HIDE, ewWaitUntilTerminated, ResultCode);
end;

{ An earlier version left running would hold the instance lock. }
function PrepareToInstall(var NeedsRestart: Boolean): String;
begin
  StopManagedHex();
  Result := '';
end;

{ Replaces Path with Contents through a .partial file. }
procedure WriteAtomically(Path, Contents: String);
var
  Partial: String;
begin
  Partial := Path + '.partial';
  DeleteFile(Partial);
  if not SaveStringToFile(Partial, Contents, False) then
    RaiseException('Could not write ' + Partial);
  DeleteFile(Path);
  if not RenameFile(Partial, Path) then
    RaiseException('Could not write ' + Path);
end;

{ The same as UTF-8 without a byte order mark: SaveStringToFile writes the
  ANSI code page, which turns a non-ASCII profile path in install.json into
  bytes Hex can't read, so the install stopped updating itself (#1352). }
procedure WriteUtf8Atomically(Path, Contents: String);
var
  Partial: String;
  Lines: TArrayOfString;
begin
  Partial := Path + '.partial';
  DeleteFile(Partial);
  SetArrayLength(Lines, 1);
  Lines[0] := Contents;
  if not SaveStringsToUTF8FileWithoutBOM(Partial, Lines, False) then
    RaiseException('Could not write ' + Partial);
  DeleteFile(Path);
  if not RenameFile(Partial, Path) then
    RaiseException('Could not write ' + Path);
end;

{ Path's entries compared case-insensitively, ignoring a trailing backslash. }
function NormalPath(Path: String): String;
begin
  Result := ';' + AnsiLowercase(Path) + ';';
  StringChangeEx(Result, '\;', ';', True);
end;

function PathHas(Path, Folder: String): Boolean;
begin
  Result := Pos(';' + AnsiLowercase(Folder) + ';', NormalPath(Path)) > 0;
end;

{ Puts bin, where hex.com lives, on the user's PATH, so a terminal runs `hex`
  as the console program that waits for hex.exe. }
procedure AddBinToPath();
var
  Path, Bin: String;
begin
  Bin := ExpandConstant('{app}\bin');
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
    Path := '';
  if PathHas(Path, Bin) then
    exit;
  if (Path <> '') and (Copy(Path, Length(Path), 1) <> ';') then
    Path := Path + ';';
  RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Path + Bin);
end;

procedure RemoveBinFromPath();
var
  Path, Bin, Entry, Kept: String;
  At: Integer;
begin
  Bin := ExpandConstant('{app}\bin');
  if not RegQueryStringValue(HKCU, 'Environment', 'Path', Path) then
    exit;
  if not PathHas(Path, Bin) then
    exit;
  { Rebuilt without the entry; the comparison form loses the original case. }
  Path := Path + ';';
  Kept := '';
  while Path <> '' do
  begin
    At := Pos(';', Path);
    Entry := Copy(Path, 1, At - 1);
    Delete(Path, 1, At);
    if (Entry <> '') and not PathHas(Entry, Bin) then
    begin
      if Kept <> '' then
        Kept := Kept + ';';
      Kept := Kept + Entry;
    end;
  end;
  RegWriteExpandStringValue(HKCU, 'Environment', 'Path', Kept);
end;

{ Point current-version at this version only once its files are in place,
  then mark the install as direct so the app updates itself
  (docs/plans/release-service.md). }
procedure CurStepChanged(CurStep: TSetupStep);
var
  Root: String;
begin
  if CurStep = ssPostInstall then
  begin
    WriteAtomically(ExpandConstant('{app}\current-version'), '{#AppVersion}' + #10);
    Root := ExpandConstant('{app}');
    StringChangeEx(Root, '\', '\\', True);
    WriteUtf8Atomically(ExpandConstant('{app}\install.json'),
      '{"app":"hex","artifact":"desktop","channel":"stable","method":"direct","root":"' + Root + '"}');
#ifdef Console
    AddBinToPath();
#endif
  end;
end;

procedure CurUninstallStepChanged(CurUninstallStep: TUninstallStep);
var
  Command: String;
begin
  if CurUninstallStep = usUninstall then
  begin
    { Self-updates run from newer version directories this uninstaller did
      not create, so stop any Hex running from the managed layout. }
    StopManagedHex();
    if RegQueryStringValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Hex', Command) and
       (Pos(LowerCase(ExpandConstant('{app}\versions\')), LowerCase(Command)) > 0) then
      RegDeleteValue(HKCU, 'Software\Microsoft\Windows\CurrentVersion\Run', 'Hex');
    RemoveBinFromPath();
  end;
end;
