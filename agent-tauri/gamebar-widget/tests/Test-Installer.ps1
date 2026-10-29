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
  Write-Host 'PASS: installer ensures WebView2 Evergreen Runtime without bundling Chromium'
  if ($source -notmatch [regex]::Escape('-DeferRegistrationWhenPackagesAreInUse')) { throw 'Background update must defer registration while widget is in use' }
  $updaterText = Get-Content (Join-Path $InstallerRoot 'Update-YummiGameBar.ps1') -Raw -Encoding UTF8
  if ($updaterText -notmatch [regex]::Escape("'-BackgroundUpdate', '-SkipAutoUpdateSetup'")) { throw 'Updater must use background-update mode' }
  if ($updaterText -match 'Stop-Process -Force') { throw 'Updater must not force-close Xbox Game Bar' }
  $manifestText = Get-Content (Join-Path $InstallerRoot 'Package.appxmanifest') -Raw -Encoding UTF8
  if ($manifestText -notmatch '<ActivateAfterInstall>false</ActivateAfterInstall>') { throw 'Game Bar widget must not auto-activate after package registration' }
  Write-Host 'PASS: automatic updates defer in-use registration and never auto-activate the widget'
  $line = ($source -split "`n" | Where-Object { $_ -match 'CheckNetIsolation.exe.*LoopbackExempt' } | Select-Object -First 1).Trim()
  if (-not $line) { throw 'Loopback invocation not found' }
  $line = $line.Replace('"$env:SystemRoot\System32\CheckNetIsolation.exe"', '$probeExe').Replace(' | Out-Null', '')
  $installedOverlay = [pscustomobject]@{ PackageFamilyName = 'YummiGameBarWidget_test' }
  $argsSeen = @(& ([scriptblock]::Create($line)))
  if (($argsSeen -join '|') -cne 'LoopbackExempt|-a|-n=YummiGameBarWidget_test') { throw "Invalid native argv: $argsSeen" }
  Write-Host 'PASS: loopback package family is one complete native argument'

  # Exercise the production permission branch without changing OS settings.
  $tokens = $null
  $errors = $null
  $ast = [Management.Automation.Language.Parser]::ParseInput($source, [ref]$tokens, [ref]$errors)
  if ($errors.Count) { throw 'Installer syntax errors' }
  $branch = $ast.Find({ param($node) $node -is [Management.Automation.Language.IfStatementAst] -and $node.Extent.Text.StartsWith('if ($installedOverlay -and') }, $true)
  if (-not $branch) { throw 'Loopback permission branch not found' }
  $body = $branch.Clauses[0].Item2.Extent.Text
  $body = $body.Substring(1, $body.Length - 2).Replace('& $loopbackTool', 'Invoke-LoopbackTool').Replace('& "$env:SystemRoot\System32\CheckNetIsolation.exe"', 'Invoke-LoopbackTool')
  function Invoke-LoopbackTool {
    $script:LoopbackCalls += ($args -join '|')
    $global:LASTEXITCODE = 0
    if ($args -contains '-s') { return $script:LoopbackListing }
  }
  function Restart-InstallerElevated {
    param([string]$InstallerPath)
    $script:ElevationRequested = $true
    throw 'TEST_ELEVATION_REQUEST'
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
  try { & ([scriptblock]::Create($body)); throw 'Expected elevation request' } catch {
    if ($_.Exception.Message -ne 'TEST_ELEVATION_REQUEST') { throw }
  }
  if (-not $script:ElevationRequested -or ($script:LoopbackCalls -join ',') -ne 'LoopbackExempt|-s') { throw 'Missing exemption must elevate before writes' }
  Write-Host 'PASS: missing exemption requests elevation before registration'

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
