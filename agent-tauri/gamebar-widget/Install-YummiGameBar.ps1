param([switch]$Force)
$ErrorActionPreference = 'Stop'

$package = Get-ChildItem $PSScriptRoot -File |
  Where-Object { $_.Extension -in '.appx', '.msix' } |
  Select-Object -First 1
if (-not $package) { throw 'Yummi Game Bar package not found.' }

$certificate = Join-Path $PSScriptRoot 'Yummi-GameBar-Overlay.cer'
if (Test-Path $certificate) {
  Write-Host '오버레이 서명 인증서를 현재 사용자 TrustedPeople에 등록합니다.'
  Import-Certificate -FilePath $certificate -CertStoreLocation 'Cert:\CurrentUser\TrustedPeople' | Out-Null
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
