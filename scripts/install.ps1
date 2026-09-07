# `mai` 한 단어로 어디서든 띄울 수 있게 설치한다 (Windows).
#
#   .\scripts\install.ps1              %LOCALAPPDATA%\mai\bin\mai.cmd 심 생성
#   .\scripts\install.ps1 -AddToPath   위에 더해 사용자 PATH 에 그 디렉터리를 넣는다
#   .\scripts\install.ps1 -AsFunction  심 대신 $PROFILE 에 함수를 추가한다
#   .\scripts\install.ps1 -Uninstall   둘 다 되돌린다
#
# 심링크를 쓰지 않는다. Windows 에서 심링크는 관리자 권한이나 개발자 모드가
# 필요해 «되는 사람과 안 되는 사람» 이 갈린다. .cmd 심은 아무 권한도 필요 없고
# PowerShell·cmd·다른 도구 어디서 불러도 똑같이 동작한다.
#
# scripts/install.sh 와 짝이다.

[CmdletBinding()]
param(
    [switch]$AddToPath,
    # 이름을 -Profile 로 두면 안 된다. PowerShell 의 자동 변수 $PROFILE 과
    # 이름이 같아(대소문자 무시) 스크립트 안에서 그 값을 가려 버린다.
    [switch]$AsFunction,
    [switch]$Uninstall
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$target = Join-Path $root 'scripts\mai.ps1'
$binDir = Join-Path $env:LOCALAPPDATA 'mai\bin'
$shim = Join-Path $binDir 'mai.cmd'
$marker = '# multi_ai_cli'

if ($Uninstall) {
    if (Test-Path $shim) { Remove-Item $shim -Force; Write-Host "지웠다: $shim" }
    if ((Test-Path $PROFILE) -and (Select-String -Path $PROFILE -SimpleMatch $marker -Quiet)) {
        $kept = Get-Content $PROFILE | Where-Object { $_ -notmatch [regex]::Escape($marker) -and $_ -notmatch '^function\s+mai\s' }
        Set-Content -Path $PROFILE -Value $kept -Encoding UTF8
        Write-Host "지웠다: $PROFILE 의 mai 함수"
    }
    return
}

if ($AsFunction) {
    if (-not (Test-Path $PROFILE)) { New-Item -ItemType File -Path $PROFILE -Force | Out-Null }
    if (Select-String -Path $PROFILE -SimpleMatch $marker -Quiet) {
        Write-Host "이미 있다: $PROFILE"
    } else {
        Add-Content -Path $PROFILE -Encoding UTF8 -Value @(
            ''
            $marker
            "function mai { & '$target' @args }"
        )
        Write-Host "추가했다: $PROFILE"
    }
    Write-Host "새 PowerShell 을 열거나  . `$PROFILE  을 실행한다."
    return
}

New-Item -ItemType Directory -Path $binDir -Force | Out-Null

# %* 를 그대로 넘긴다. -File 은 인자를 문자열로 받으므로 스위치도 그대로 통한다.
@"
@echo off
powershell -NoLogo -NoProfile -ExecutionPolicy Bypass -File "$target" %*
"@ | Set-Content -Path $shim -Encoding ASCII

Write-Host "설치했다: $shim -> $target"

$userPath = [Environment]::GetEnvironmentVariable('Path', 'User')
if ($userPath -split ';' -contains $binDir) {
    Write-Host "PATH 에 있다. 새 셸에서 'mai' 로 바로 띄운다."
} elseif ($AddToPath) {
    # 사용자 범위만 건드린다. 시스템 PATH 는 손대지 않는다.
    [Environment]::SetEnvironmentVariable('Path', "$userPath;$binDir", 'User')
    Write-Host "사용자 PATH 에 넣었다: $binDir"
    Write-Host "새 셸에서 'mai' 로 바로 띄운다."
} else {
    Write-Host ""
    Write-Host "$binDir 가 PATH 에 없다. 넣으려면:"
    Write-Host "  .\scripts\install.ps1 -AddToPath"
}
