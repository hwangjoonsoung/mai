# Rust 없이 설치한다 (Windows). GitHub 최신 Release 의 실행 파일을 내려받는다.
#
#   irm https://raw.githubusercontent.com/hwangjoonsoung/mai/main/scripts/install-remote.ps1 | iex
#
# 업데이트도 같은 한 줄이다. 실행 중인 mai 가 있으면 먼저 닫는다(파일이 잠겨 있다).
#
# 놓는 자리는 install.ps1 과 같다(%LOCALAPPDATA%\mai\bin\mai.cmd). 소스 빌드 심이 있었다면
# 이 설치가 그 자리를 대신한다 — 소스로 돌아가려면 install.ps1 을 다시 실행한다.
#
# iex 로 돌기 때문에 exit 를 쓰지 않는다. 사용자의 셸이 닫힌다.

& {
    $ErrorActionPreference = 'Stop'
    # PowerShell 5.1 은 기본 TLS 가 낮아 GitHub 연결이 실패할 수 있다.
    [Net.ServicePointManager]::SecurityProtocol = [Net.SecurityProtocolType]::Tls12

    $url = 'https://github.com/hwangjoonsoung/mai/releases/latest/download/mai-windows-x64.exe'
    $binDir = Join-Path $env:LOCALAPPDATA 'mai\bin'
    $exe = Join-Path $binDir 'multi_ai_cli.exe'
    $shim = Join-Path $binDir 'mai.cmd'

    New-Item -ItemType Directory -Path $binDir -Force | Out-Null
    Write-Host "내려받는 중: $url"
    Invoke-WebRequest -Uri $url -OutFile $exe -UseBasicParsing

    # mai.ps1 처럼 콘솔을 UTF-8 로 두고 띄운다. Windows 기본은 949(CP949)다.
    @"
@echo off
chcp 65001 >nul
"%~dp0multi_ai_cli.exe" %*
"@ | Set-Content -Path $shim -Encoding ASCII
    Write-Host "설치했다: $shim"

    # 사용자 범위 PATH 만 건드린다.
    $userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
    if (-not ($userPath -split ';' -contains $binDir)) {
        [Environment]::SetEnvironmentVariable('Path', "$userPath;$binDir", 'User')
        Write-Host "사용자 PATH 에 넣었다: $binDir"
    }
    # 지금 셸에서도 바로 쓰게 한다.
    if (-not ($env:Path -split ';' -contains $binDir)) { $env:Path = "$env:Path;$binDir" }

    Write-Host ""
    Write-Host "끝. 아무 경로에서 'mai' 를 친다. 플래그는 --trust · --which 처럼 쓴다."
}
