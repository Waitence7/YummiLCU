param([string]$Source = (Join-Path (Split-Path -Parent $PSScriptRoot) 'SnapshotJson.cs'))
$ErrorActionPreference = 'Stop'
$root = Join-Path ([IO.Path]::GetTempPath()) ('Yummi-json-test-' + [guid]::NewGuid().ToString('N'))
New-Item -ItemType Directory -Path $root | Out-Null
try {
  $framework = Join-Path $env:SystemRoot 'Microsoft.NET\Framework64\v4.0.30319'
  $references = @(
    (Join-Path $env:SystemRoot 'System32\WinMetadata\Windows.Data.winmd'),
    (Join-Path $env:SystemRoot 'System32\WinMetadata\Windows.Foundation.winmd'),
    (Join-Path $framework 'System.Runtime.dll'),
    (Join-Path $framework 'System.Runtime.WindowsRuntime.dll')
  )
  $harness = @'
namespace YummiGameBarWidget {
 public static class SnapshotJsonTests {
  public static void Run() {
   var json=Windows.Data.Json.JsonObject.Parse("{\"riot_id\":null,\"phase\":\"Offline\",\"number\":123,\"boolean\":false,\"name\":\"test\"}");
   if(SnapshotJson.String(json,"riot_id")!="") throw new System.Exception("Null failed");
   if(SnapshotJson.String(json,"missing","fallback")!="fallback") throw new System.Exception("Missing failed");
   if(SnapshotJson.String(json,"number")!="") throw new System.Exception("Number failed");
   if(SnapshotJson.String(json,"boolean")!="") throw new System.Exception("Boolean failed");
   if(SnapshotJson.String(json,"phase")!="Offline") throw new System.Exception("Phase failed");
   if(SnapshotJson.String(json,"name")!="test") throw new System.Exception("String failed");
  }
 }
}
'@
  $testSource = Join-Path $root 'SnapshotJsonTests.cs'
  $dll = Join-Path $root 'SnapshotJsonTests.dll'
  [IO.File]::WriteAllText($testSource, ((Get-Content $Source -Raw) + "`n" + $harness), [Text.Encoding]::UTF8)
  $arguments = @('/nologo','/target:library',('/out:'+$dll)) + @($references | ForEach-Object { '/reference:'+$_ }) + @($testSource)
  & (Join-Path $framework 'csc.exe') @arguments
  if ($LASTEXITCODE -ne 0) { throw 'WinRT JSON test compilation failed' }
  $runner = Join-Path $root 'Run.ps1'
  @'
param([string]$Assembly)
$ErrorActionPreference = 'Stop'
[Reflection.Assembly]::LoadFrom($Assembly) | Out-Null
[YummiGameBarWidget.SnapshotJsonTests]::Run()
Write-Output 'PASS: production reader handles null, missing, wrong types and strings on real WinRT JSON'
'@ | Set-Content -LiteralPath $runner -Encoding UTF8
  & powershell.exe -NoProfile -ExecutionPolicy Bypass -File $runner -Assembly $dll
  if ($LASTEXITCODE -ne 0) { throw 'WinRT JSON regression failed' }
}
finally {
  # The child released its DLL. Cleanup only this newly created GUID directory.
  if ((Split-Path -Parent $root) -eq [IO.Path]::GetTempPath().TrimEnd('\')) {
    Remove-Item -LiteralPath $root -Recurse -Force -ErrorAction SilentlyContinue
  }
}
