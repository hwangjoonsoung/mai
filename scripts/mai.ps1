# multi_ai_cli 단일 진입점 (Windows). 빌드가 필요하면 빌드하고 바로 띄운다.
#
#   mai                     지금 있는 디렉터리를 공간으로 실행
#   mai C:\proj             그 디렉터리를 공간으로 실행
#   mai -Which              참여자를 어떻게 띄우는지 확인
#   mai -Probe              셸과 «cd 따라가기» 가 이 환경에서 되는지 점검
#   mai -Selftest           PTY+VT 파이프라인 점검
#   mai -Trust              지금 디렉터리를 각 에이전트에 신뢰 등록
#   mai -Rooms              저장된 방 목록
#   mai -Quote claude       답 넘기기가 무엇을 꺼내는지 확인
#   mai -Doctor             설치·인증·툴체인 점검
#   mai -Rebuild            강제로 다시 빌드하고 실행
#
# 앱의 플래그를 **스위치로 다 열어 둔다.** PowerShell 은 `--quote` 같은 토큰을
# 자기 파라미터로 해석하려다 실패하므로, 사용자가 raw 플래그를 칠 일이 없어야 한다.
#
# **cwd 를 바꾸지 않는다.** 앱은 자기 작업 디렉터리를 첫 공간으로 잡으므로,
# 어디서 쳤는지가 곧 어느 프로젝트인지다. 빌드만 Push-Location 으로 잠깐 옮긴다.
#
# scripts/mai (bash) 와 짝이다. 한쪽을 고치면 다른 쪽도 본다.

[CmdletBinding()]
param(
    [Parameter(Position = 0)][string]$Workspace,
    [switch]$Rebuild,
    [switch]$Doctor,
    [switch]$Which,
    [switch]$Probe,
    [switch]$Selftest,
    [switch]$Trust,
    [switch]$Rooms,
    [string]$Quote,
    [switch]$Release,
    [Parameter(ValueFromRemainingArguments = $true)][string[]]$Rest
)

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot

if ($Doctor) { & (Join-Path $PSScriptRoot 'doctor.ps1'); return }

# rustup 으로 깐 툴체인은 PATH 에 없을 수 있다. 흔한 자리를 직접 본다.
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) {
        $env:Path = "$cargoBin;$env:Path"
    }
}

$profileName = if ($Release) { 'release' } else { 'debug' }
$bin = Join-Path $root "rust\target\$profileName\multi_ai_cli.exe"

# 소스가 바이너리보다 새로우면 다시 빌드한다. cargo 에 맡기면 시작할 때마다
# 몇백 ms 를 쓰므로 우리가 먼저 걸러 낸다.
$stale = $true
if (Test-Path $bin) {
    $binTime = (Get-Item $bin).LastWriteTimeUtc
    $newer = Get-ChildItem -Path (Join-Path $root 'rust\src') -Filter '*.rs' -Recurse -ErrorAction SilentlyContinue |
        Where-Object { $_.LastWriteTimeUtc -gt $binTime } | Select-Object -First 1
    $toml = Join-Path $root 'rust\Cargo.toml'
    $tomlNewer = (Test-Path $toml) -and ((Get-Item $toml).LastWriteTimeUtc -gt $binTime)
    $stale = [bool]$newer -or $tomlNewer
}

if ($Rebuild -or $stale) {
    if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
        Write-Error @"
cargo 가 없다. Rust 툴체인을 먼저 깐다:
  winget install Rustlang.Rustup
  (또는 https://rustup.rs 의 rustup-init.exe)
"@
        return
    }
    Write-Host '빌드 중…' -ForegroundColor DarkGray
    Push-Location (Join-Path $root 'rust')
    try {
        # 실패했을 때만 전문을 보여준다. 성공했는데 경고까지 쏟으면 시작이 시끄럽다.
        $log = if ($Release) { cargo build --release 2>&1 } else { cargo build 2>&1 }
        if ($LASTEXITCODE -ne 0) {
            $log | ForEach-Object { Write-Host $_ }
            Write-Error '빌드 실패'
            return
        }
    } finally { Pop-Location }
}

if (-not (Test-Path $bin)) { Write-Error "빌드 산출물이 없다: $bin"; return }

# 한글이 깨지지 않게 콘솔을 UTF-8 로 둔다. Windows 기본은 949(CP949)다.
try {
    [Console]::OutputEncoding = [Text.Encoding]::UTF8
    [Console]::InputEncoding = [Text.Encoding]::UTF8
} catch { }

$argv = @()
if ($Which)    { $argv += '--which' }
if ($Probe)    { $argv += '--probe' }
if ($Selftest) { $argv += '--selftest' }
if ($Trust)    { $argv += '--trust' }
if ($Rooms)    { $argv += '--rooms' }
if ($Quote)    { $argv += @('--quote', $Quote) }
# 그 밖의 것은 그대로 넘긴다. PowerShell 이 삼키면 `mai --% --무엇` 을 쓴다.
if ($Rest)     { $argv += $Rest }

# 워크스페이스 지정은 곧 cwd 변경이다. 앱이 자기 cwd 를 첫 공간으로 잡는다.
if ($Workspace) { Push-Location $Workspace }
try {
    & $bin @argv
    exit $LASTEXITCODE
} finally {
    if ($Workspace) { Pop-Location }
}
