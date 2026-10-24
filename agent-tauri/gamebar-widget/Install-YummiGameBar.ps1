param([switch]$Force)
$ErrorActionPreference = 'Stop'

$package = Get-ChildItem $PSScriptRoot -File |
  Where-Object { $_.Extension -in '.appx', '.msix' } |
  Select-Object -First 1
if (-not $package) { throw 'Yummi Game Bar package not found.' }

$certificate = Join-Path $PSScriptRoot 'Yummi-GameBar-Overlay.cer'
if (Test-Path $certificate) {
  $identity = [Security.Principal.WindowsIdentity]::GetCurrent()
  $principal = New-Object Security.Principal.WindowsPrincipal($identity)
  $isAdmin = $principal.IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)

  if (-not $isAdmin) {
    Write-Host 'Game Bar 오버레이 인증서 등록을 위해 관리자 권한을 요청합니다.' -ForegroundColor Yellow
    $scriptPath = $MyInvocation.MyCommand.Path
    $arguments = @('-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', ('"' + $scriptPath + '"'))
    if ($Force) { $arguments += '-Force' }
    $process = Start-Process -FilePath 'powershell.exe' -Verb RunAs -ArgumentList $arguments -Wait -PassThru
    if ($process.ExitCode -ne 0) { throw "관리자 설치 프로세스가 종료 코드 $($process.ExitCode)로 실패했습니다." }
    exit 0
  }

  Write-Host '오버레이 서명 인증서를 LocalMachine\\TrustedPeople에 등록합니다.'
  $imported = Import-Certificate -FilePath $certificate -CertStoreLocation 'Cert:\LocalMachine\TrustedPeople'
  if (-not $imported) { throw '오버레이 서명 인증서를 신뢰 저장소에 등록하지 못했습니다.' }
}

$depsDir = Join-Path $PSScriptRoot 'Dependencies'
$deps = @()
if (Test-Path $depsDir) {
  $deps = @(Get-ChildItem $depsDir -Recurse -File |
    Where-Object { $_.Extension -in '.appx', '.msix' } |
    Select-Object -ExpandProperty FullName)
}

if ($deps.Count -gt 0) {
  Add-AppxPackage -Path $package.FullName -DependencyPath $deps -ForceApplicationShutdown:$Force
} else {
  Add-AppxPackage -Path $package.FullName -ForceApplicationShutdown:$Force
}

Write-Host ''
Write-Host '설치 완료. Win+G > 위젯 메뉴 > 유미 LCU 에서 실행하세요.' -ForegroundColor Green
