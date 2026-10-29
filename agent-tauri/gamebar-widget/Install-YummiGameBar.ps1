param(
  [switch]$Force,
  [switch]$SkipAutoUpdateSetup,
  [switch]$BackgroundUpdate,
  [string]$LogPath = (Join-Path $env:TEMP 'Yummi-GameBar-install.log')
)
$ErrorActionPreference = 'Stop'

$script:TranscriptStarted = $false
try {
  $logDir = Split-Path -Parent $LogPath
  if ($logDir) { New-Item -ItemType Directory -Force -Path $logDir | Out-Null }
  Start-Transcript -Path $LogPath -Force | Out-Null
  $script:TranscriptStarted = $true
}
catch {
  # Installation must still run even if transcript initialization itself fails.
}

trap {
  $detail = ($_ | Out-String).TrimEnd()
  if ($script:TranscriptStarted) { try { Stop-Transcript | Out-Null } catch {} }
  try {
    if ($detail) { Add-Content -Path $LogPath -Value $detail -Encoding UTF8 }
  } catch {}
  exit 1
}

function Test-WebView2Runtime {
  $roots = @(
    (Join-Path ([Environment]::GetFolderPath('ProgramFilesX86')) 'Microsoft\EdgeWebView\Application'),
    (Join-Path ([Environment]::GetFolderPath('ProgramFiles')) 'Microsoft\EdgeWebView\Application'),
    (Join-Path $env:LOCALAPPDATA 'Microsoft\EdgeWebView\Application')
  ) | Where-Object { $_ -and (Test-Path $_) } | Select-Object -Unique

  foreach ($root in $roots) {
    $runtime = Get-ChildItem $root -Directory -ErrorAction SilentlyContinue |
      Where-Object { Test-Path (Join-Path $_.FullName 'msedgewebview2.exe') } |
      Select-Object -First 1
    if ($runtime) { return $true }
  }
  return $false
}

function Install-WebView2RuntimeIfMissing {
  if (Test-WebView2Runtime) {
    Write-Host 'Microsoft Edge WebView2 Runtime 확인됨.' -ForegroundColor DarkGray
    return
  }

  # Microsoft's Evergreen bootstrapper is intentionally downloaded at install
  # time so Chromium/WebView2 is not bundled into the Yummi overlay archive.
  $bootstrapperUri = 'https://go.microsoft.com/fwlink/p/?LinkId=2124703'
  $bootstrapper = Join-Path $env:TEMP 'MicrosoftEdgeWebview2Setup.exe'
  Write-Host 'WebView2 Runtime이 없어 Microsoft Evergreen Runtime을 설치합니다.' -ForegroundColor Yellow
  try {
    Invoke-WebRequest -UseBasicParsing -Uri $bootstrapperUri -OutFile $bootstrapper
    $process = Start-Process -FilePath $bootstrapper -ArgumentList '/silent /install' -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "WebView2 설치 종료 코드: $($process.ExitCode)" }
    if (-not (Test-WebView2Runtime)) { throw '설치 후 WebView2 Runtime을 찾지 못했습니다.' }
    Write-Host 'WebView2 Runtime 설치 완료.' -ForegroundColor Green
  }
  finally {
    Remove-Item $bootstrapper -Force -ErrorAction SilentlyContinue
  }
}

function Restart-InstallerElevated {
  param([string]$InstallerPath)
  # Release the transcript before the elevated process opens the same log.
  if ($script:TranscriptStarted) {
    Stop-Transcript | Out-Null
    $script:TranscriptStarted = $false
  }
  $quote = [char]34
  $forceArg = if ($Force) { ' -Force' } else { '' }
  $skipArg = if ($SkipAutoUpdateSetup) { ' -SkipAutoUpdateSetup' } else { '' }
  $backgroundArg = if ($BackgroundUpdate) { ' -BackgroundUpdate' } else { '' }
  $arguments = '-NoProfile -ExecutionPolicy Bypass -File ' + $quote + $InstallerPath + $quote + $forceArg + $skipArg + $backgroundArg + ' -LogPath ' + $quote + $LogPath + $quote
  $process = Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $arguments -Wait -PassThru
  if ($process.ExitCode -ne 0) {
    if (Test-Path $logPath) {
      Write-Host ''
      Write-Host '관리자 설치 로그:' -ForegroundColor Yellow
      Get-Content $logPath | ForEach-Object { Write-Host $_ }
    }
    throw "관리자 설치 프로세스가 종료 코드 $($process.ExitCode)로 실패했습니다. 로그: $logPath"
  }
  exit 0
}

$identity = [Security.Principal.WindowsIdentity]::GetCurrent()
$principal = New-Object Security.Principal.WindowsPrincipal($identity)
$isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

$package = Get-ChildItem $PSScriptRoot -File |
  Where-Object { $_.Extension -in '.appx', '.msix' } |
  Select-Object -First 1
if (-not $package) { throw 'Yummi Game Bar package not found.' }

$certificate = Join-Path $PSScriptRoot 'Yummi-GameBar-Overlay.cer'
if (Test-Path $certificate) {
  $certInfo = New-Object System.Security.Cryptography.X509Certificates.X509Certificate2($certificate)
  $thumbprint = $certInfo.Thumbprint
  $trustedPeoplePath = "Cert:\LocalMachine\TrustedPeople\$thumbprint"
  $trustedRootPath = "Cert:\LocalMachine\Root\$thumbprint"
  $certificateAlreadyTrusted = (Test-Path $trustedPeoplePath) -and (Test-Path $trustedRootPath)

  if ($certificateAlreadyTrusted) {
    Write-Host "이미 신뢰된 오버레이 인증서 사용: $thumbprint" -ForegroundColor DarkGray
  } else {
    if (-not $isAdmin) {
      Write-Host '오버레이 인증서는 최초 1회만 관리자 권한으로 등록합니다.' -ForegroundColor Yellow
      Restart-InstallerElevated -InstallerPath $PSCommandPath
    }

    Write-Host '오버레이 서명 인증서를 최초 1회 등록합니다.'
    if (-not (Test-Path $trustedPeoplePath)) {
      $trustedPeople = Import-Certificate -FilePath $certificate -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople'
      if (-not $trustedPeople) { throw '오버레이 서명 인증서를 TrustedPeople에 등록하지 못했습니다.' }
    }
    if (-not (Test-Path $trustedRootPath)) {
      $trustedRoot = Import-Certificate -FilePath $certificate -CertStoreLocation 'Cert:\LocalMachine\Root'
      if (-not $trustedRoot) { throw '오버레이 서명 인증서를 Trusted Root에 등록하지 못했습니다.' }
    }
  }
}

$depsDir = Join-Path $PSScriptRoot 'Dependencies'
$deps = @()
if (Test-Path $depsDir) {
  $deps = @(Get-ChildItem $depsDir -Recurse -File |
    Where-Object { $_.Extension -in '.appx', '.msix' } |
    Where-Object { $_.FullName -notmatch '(?i)\\(arm|arm64|x86)\\' -and $_.Name -notmatch '(?i)_(arm|arm64|x86)_' })
}

# Framework packages are shared by many Windows apps. Reinstalling an already
# installed framework can stall AppX deployment while those apps are running.
$missingDeps = @()
foreach ($dep in $deps) {
  $name = $null
  if ($dep.Name -match '^Microsoft\.NET\.Native\.Framework\.2\.2') { $name = 'Microsoft.NET.Native.Framework.2.2' }
  elseif ($dep.Name -match '^Microsoft\.NET\.Native\.Runtime\.2\.2') { $name = 'Microsoft.NET.Native.Runtime.2.2' }
  elseif ($dep.Name -match '^Microsoft\.UI\.Xaml\.2\.8') { $name = 'Microsoft.UI.Xaml.2.8' }
  elseif ($dep.Name -match '^Microsoft\.VCLibs\.x64\.14\.00\.Desktop') { $name = 'Microsoft.VCLibs.140.00.UWPDesktop' }
  elseif ($dep.Name -match '^Microsoft\.VCLibs\.x64\.14\.00') { $name = 'Microsoft.VCLibs.140.00' }

  $installed = if ($name) { Get-AppxPackage -Name $name -ErrorAction SilentlyContinue } else { $null }
  if ($installed) {
    Write-Host "이미 설치된 의존성 건너뜀: $($dep.Name)" -ForegroundColor DarkGray
  } else {
    $missingDeps += $dep.FullName
  }
}

foreach ($depPath in $missingDeps) {
  Write-Host "의존성 설치: $([IO.Path]::GetFileName($depPath))"
  Add-AppxPackage -Path $depPath -ForceApplicationShutdown:$Force
}

Install-WebView2RuntimeIfMissing

Write-Host '유미지지 Xbox Game Bar 오버레이를 설치합니다.'
if ($BackgroundUpdate) {
  # Do not tear down an active Game Bar/widget during an automatic update.
  # Windows will finish registration after the package is no longer in use.
  Add-AppxPackage -Path $package.FullName -ForceUpdateFromAnyVersion -DeferRegistrationWhenPackagesAreInUse
} else {
  Add-AppxPackage -Path $package.FullName -ForceApplicationShutdown:$Force -ForceUpdateFromAnyVersion
}

# UWP/Game Bar widgets are AppContainer processes. Enable the documented
# localhost exemption so the overlay can reach the full-trust Agent fallback
# bridge when cross-package named pipes are unavailable on a Windows build.
$installedOverlay = Get-AppxPackage -Name 'YummiGameBarWidget' -ErrorAction SilentlyContinue | Select-Object -First 1
if ($installedOverlay -and $installedOverlay.PackageFamilyName) {
  $loopbackTool = "$env:SystemRoot\System32\CheckNetIsolation.exe"
  $loopbackEntries = & $loopbackTool LoopbackExempt -s
  if ($LASTEXITCODE -ne 0) { throw "Game Bar loopback exemption query failed: $LASTEXITCODE" }
  $familyPattern = '(?i)(?<![A-Za-z0-9_])' + [regex]::Escape($installedOverlay.PackageFamilyName) + '(?![A-Za-z0-9_])'
  $alreadyExempt = ($loopbackEntries -join "`n") -match $familyPattern
  if ($alreadyExempt) {
    Write-Host 'Game Bar loopback exemption already registered.' -ForegroundColor DarkGray
  } else {
    if (-not $isAdmin) {
      Write-Host 'Game Bar loopback setup requires administrator permission.' -ForegroundColor Yellow
      Restart-InstallerElevated -InstallerPath $PSCommandPath
    }
    & "$env:SystemRoot\System32\CheckNetIsolation.exe" LoopbackExempt -a ('-n=' + $installedOverlay.PackageFamilyName) | Out-Null
    if ($LASTEXITCODE -ne 0) { throw "Game Bar loopback exemption registration failed: $LASTEXITCODE" }
  }
}

Write-Host ''
Write-Host '설치 완료. Win+G > 위젯 메뉴 > 유미지지 오버레이 에서 실행하세요.' -ForegroundColor Green


if (-not $SkipAutoUpdateSetup) {
  try {
    $updaterSource = Join-Path $PSScriptRoot 'Update-YummiGameBar.ps1'
    if (Test-Path $updaterSource) {
      $updaterDir = Join-Path $env:LOCALAPPDATA 'Yummi\GameBarUpdater'
      $updaterTarget = Join-Path $updaterDir 'Update-YummiGameBar.ps1'
      New-Item -ItemType Directory -Force -Path $updaterDir | Out-Null
      Copy-Item $updaterSource $updaterTarget -Force

      # The 유미지지 데스크톱 owns periodic overlay update checks.
      # Keep only the updater files here; no scheduled task is required.
      Write-Host '오버레이 자동 업데이트 준비 완료 (유미지지 데스크톱가 5분마다 확인).' -ForegroundColor DarkGray
    } else {
      Write-Warning '오버레이 updater 스크립트를 찾지 못했습니다.'
    }
  }
  catch {
    Write-Warning "자동 업데이트 준비 실패: $($_.Exception.Message)"
  }
}

if ($script:TranscriptStarted) { Stop-Transcript | Out-Null }
