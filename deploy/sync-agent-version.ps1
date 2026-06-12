# csproj Version → deploy/agent-version.json + deploy/latest.json (Windows / CI)
param(
    [string]$Csproj = "$PSScriptRoot\..\agent\YummiLcu.App\YummiLcu.App.csproj",
    [string]$Out = "$PSScriptRoot\agent-version.json",
    [string]$LatestOut = "$PSScriptRoot\latest.json",
    [string]$PublicUrl = "https://yummi.duckdns.org",
    [string]$Notes = "",
    [string]$ZipPath = "",
    [string]$PatchZipPath = "",
    [string]$PatchFrom = "",
    [string]$InstallerPath = ""
)

[xml]$xml = Get-Content $Csproj
$ver = $xml.Project.PropertyGroup.Version
if (-not $ver) { throw "Version not found in csproj" }
if (-not $Notes) { $Notes = "Yummi Agent $ver" }

$base = $PublicUrl.TrimEnd('/')
$obj = [ordered]@{
    version      = "$ver"
    url          = "$base/agent/YummiAgent.zip"
    installerUrl = "$base/agent/setup.exe"
    notes        = $Notes
}

if (-not $ZipPath) { $ZipPath = Join-Path $PSScriptRoot "YummiAgent.zip" }
if (-not (Test-Path $ZipPath)) {
    $ZipPath = Join-Path (Get-Location) "YummiAgent-win-x64-portable.zip"
}
if (Test-Path $ZipPath) {
    $obj.sha256 = (Get-FileHash -Path $ZipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Host "sha256: $($obj.sha256)"
}

if (-not $PatchZipPath) { $PatchZipPath = Join-Path (Get-Location) "YummiAgent-patch.zip" }
if (Test-Path $PatchZipPath) {
    if (-not $PatchFrom) {
        $prev = [version]$ver
        $PatchFrom = "$($prev.Major).$($prev.Minor).$([Math]::Max(0, $prev.Build - 1))"
    }
    $obj.patchUrl = "$base/agent/YummiAgent-patch.zip"
    $obj.patchFrom = $PatchFrom
    $obj.patchSha256 = (Get-FileHash -Path $PatchZipPath -Algorithm SHA256).Hash.ToLowerInvariant()
    Write-Host "patchFrom: $($obj.patchFrom) patchSha256: $($obj.patchSha256)"
}

$json = $obj | ConvertTo-Json
Set-Content -Path $Out -Value $json -Encoding UTF8
Write-Host "Wrote $Out"
Write-Host $json

if (-not $InstallerPath) { $InstallerPath = Join-Path (Get-Location) "agent\installer\output\YummiAgent-Setup.exe" }
if (Test-Path $InstallerPath) {
    $installerSha = (Get-FileHash -Path $InstallerPath -Algorithm SHA256).Hash.ToLowerInvariant()
    $latest = [ordered]@{
        version = "$ver"
        url     = "$base/agent/files/YummiAgent-Setup-$ver.exe"
        sha256  = $installerSha
        notes   = $Notes
    }
    $latestJson = $latest | ConvertTo-Json
    Set-Content -Path $LatestOut -Value $latestJson -Encoding UTF8
    Write-Host "Wrote $LatestOut"
    Write-Host $latestJson
}
