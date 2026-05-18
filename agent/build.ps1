$ErrorActionPreference = "Stop"
Set-Location $PSScriptRoot
Set-Location YummiLcu.Agent
Write-Host "dotnet publish..."
dotnet publish -c Release -r win-x64 --self-contained true -p:PublishSingleFile=true
$out = Join-Path (Get-Location) "bin\Release\net8.0-windows\win-x64\publish"
Copy-Item (Join-Path $PSScriptRoot "agent.json") (Join-Path $out "agent.json") -Force
Write-Host ""
Write-Host "Build OK"
Write-Host "Run: $(Join-Path $out 'YummiLcu.Agent.exe')"
Start-Process explorer.exe $out
