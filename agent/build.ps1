$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
Set-Location YummiLcu.App
Write-Host "dotnet publish..."
dotnet publish -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true
$out = Join-Path (Get-Location) "bin\Release\net8.0-windows\win-x64\publish"
$agentJson = Join-Path $PSScriptRoot "agent.json"
if (-not (Test-Path $agentJson)) { $agentJson = Join-Path $PSScriptRoot "agent.json.example" }
Copy-Item $agentJson (Join-Path $out "agent.json") -Force
Write-Host ""
Write-Host "Build OK"
Write-Host "Run: $(Join-Path $out 'YummiLcu.App.exe')"
Start-Process explorer.exe $out
