# multi_ai_cli 실행.
# 사용법:  .\scripts\run.ps1 [-Workspace <path>] [-Room <id>]
#
# 콘솔 코드페이지와 Java 출력 인코딩을 UTF-8 로 맞춘다.
# 스트림 디코딩(UTF-8)과 콘솔 출력 코드페이지는 별개다 — 이걸 안 맞추면
# 한글과 발화자 블록이 CP949 로 깨진다 (SPEC §6.3, 교차검토 R11).

param(
    [string]$Workspace = (Get-Location).Path,
    [string]$Room = ''
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$out  = Join-Path $root 'out'

if (-not (Test-Path (Join-Path $out 'io\multiai\cli\Main.class'))) {
    Write-Host "빌드 산출물이 없다. 먼저 컴파일한다."
    & (Join-Path $PSScriptRoot 'compile.ps1')
}

chcp 65001 > $null
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
[Console]::InputEncoding  = [System.Text.Encoding]::UTF8

$java = 'java'
if ($env:JAVA_HOME) {
    $candidate = Join-Path $env:JAVA_HOME 'bin\java.exe'
    if (Test-Path $candidate) { $java = $candidate }
}

# TUI 는 터미널 크기를 알아야 칸을 나눈다. JVM 은 직접 알 수 없으므로 여기서 넘긴다.
$sz = $Host.UI.RawUI.WindowSize
$cols = if ($sz -and $sz.Width -gt 0) { $sz.Width } else { 120 }
$rows = if ($sz -and $sz.Height -gt 0) { $sz.Height } else { 30 }

$jvmArgs = @(
    "-Dterm.cols=$cols"
    "-Dterm.rows=$rows"
    '-Dfile.encoding=UTF-8'
    '-Dsun.stdout.encoding=UTF-8'
    '-Dsun.stderr.encoding=UTF-8'
    '-cp', $out
    'io.multiai.cli.Main'
    '--workspace', $Workspace
)
if ($Room) { $jvmArgs += @('--room', $Room) }

# 파이프로 들어온 입력은 스크립트의 $input 에 담기고 자식 프로세스로 자동 전달되지
# 않는다. 배치 실행(Get-Content cmds.txt | .\run.ps1)을 지원하려면 명시적으로 넘겨야
# 한다. 다만 파이프가 없을 때 $input 을 넘기면 stdin 이 즉시 닫혀 대화형이 깨지므로
# ExpectingInput 으로 갈라준다.
if ($MyInvocation.ExpectingInput) {
    $input | & $java @jvmArgs
} else {
    & $java @jvmArgs
}
