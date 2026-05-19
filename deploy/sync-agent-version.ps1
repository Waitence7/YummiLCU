# csproj Version → deploy/agent-version.json (Windows / CI)
param(
    [string]$Csproj = "$PSScriptRoot\..\agent\YummiLcu.App\YummiLcu.App.csproj",
    [string]$Out = "$PSScriptRoot\agent-version.json",
    [string]$PublicUrl = "https://yummi.duckdns.org",
    [string]$Notes = ""
)

[xml]$xml = Get-Content $Csproj
$ver = $xml.Project.PropertyGroup.Version
if (-not $ver) { throw "Version not found in csproj" }
if (-not $Notes) { $Notes = "Yummi Agent $ver" }

$base = $PublicUrl.TrimEnd('/')
$obj = [ordered]@{
    version      = "$ver"
    url          = "$base/agent/YummiAgent.zip"
    installerUrl = "$base/agent/YummiAgent-Setup-$ver.exe"
    notes        = $Notes
}
$zipPath = Join-Path $PSScriptRoot "YummiAgent.zip"
if (Test-Path $zipPath) {
    $hash = (Get-FileHash -Path $zipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $obj.sha256 = $hash
    Write-Host "sha256: $hash"
}
$json = $obj | ConvertTo-Json
Set-Content -Path $Out -Value $json -Encoding UTF8
Write-Host "Wrote $Out"
Write-Host $json
