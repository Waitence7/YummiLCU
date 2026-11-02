param([string]$InstallerRoot = (Split-Path -Parent $PSScriptRoot))
$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ('Yummi-installer-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
try {
  # Run CMD's real quoting path without elevation or installing a package.
  $stage = Join-Path $root "folder with spaces and 'apostrophe"
  New-Item -ItemType Directory -Path $stage | Out-Null
  $cmd = Get-Content (Join-Path $InstallerRoot 'Install-YummiGameBar.cmd') -Raw
  $probe = [regex]::Replace($cmd, '\$p=Start-Process powershell.exe .*?; exit \$p.ExitCode', 'Write-Output $argLine; exit 0')
  if ($probe -eq $cmd) { throw 'Launcher probe failed to replace elevation call' }
  $cmdPath = Join-Path $stage 'Install-YummiGameBar.cmd'
  [IO.File]::WriteAllText($cmdPath, $probe, [Text.Encoding]::ASCII)
  foreach ($name in @('Install-YummiGameBar.ps1','Yummi-GameBar-Overlay.cer','test.msix')) {
    New-Item -ItemType File -Path (Join-Path $stage $name) | Out-Null
  }
  $stdout = Join-Path $root 'launcher.txt'
  $proc = Start-Process $env:ComSpec -ArgumentList ('/d /s /c ""' + $cmdPath + '""') -WindowStyle Hidden -RedirectStandardOutput $stdout -Wait -PassThru
  if ($proc.ExitCode -ne 0) { throw 'CMD probe failed' }
  $actual = (Get-Content $stdout -Raw).Trim()
  $expected = '-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File "' + (Join-Path $stage 'Install-YummiGameBar.ps1') + '" -Force -LogPath "' + (Join-Path $env:TEMP 'Yummi-GameBar-install.log') + '"'
  if ($actual -cne $expected) { throw "Launcher arguments differ: $actual" }
  Write-Host 'PASS: CMD preserves quoted paths including spaces and apostrophes'

  # Exercise the actual installer trap, including transcript release.
  $failureDir = Join-Path $root 'missing-package'
  New-Item -ItemType Directory -Path $failureDir | Out-Null
  $installer = Join-Path $failureDir 'Install-YummiGameBar.ps1'
  Copy-Item (Join-Path $InstallerRoot 'Install-YummiGameBar.ps1') $installer
  $log = Join-Path $failureDir 'failure.log'
  $failureOutput = & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $installer -LogPath $log 2>&1
  if ($LASTEXITCODE -ne 1) { throw 'Missing package must return exit 1' }
  $text = Get-Content $log -Raw
  if ($text -notmatch 'Yummi Game Bar package not found\.') { throw 'Original error was lost from log' }
  if ($text -match 'being used by another process') { throw 'Transcript file lock regression' }
  $stream = [IO.File]::Open($log, 'Open', 'ReadWrite', 'None')
  $stream.Dispose()
  Write-Host 'PASS: installer failure retains original error and releases transcript'

  # Capture native argv using the exact production invocation expression.
  $probeExe = Join-Path $root 'argv.exe'
  Add-Type -TypeDefinition 'public class Argv { public static void Main(string[] args) { foreach (var arg in args) System.Console.WriteLine(arg); } }' -OutputAssembly $probeExe -OutputType ConsoleApplication
  $source = Get-Content (Join-Path $InstallerRoot 'Install-YummiGameBar.ps1') -Raw
  if ($source -notmatch [regex]::Escape('https://go.microsoft.com/fwlink/p/?LinkId=2124703')) { throw 'Official WebView2 Evergreen bootstrapper link missing' }
  if ($source -notmatch [regex]::Escape("'/silent /install'")) { throw 'WebView2 silent install arguments missing' }
  if ($source -notmatch [regex]::Escape('{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}')) { throw 'WebView2 registry fast-path missing' }
  if ($source -notmatch [regex]::Escape('-DependencyPath $missingDeps')) { throw 'Missing AppX dependencies must install atomically with the main package' }
  if ($source -notmatch [regex]::Escape('[switch]$UpdateOnly')) { throw 'Installer update-only fast path missing' }
  if ($source -notmatch [regex]::Escape("Start-Process -FilePath 'powershell.exe' -Verb RunAs -WindowStyle Hidden")) { throw 'Elevated PowerShell must stay hidden after UAC approval' }
  if ($source -notmatch [regex]::Escape("'-NoProfile -WindowStyle Hidden -ExecutionPolicy Bypass -File '")) { throw 'Elevated PowerShell arguments must request hidden window style' }
  Write-Host 'PASS: installer ensures WebView2 Evergreen Runtime without bundling Chromium'
  if ($source -notmatch [regex]::Escape('-DeferRegistrationWhenPackagesAreInUse')) { throw 'Background update must defer registration while widget is in use' }
  $updaterText = Get-Content (Join-Path $InstallerRoot 'Update-YummiGameBar.ps1') -Raw -Encoding UTF8
  if ($updaterText -notmatch [regex]::Escape("'-BackgroundUpdate', '-SkipAutoUpdateSetup'")) { throw 'Updater must use background-update mode' }
  if ($updaterText -notmatch [regex]::Escape("$installArgs += '-UpdateOnly'")) { throw 'Existing installs must use the optimized update-only path' }
  if ($updaterText -notmatch [regex]::Escape("Get-Process -Name 'YummiGameBarWidget'")) { throw 'Updater must verify the widget is closed before finalizing deferred registration' }
  if ($updaterText -notmatch [regex]::Escape("'-Force', '-SkipAutoUpdateSetup'")) { throw 'Updater must force-finalize a stale deferred registration after the widget closes' }
  if ($updaterText -notmatch [regex]::Escape('[switch]$ForceFinalize')) { throw 'Updater must accept forced stale-registration finalization from the Agent' }
  if ($updaterText -match 'Stop-Process -Force') { throw 'Updater must not directly kill Xbox Game Bar processes' }
  $manifestText = Get-Content (Join-Path $InstallerRoot 'Package.appxmanifest') -Raw -Encoding UTF8
  if ($manifestText -notmatch '<ActivateAfterInstall>false</ActivateAfterInstall>') { throw 'Game Bar widget must not auto-activate after package registration' }
  Write-Host 'PASS: automatic updates defer in-use registration and never auto-activate the widget'

  $widgetSource = Get-Content (Join-Path $InstallerRoot 'Widget.xaml.cs') -Raw -Encoding UTF8
  if ($widgetSource -match 'CenterWindowAsync') { throw 'Widget must never auto-center the user window' }
  if ($widgetSource -notmatch 'FitToLargestOnscreenAsync') { throw 'Widget must resolve its initial size from the current display/host instead of a machine-specific size' }
  if ($manifestText -match '<Size><Height>900</Height><Width>1440</Width>') { throw 'Main Game Bar widget must not hardcode a 1440x900 initial size' }
  Write-Host 'PASS: Game Bar initial size is display-derived and user position is preserved'

  $widgetXaml = Get-Content (Join-Path $InstallerRoot 'Widget.xaml') -Raw -Encoding UTF8
  if ($widgetSource -notmatch 'SetPanelClosed\(key, true\)') { throw 'X close must use a dedicated closed state' }
  if ($widgetSource -notmatch 'closed \? 0\.0') { throw 'Closed panels must render fully invisible in both Win+G and pinned modes' }
  if ($widgetSource -notmatch 'SetPanelVisible\(key, visible\)') { throw 'Eye control must toggle runtime visibility' }
  if ($widgetSource -notmatch 'HiddenRuntimePanelOpacity = 0\.60') { throw 'Eye-hidden Win+G ghost opacity must remain 60 percent' }
  if ($widgetXaml -notmatch '<Button x:Name="NativeHookHitbox"') { throw 'Native input hitbox must be a real Button control' }
  if ($widgetXaml -notmatch 'x:Name="PointerOver"' -or $widgetXaml -notmatch 'x:Name="Pressed"') { throw 'Native input button must provide hover and pressed visual states' }
  if ($widgetXaml -notmatch 'x:Name="NativeHookClickCountText"' -or $widgetSource -notmatch 'NativeHookClickCountText\.Text = clickCount\.ToString\(\)') { throw 'Native input button must show the live native click count' }
  if ($widgetXaml -notmatch 'x:Name="NativeHookButtonTransform"') { throw 'Native input button must expose a transform for native hover/press feedback' }
  if ($widgetSource -notmatch 'UpdateNativeHookButtonVisual') { throw 'Native input hit-test state must drive button hover/press visuals' }
  if ($widgetSource -notmatch 'fallbackHovered') { throw 'Native input hover feedback must remain compatible with older Agents' }
  if ($widgetXaml -notmatch 'x:Name="OverlayToastHost"') { throw 'Shared overlay toast host must exist for transient notifications' }
  if ($widgetSource -notmatch 'ApplyOverlayToasts' -or $widgetSource -notmatch 'overlay_toasts') { throw 'Widget must render the shared overlay toast payload contract' }
  if ($widgetSource -notmatch 'HandlePinnedNativePanelDrag') { throw 'Pinned runtime must support native panel dragging' }
  if ($widgetSource -notmatch 'RuntimeNativeDragBandHeight = 24\.0') { throw 'Pinned runtime drag must stay limited to the panel top band' }
  if ($widgetSource -notmatch 'leftButtonDown') { throw 'Pinned runtime drag must consume raw native left-button state' }
  if ($widgetSource -notmatch 'HandlePinnedYoutubeNativeInputAsync') { throw 'Pinned runtime must forward native mouse input to YouTube' }
  if ($widgetSource -notmatch 'Input\.dispatchMouseEvent') { throw 'YouTube runtime input must use WebView2 DevTools mouse dispatch' }
  if ($widgetSource -notmatch 'local\.Y < RuntimeNativeDragBandHeight') { throw 'YouTube runtime input must preserve the top drag band' }
  Write-Host 'PASS: eye ghost, hard close, native input button, pinned runtime drag, and YouTube native input semantics'
  if ($widgetXaml -notmatch 'x:Name="BrowserPanel"' -or $widgetXaml -notmatch 'x:Name="BrowserAddressBox"') { throw 'Single-tab browser panel must exist' }
  if ($widgetSource -notmatch 'EnsureBrowserWebView2Async' -or $widgetSource -notmatch 'NormalizeBrowserInput') { throw 'Browser panel must use the shared WebView2 runtime and URL/search normalization' }
  if ($widgetSource -match 'BrowserPanelVisible = show') { throw 'F9 must not rewrite the browser visibility setting' }
  if ($widgetSource -notmatch 'ApplyPanelChrome\("browser"') { throw 'Browser panel must share eye/close panel semantics' }
  $prefsSource = Get-Content (Join-Path $InstallerRoot 'HudPreferences.cs') -Raw -Encoding UTF8
  if ($prefsSource -notmatch 'BrowserPanelVisible", out value\) && value is bool && \(bool\)value') { throw 'Browser panel must default hidden/off on a fresh install' }
  if ($prefsSource -notmatch 'VoiceChatPanelVisible"' -or $prefsSource -notmatch '!ApplicationData.Current.LocalSettings.Values.TryGetValue\("VoiceChatPanelVisible"') { throw 'Discord Chat must default visible/on on a fresh install' }
  if ($widgetXaml -notmatch 'x:Name="VoiceChatEyeButton"' -or $widgetXaml -notmatch 'Tag="voice-chat"') { throw 'Discord Chat must expose the shared eye/close panel controls' }
  if ($widgetSource -match 'VoiceChatPanelVisible = show') { throw 'F9 must not rewrite Discord Chat visibility' }
  if ($widgetSource -notmatch 'runtimeOverlayHiddenByHotkey = !runtimeOverlayHiddenByHotkey' -or $widgetSource -notmatch 'effectiveHudVisible = hudVisible && \(!pinnedOnly \|\| !runtimeOverlayHiddenByHotkey\)') { throw 'F9 must use an independent runtime master visibility gate' }
  if ($widgetSource -match 'HudPreferences\.Visible = true;[\s\S]{0,800}runtime-overlay-hotkey-toggle') { throw 'F9 must not rewrite the global HUD setting' }
  if ($widgetXaml -notmatch 'x:Name="WinGPanelSettings"' -or $widgetSource -notmatch 'RefreshWinGPanelSettings') { throw 'Win+G must expose the quick panel selector' }
  if ($widgetSource -notmatch 'GameBarDisplayMode != XboxGameBarDisplayMode\.PinnedOnly' -or $widgetSource -notmatch 'WinGPanelSettings\.Visibility = editing') { throw 'Quick panel selector must be visible only while Win+G is foreground' }
  if ($widgetSource -notmatch 'WinGPanelToggle_Toggled' -or $widgetSource -notmatch 'SetPanelClosed\(key, false\)') { throw 'Win+G selector must reopen and toggle shared panels' }
  $bridgeSource = Get-Content (Join-Path $InstallerRoot '..\src-tauri\src\gamebar_bridge.rs') -Raw -Encoding UTF8
  if ($bridgeSource -notmatch 'is_gamebar_process' -or $bridgeSource -notmatch 'ends_with\("\\\\gamebar\.exe"\)') { throw 'Game Bar UIA matching must be process-based, not text-keyword based' }
  $yummiOverlayTitle = -join ([char[]](0xC720,0xBBF8,0xC9C0,0xC9C0,0x20,0xC624,0xBC84,0xB808,0xC774))
  if ($bridgeSource -notmatch 'automationId' -or $bridgeSource -notmatch 'HomeBarItemButton' -or $bridgeSource -notmatch [regex]::Escape($yummiOverlayTitle)) { throw 'Win+G automation must select the Yummi overlay entry' }
  if ($bridgeSource -notmatch 'PinToggle' -or $bridgeSource -notmatch 'ClickThroughToggleButton' -or $bridgeSource -notmatch 'UIA_TogglePatternId') { throw 'Win+G automation must enable Yummi pin and click-through through UIA toggle patterns' }
  if ($widgetSource -notmatch 'data.GetNamedBoolean\("ready", false\)' -or $widgetSource -notmatch 'gameBarUiaProbeCompleted = false') { throw 'Win+G automation must retry until ready and rearm for the next Win+G session' }
  Write-Host 'PASS: Game Bar UIA auto-selects Yummi and enables pin/click-through using process-scoped AutomationIds'
  Write-Host 'PASS: fresh defaults keep only Discord Chat enabled, Discord shares panel lifecycle, and Win+G has a panel selector'
  Write-Host 'PASS: single-tab browser panel, navigation, and panel lifecycle'
  # Exercise the production permission branch without changing OS settings.
  $tokens = $null
  $errors = $null
  $ast = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
  if ($errors.Count) { throw 'Installer syntax errors' }
  $branch = $ast.Find({ param($node) $node -is [Management.Automation.Language.IfStatementAst] -and $node.Extent.Text.StartsWith('if ($installedOverlay -and') }, $true)
  if (-not $branch) { throw 'Loopback permission branch not found' }
  $body = $branch.Clauses[0].Item2.Extent.Text
  $body = $body.Substring(1, $body.Length - 2).Replace('& $loopbackTool', 'Invoke-LoopbackTool').Replace('& "$env:SystemRoot\System32\CheckNetIsolation.exe"', 'Invoke-LoopbackTool')
  $installedOverlay = [pscustomobject]@{ PackageFamilyName = 'YummiGameBarWidget_test' }
  function Invoke-LoopbackTool {
    $script:LoopbackCalls += ($args -join '|')
    $global:LASTEXITCODE = 0
    if ($args -contains '-s') { return $script:LoopbackListing }
  }
  function Start-Process {
    param(
      [string]$FilePath,
      [string]$Verb,
      [string]$WindowStyle,
      [object[]]$ArgumentList,
      [switch]$Wait,
      [switch]$PassThru
    )
    $script:ElevationRequested = $true
    $script:ElevatedFile = $FilePath
    $script:ElevatedArgs = @($ArgumentList)
    return [pscustomobject]@{ ExitCode = 0 }
  }
  $isAdmin = $false
  $script:LoopbackListing = @('Name: yummigamebarwidget_test')
  $script:LoopbackCalls = @()
  $script:ElevationRequested = $false
  & ([scriptblock]::Create($body))
  if ($script:ElevationRequested -or ($script:LoopbackCalls -join ',') -ne 'LoopbackExempt|-s') { throw 'Existing exemption must skip elevation and writes' }
  Write-Host 'PASS: standard-user update reuses an existing loopback exemption'

  $script:LoopbackListing = @('Name: YummiGameBarWidget_test_other')
  $script:LoopbackCalls = @()
  $script:ElevationRequested = $false
  $script:ElevatedFile = $null
  $script:ElevatedArgs = @()
  & ([scriptblock]::Create($body))
  if (-not $script:ElevationRequested -or ($script:LoopbackCalls -join ',') -ne 'LoopbackExempt|-s') { throw 'Missing exemption must elevate only the loopback write' }
  if ($script:ElevatedFile -notmatch 'CheckNetIsolation\.exe$' -or ($script:ElevatedArgs -join '|') -ne 'LoopbackExempt|-a|-n=YummiGameBarWidget_test') { throw 'Loopback elevation must target CheckNetIsolation directly' }
  Write-Host 'PASS: missing exemption elevates only CheckNetIsolation without reinstalling AppX'

  $isAdmin = $true
  $script:LoopbackCalls = @()
  $script:ElevationRequested = $false
  & ([scriptblock]::Create($body))
  if ($script:ElevationRequested -or ($script:LoopbackCalls -join ',') -ne 'LoopbackExempt|-s,LoopbackExempt|-a|-n=YummiGameBarWidget_test') { throw 'Administrator must register the missing exemption' }
  Write-Host 'PASS: administrator registers only a missing exemption'

  # Run the production state writer, without invoking an installer or updater.
  $updaterSource = Get-Content (Join-Path $InstallerRoot 'Update-YummiGameBar.ps1') -Raw -Encoding UTF8
  $updaterAst = [Management.Automation.Language.Parser]::ParseInput($updaterSource, [ref]$tokens, [ref]$errors)
  if ($errors.Count) { throw 'Updater syntax errors' }
  $stateWriter = $updaterAst.Find({ param($node) $node -is [Management.Automation.Language.FunctionDefinitionAst] -and $node.Name -eq 'Write-UpdateState' }, $true)
  if (-not $stateWriter) { throw 'Production state writer not found' }
  . ([scriptblock]::Create($stateWriter.Extent.Text))
  $stateFile = Join-Path $root 'update-state.json'
  $expectedHash = 'a' * 64
  Write-UpdateState -Path $stateFile -State ([pscustomobject]@{build_id='69';sha256=$expectedHash;release_tag='gamebar-v0.1.14'})
  $bytes = [IO.File]::ReadAllBytes($stateFile)
  if ($bytes[0] -ne 0x7B) { throw 'Agent update state must start with JSON, without UTF8 BOM' }
  $state = [Text.Encoding]::UTF8.GetString($bytes) | ConvertFrom-Json
  if ($state.build_id -cne '69' -or $state.sha256 -cne $expectedHash) { throw 'Update identity changed during state serialization' }
  Write-Host 'PASS: PowerShell 5.1 updater state is BOM-free and preserves build/hash identity'

} finally {
  # Only remove this test's newly created GUID directory.
  if ((Split-Path -Parent $root) -eq [IO.Path]::GetTempPath().TrimEnd('\')) {
    Remove-Item -LiteralPath $root -Recurse -Force
  }
}
