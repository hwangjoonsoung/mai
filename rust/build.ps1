# Rust 빌드. MSVC 링커의 LIB 경로를 잡아준다.
#
# Developer Command Prompt 가 아니면 LIB/INCLUDE 가 없어 링커가
# 'dbghelp.lib' 를 못 찾는다(LNK1181). 여기서 직접 넣어준다.
param([switch]$Release)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

if (-not $env:PATH.Contains('.cargo')) {
    $env:PATH = "$env:USERPROFILE\.cargo\bin;$env:PATH"
}

# 설치된 MSVC 툴셋과 Windows SDK 버전을 자동 탐색한다.
$vcRoot = "C:\Program Files (x86)\Microsoft Visual Studio\2022\BuildTools\VC\Tools\MSVC"
$sdkRoot = "C:\Program Files (x86)\Windows Kits\10"
$vc = (Get-ChildItem $vcRoot -Directory | Sort-Object Name -Descending | Select-Object -First 1).FullName
$sdkVer = (Get-ChildItem "$sdkRoot\Lib" -Directory | Sort-Object Name -Descending | Select-Object -First 1).Name

$env:LIB = "$vc\lib\x64;$sdkRoot\Lib\$sdkVer\ucrt\x64;$sdkRoot\Lib\$sdkVer\um\x64"
$env:INCLUDE = "$vc\include;$sdkRoot\Include\$sdkVer\ucrt;$sdkRoot\Include\$sdkVer\um;$sdkRoot\Include\$sdkVer\shared"

Write-Host "MSVC : $vc"
Write-Host "SDK  : $sdkVer"

if ($Release) { cargo build --release } else { cargo build }
