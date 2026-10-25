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
  $line = ($source -split "`n" | Where-Object { $_ -match 'CheckNetIsolation.exe.*LoopbackExempt' } | Select-Object -First 1).Trim()
  if (-not $line) { throw 'Loopback invocation not found' }
  $line = $line.Replace('"$env:SystemRoot\System32\CheckNetIsolation.exe"', '$probeExe').Replace(' | Out-Null', '')
  $installedOverlay = [pscustomobject]@{ PackageFamilyName = 'YummiGameBarWidget_test' }
  $argsSeen = @(& ([scriptblock]::Create($line)))
  if (($argsSeen -join '|') -cne 'LoopbackExempt|-a|-n=YummiGameBarWidget_test') { throw "Invalid native argv: $argsSeen" }
  Write-Host 'PASS: loopback package family is one complete native argument'
} finally {
  # Only remove this test's newly created GUID directory.
  if ((Split-Path -Parent $root) -eq [IO.Path]::GetTempPath().TrimEnd('\')) {
    Remove-Item -LiteralPath $root -Recurse -Force
  }
}
