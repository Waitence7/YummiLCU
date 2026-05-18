# csproj Version → deploy/agent-version.json (Windows / CI)
param(
    [string]$Csproj = "$PSScriptRoot\..\agent\YummiLcu.Agent\YummiLcu.Agent.csproj",
    [string]$Out = "$PSScriptRoot\agent-version.json",
    [string]$PublicUrl = "https://yummi.duckdns.org",
    [string]$Notes = ""
)

[xml]$xml = Get-Content $Csproj
$ver = $xml.Project.PropertyGroup.Version
if (-not $ver) { throw "Version not found in csproj" }
if (-not $Notes) { $Notes = "Yummi Agent $ver" }

$obj = [ordered]@{
    version = "$ver"
    url     = "$($PublicUrl.TrimEnd('/'))/agent/YummiAgent.zip"
    notes   = $Notes
}
$json = $obj | ConvertTo-Json
Set-Content -Path $Out -Value $json -Encoding UTF8
Write-Host "Wrote $Out"
Write-Host $json
