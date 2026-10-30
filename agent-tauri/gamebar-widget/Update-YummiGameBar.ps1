param(
  [string]$PackagePath,
  [string]$ExpectedSha256,
  [string]$BuildId,
  [string]$ReleaseTag,
  [switch]$Quiet,
  [switch]$ForceFinalize
)

$ErrorActionPreference = 'Stop'
$stateDir = Join-Path $env:LOCALAPPDATA 'Yummi\GameBarUpdater'
$statePath = Join-Path $stateDir 'state.json'
$logPath = Join-Path $stateDir 'update.log'
New-Item -ItemType Directory -Force -Path $stateDir | Out-Null
$pendingRegistration = $false

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
      '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $installer + '"'), '-BackgroundUpdate', '-SkipAutoUpdateSetup'
    ) -Wait -PassThru -WindowStyle Hidden
    if ($proc.ExitCode -ne 0) { throw "오버레이 설치가 종료 코드 $($proc.ExitCode)로 실패했습니다." }

    $releaseVersion = $ReleaseTag -replace '^gamebar-v', ''
    if ($releaseVersion -notmatch '^\d+\.\d+\.\d+$') { throw "Game Bar release tag 형식이 올바르지 않습니다: $ReleaseTag" }
    $expectedAppxVersion = "$releaseVersion.$BuildId"
    $installedOverlay = Get-AppxPackage -Name 'YummiGameBarWidget' -ErrorAction SilentlyContinue | Select-Object -First 1
    if (-not $installedOverlay) { throw 'Game Bar 설치 후 AppX 패키지를 찾지 못했습니다.' }
    $actualAppxVersion = $installedOverlay.Version.ToString()
    $pendingRegistration = $actualAppxVersion -ne $expectedAppxVersion
    if ($pendingRegistration) {
      Write-UpdateLog "오버레이 업데이트 등록 지연: expected=$expectedAppxVersion active=$actualAppxVersion (위젯 종료 후 자동 마무리)"

      # DeferRegistrationWhenPackagesAreInUse can remain pending even after the
      # widget process exits because the Game Bar host may retain the package.
      # Preserve the seamless path while the widget is active, but once the
      # widget is actually gone, finish the already-verified update with the
      # installer's ForceApplicationShutdown path. This only affects the stale
      # Game Bar host and prevents an endless deferred-registration loop.
      Start-Sleep -Milliseconds 200
      $widgetRunning = Get-Process -Name 'YummiGameBarWidget' -ErrorAction SilentlyContinue | Select-Object -First 1
      if ($ForceFinalize -or -not $widgetRunning) {
        $finalizeReason = if ($ForceFinalize) { '보류 시간 초과' } else { '위젯 종료 확인' }
        Write-UpdateLog "오버레이 $finalizeReason, 보류 등록 강제 마무리 시도: expected=$expectedAppxVersion active=$actualAppxVersion"
        $finalizeProc = Start-Process -FilePath 'powershell.exe' -ArgumentList @(
          '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $installer + '"'), '-Force', '-SkipAutoUpdateSetup'
        ) -Wait -PassThru -WindowStyle Hidden
        if ($finalizeProc.ExitCode -ne 0) { throw "오버레이 보류 등록 마무리가 종료 코드 $($finalizeProc.ExitCode)로 실패했습니다." }

        $installedOverlay = Get-AppxPackage -Name 'YummiGameBarWidget' -ErrorAction SilentlyContinue | Select-Object -First 1
        if (-not $installedOverlay) { throw '보류 등록 마무리 후 Game Bar AppX 패키지를 찾지 못했습니다.' }
        $actualAppxVersion = $installedOverlay.Version.ToString()
        $pendingRegistration = $actualAppxVersion -ne $expectedAppxVersion
        if ($pendingRegistration) {
          Write-UpdateLog "오버레이 보류 등록 마무리 후에도 버전 불일치: expected=$expectedAppxVersion active=$actualAppxVersion"
        } else {
          Write-UpdateLog "오버레이 AppX 보류 등록 마무리 완료: $actualAppxVersion"
        }
      }
    } else {
      Write-UpdateLog "오버레이 AppX 검증 완료: $actualAppxVersion"
    }

    $newUpdaterStaged = Join-Path $stateDir 'Update-YummiGameBar.ps1.new'
    $currentUpdater = Join-Path $stateDir 'Update-YummiGameBar.ps1'
    if (Test-Path $newUpdaterStaged) { Move-Item $newUpdaterStaged $currentUpdater -Force }

    $installedState = [pscustomobject]@{
      build_id = $BuildId
      sha256 = $ExpectedSha256.ToLowerInvariant()
      release_tag = $ReleaseTag
      installed_at = (Get-Date).ToString('o')
      pending_registration = $pendingRegistration
    }
    Write-UpdateState -Path $statePath -State $installedState
    $completionMessage = if ($pendingRegistration) {
      "오버레이 업데이트 예약 완료: $ReleaseTag / build $BuildId"
    } else {
      "오버레이 업데이트 완료: $ReleaseTag / build $BuildId"
    }
    Write-UpdateLog $completionMessage
  }
  finally {
    Remove-Item $tempRoot -Recurse -Force -ErrorAction SilentlyContinue
    if (-not $pendingRegistration) { Remove-Item $PackagePath -Force -ErrorAction SilentlyContinue }
  }
}
catch {
  Write-UpdateLog "오버레이 업데이트 실패: $($_.Exception.Message)"
  exit 1
}
