param(
  [switch]$ForceCheck,
  [switch]$Quiet
)

$ErrorActionPreference = 'Stop'
$repo = 'Waitence7/YummiLCU'
$api = "https://api.github.com/repos/$repo/releases?per_page=15"
$stateDir = Join-Path $env:LOCALAPPDATA 'Yummi\GameBarUpdater'
$statePath = Join-Path $stateDir 'state.json'
$logPath = Join-Path $stateDir 'update.log'
New-Item -ItemType Directory -Force -Path $stateDir | Out-Null

function Write-UpdateLog([string]$message) {
  $line = "[$(Get-Date -Format 'yyyy-MM-dd HH:mm:ss')] $message"
  Add-Content -Path $logPath -Value $line -Encoding UTF8
  if (-not $Quiet) { Write-Host $message }
}

try {
  $headers = @{ 'User-Agent' = 'Yummi-GameBar-Updater'; 'Accept' = 'application/vnd.github+json' }
  $releases = Invoke-RestMethod -Uri $api -Headers $headers -Method Get -TimeoutSec 20
  $candidate = $null
  foreach ($release in $releases) {
    if ($release.draft) { continue }
    $asset = @($release.assets | Where-Object { $_.name -like 'Yummi-GameBar-Overlay-*.zip' }) | Select-Object -First 1
    if ($asset) {
      $candidate = [pscustomobject]@{ release = $release; asset = $asset }
      break
    }
  }
  if (-not $candidate) { throw '업데이트 가능한 Game Bar Overlay 릴리즈를 찾지 못했습니다.' }

  $asset = $candidate.asset
  $fingerprint = if ($asset.digest) { [string]$asset.digest } else { "$($asset.id):$($asset.updated_at):$($asset.size)" }
  $state = $null
  if (Test-Path $statePath) {
    try { $state = Get-Content $statePath -Raw | ConvertFrom-Json } catch { $state = $null }
  }

  if ($state -and $state.fingerprint -eq $fingerprint) {
    Write-UpdateLog "최신 오버레이 사용 중: $($candidate.release.tag_name)"
    exit 0
  }

  Write-UpdateLog "새 오버레이 감지: $($candidate.release.tag_name) / $($asset.name)"
  $tempRoot = Join-Path $env:TEMP ("YummiGameBarUpdate-" + [guid]::NewGuid().ToString('N'))
  $zipPath = Join-Path $tempRoot $asset.name
  $extractPath = Join-Path $tempRoot 'package'
  New-Item -ItemType Directory -Force -Path $extractPath | Out-Null

  try {
    Invoke-WebRequest -Uri $asset.browser_download_url -Headers $headers -OutFile $zipPath -UseBasicParsing -TimeoutSec 120
    Expand-Archive -Path $zipPath -DestinationPath $extractPath -Force

    $installer = Join-Path $extractPath 'Install-YummiGameBar.ps1'
    if (-not (Test-Path $installer)) { throw '다운로드한 오버레이에 설치 스크립트가 없습니다.' }

    # Refresh the updater itself before installing the package.
    $newUpdater = Join-Path $extractPath 'Update-YummiGameBar.ps1'
    if (Test-Path $newUpdater) {
      Copy-Item $newUpdater (Join-Path $stateDir 'Update-YummiGameBar.ps1.new') -Force
    }

    $proc = Start-Process -FilePath 'powershell.exe' -ArgumentList @(
      '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $installer + '"'), '-Force', '-SkipAutoUpdateSetup'
    ) -Wait -PassThru -WindowStyle Hidden
    if ($proc.ExitCode -ne 0) { throw "오버레이 설치가 종료 코드 $($proc.ExitCode)로 실패했습니다." }

    $newUpdaterStaged = Join-Path $stateDir 'Update-YummiGameBar.ps1.new'
    $currentUpdater = Join-Path $stateDir 'Update-YummiGameBar.ps1'
    if (Test-Path $newUpdaterStaged) {
      Move-Item $newUpdaterStaged $currentUpdater -Force
    }

    [pscustomobject]@{
      fingerprint = $fingerprint
      asset_id = $asset.id
      asset_name = $asset.name
      release_tag = $candidate.release.tag_name
      installed_at = (Get-Date).ToString('o')
    } | ConvertTo-Json | Set-Content -Path $statePath -Encoding UTF8

    Write-UpdateLog "오버레이 자동 업데이트 완료: $($candidate.release.tag_name)"
  }
  finally {
    Remove-Item $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
  }
}
catch {
  Write-UpdateLog "자동 업데이트 실패: $($_.Exception.Message)"
  exit 1
}
