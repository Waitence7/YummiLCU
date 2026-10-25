param([switch]$Force)
$ErrorActionPreference = 'Stop'

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
    $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
    $principal = New-Object Security.Principal.WindowsPrincipal($identity)
    $isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

    if (-not $isAdmin) {
      Write-Host '오버레이 인증서는 최초 1회만 관리자 권한으로 등록합니다.' -ForegroundColor Yellow
      $scriptPath = $MyInvocation.MyCommand.Path
      $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $scriptPath + '"'))
      if ($Force) { $arguments += '-Force' }
      $process = Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $arguments -Wait -PassThru
      if ($process.ExitCode -ne 0) { throw "관리자 설치 프로세스가 종료 코드 $($process.ExitCode)로 실패했습니다." }
      exit 0
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

Write-Host '유미 Game Bar 오버레이를 설치합니다.'
Add-AppxPackage -Path $package.FullName -ForceApplicationShutdown:$Force

Write-Host ''
Write-Host '설치 완료. Win+G > 위젯 메뉴 > 유미 LCU 에서 실행하세요.' -ForegroundColor Green
