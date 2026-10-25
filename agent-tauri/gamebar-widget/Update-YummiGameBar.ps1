param(
  [string]$PackagePath,
  [string]$ExpectedSha256,
  [string]$BuildId,
  [string]$ReleaseTag,
  [switch]$Quiet
)

$ErrorActionPreference = 'Stop'
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
  if (-not $PackagePath -or -not (Test-Path $PackagePath)) { throw 'Agent가 검증한 Game Bar ZIP 경로가 없습니다.' }
  if ($ExpectedSha256 -notmatch '^[0-9a-fA-F]{64}$') { throw '검증 SHA256 형식이 올바르지 않습니다.' }
  $actual = (Get-FileHash -Path $PackagePath -Algorithm SHA256).Hash.ToLowerInvariant()
  if ($actual -ne $ExpectedSha256.ToLowerInvariant()) { throw '설치 직전 Game Bar ZIP SHA256 재검증 실패' }

  $tempRoot = Join-Path $env:TEMP ("YummiGameBarUpdate-" + [guid]::NewGuid().ToString('N'))
  $extractPath = Join-Path $tempRoot 'package'
  New-Item -ItemType Directory -Force -Path $extractPath | Out-Null
  try {
    Expand-Archive -Path $PackagePath -DestinationPath $extractPath -Force
    $installer = Join-Path $extractPath 'Install-YummiGameBar.ps1'
    if (-not (Test-Path $installer)) { throw '검증된 오버레이 ZIP에 설치 스크립트가 없습니다.' }

    $newUpdater = Join-Path $extractPath 'Update-YummiGameBar.ps1'
    if (Test-Path $newUpdater) { Copy-Item $newUpdater (Join-Path $stateDir 'Update-YummiGameBar.ps1.new') -Force }

    $proc = Start-Process -FilePath 'powershell.exe' -ArgumentList @(
      '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $installer + '"'), '-Force', '-SkipAutoUpdateSetup'
    ) -Wait -PassThru -WindowStyle Hidden
    if ($proc.ExitCode -ne 0) { throw "오버레이 설치가 종료 코드 $($proc.ExitCode)로 실패했습니다." }

    $newUpdaterStaged = Join-Path $stateDir 'Update-YummiGameBar.ps1.new'
    $currentUpdater = Join-Path $stateDir 'Update-YummiGameBar.ps1'
    if (Test-Path $newUpdaterStaged) { Move-Item $newUpdaterStaged $currentUpdater -Force }

    [pscustomobject]@{
      build_id = $BuildId
      sha256 = $ExpectedSha256.ToLowerInvariant()
      release_tag = $ReleaseTag
      installed_at = (Get-Date).ToString('o')
    } | ConvertTo-Json | Set-Content -Path $statePath -Encoding UTF8
    Write-UpdateLog "오버레이 업데이트 완료: $ReleaseTag / build $BuildId"
  }
  finally {
    Remove-Item $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    Remove-Item $PackagePath -Force -ErrorAction SilentlyContinue
  }
}
catch {
  Write-UpdateLog "오버레이 업데이트 실패: $($_.Exception.Message)"
  exit 1
}
