# multi_ai_cli 실행. 반드시 **실제 터미널**에서 돌려야 한다.
#
# ConPTY 는 콘솔이 붙어 있어야 동작한다. 파이프로 캡처하거나 Start-Job 으로
# 돌리면 자식 출력이 오지 않는다(실측).
#
#   .\run.ps1                  시작 화면 → 질문 입력 → 참여자별 패널
#   .\run.ps1 -Solo            claude 하나만 전체 화면 (R1 확인용)
#   .\run.ps1 -Solo codex      다른 에이전트로
#   .\run.ps1 -SelfTest        PTY+VT 파이프라인 자동 점검
param(
    [string]$Agent = 'claude',
    [switch]$SelfTest,
    [switch]$Solo
)

$ErrorActionPreference = 'Stop'
Set-Location $PSScriptRoot

$exe = Join-Path $PSScriptRoot 'target\debug\multi_ai_cli.exe'
if (-not (Test-Path $exe)) {
    & "$PSScriptRoot\build.ps1"
}

if ($SelfTest) {
    & $exe --selftest
} elseif ($Solo) {
    Write-Host "한 에이전트만 전체 화면. Ctrl+] 로 나옵니다."
    & $exe --solo $Agent
} else {
    Write-Host "Ctrl+] 가 프리픽스입니다. 이어서 1/2/3 포커스, n 새 질문, q 종료."
    & $exe
}
