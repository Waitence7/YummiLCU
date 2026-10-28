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

function Write-UpdateState([string]$Path, $State) {
  # PowerShell 5.1 -Encoding UTF8 adds a BOM that serde_json rejects.
  # The Agent must read this marker to avoid reinstalling an unchanged build.
  [IO.File]::WriteAllText($Path, ($State | ConvertTo-Json), [Text.UTF8Encoding]::new($false))
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

    $releaseVersion = $ReleaseTag -replace '^gamebar-v', ''
    if ($releaseVersion -notmatch '^\d+\.\d+\.\d+$') { throw "Game Bar release tag 형식이 올바르지 않습니다: $ReleaseTag" }
    $expectedAppxVersion = "$releaseVersion.$BuildId"
    $installedOverlay = Get-AppxPackage -Name 'YummiGameBarWidget' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $installedOverlay) { throw 'Game Bar 설치 후 AppX 패키지를 찾지 못했습니다.' }
    $actualAppxVersion = $installedOverlay.Version.ToString()
    if ($actualAppxVersion -ne $expectedAppxVersion) {
      throw "Game Bar 설치 버전 검증 실패: expected=$expectedAppxVersion actual=$actualAppxVersion"
    }
    Write-UpdateLog "오버레이 AppX 검증 완료: $actualAppxVersion"

    # Xbox Game Bar hosts widget code in its own process. Updating the AppX can
    # leave an already-open host running the previous widget DLL even though
    # Get-AppxPackage reports the new version. Force only the Game Bar host to
    # exit so the next Win+G launch loads the newly installed package.
    $gameBarProcesses = @('GameBar', 'GameBarFTServer')
    foreach ($processName in $gameBarProcesses) {
      Get-Process -Name $processName -ErrorAction SilentlyContinue | Stop-Process -Force -ErrorAction SilentlyContinue
    }
    Write-UpdateLog 'Xbox Game Bar host 종료 요청 완료; 다음 Win+G에서 새 위젯을 로드합니다.'

    $newUpdaterStaged = Join-Path $stateDir 'Update-YummiGameBar.ps1.new'
    $currentUpdater = Join-Path $stateDir 'Update-YummiGameBar.ps1'
    if (Test-Path $newUpdaterStaged) { Move-Item $newUpdaterStaged $currentUpdater -Force }

    $installedState = [pscustomobject]@{
      build_id = $BuildId
      sha256 = $ExpectedSha256.ToLowerInvariant()
      release_tag = $ReleaseTag
      installed_at = (Get-Date).ToString('o')
    }
    Write-UpdateState -Path $statePath -State $installedState
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
