# 착수 전 환경 점검. SPEC §7.10 완료 기준의 /status 를 셸에서 미리 확인한다.
# 사용법:  .\scripts\doctor.ps1

$ErrorActionPreference = 'Continue'

Write-Host ""
Write-Host "== CLI 설치 확인 =="
foreach ($name in @('claude', 'codex', 'agy')) {
    $cmd = Get-Command $name -ErrorAction SilentlyContinue
    if ($cmd) {
        $ver = (& $name --version 2>&1 | Select-Object -First 1)
        Write-Host ("  {0,-8} {1}" -f $name, $cmd.Source)
        Write-Host ("  {0,-8} {1}" -f '', $ver)
    } else {
        Write-Host ("  {0,-8} MISSING" -f $name)
    }
}

Write-Host ""
Write-Host "== Codex 실행 경로 (SPEC §6.3) =="
# .cmd / .ps1 폴백은 쓰지 않는다. 네이티브 exe 또는 node + codex.js 만 쓴다.
$npmRoot = Join-Path $env:APPDATA 'npm'
$vendor = Get-ChildItem -Path (Join-Path $npmRoot 'node_modules\@openai\codex\node_modules') `
    -Filter 'codex.exe' -Recurse -ErrorAction SilentlyContinue |
    Where-Object { $_.FullName -like '*\vendor\*' } | Select-Object -First 1
if ($vendor) {
    Write-Host "  tier 1  네이티브: $($vendor.FullName)"
} else {
    $js = Join-Path $npmRoot 'node_modules\@openai\codex\bin\codex.js'
    if (Test-Path $js) { Write-Host "  tier 2  node + $js" }
    else { Write-Host "  tier 3  지원 불가 — Codex 참여자는 비활성된다" }
}

Write-Host ""
Write-Host "== agy 사용 가능 모델 (SPEC D13) =="
if (Get-Command agy -ErrorAction SilentlyContinue) {
    $models = (& agy models 2>&1) | Where-Object { $_ -match '^gemini-' }
    if ($models) { $models | Select-Object -First 12 | ForEach-Object { Write-Host "  $_" } }
    else { Write-Host "  모델 조회 실패 — 인증 상태를 확인하라" }
} else {
    Write-Host "  agy MISSING"
}

Write-Host ""
Write-Host "== Rust 툴체인 (현행 구현) =="
if (-not (Get-Command cargo -ErrorAction SilentlyContinue)) {
    $cargoBin = Join-Path $env:USERPROFILE '.cargo\bin'
    if (Test-Path (Join-Path $cargoBin 'cargo.exe')) { $env:Path = "$cargoBin;$env:Path" }
}
if (Get-Command cargo -ErrorAction SilentlyContinue) {
    Write-Host ("  {0,-8} {1}" -f 'cargo', (& cargo --version))
    Write-Host ("  {0,-8} {1}" -f 'rustc', (& rustc --version))
} else {
    Write-Host "  MISSING — Rust 판을 빌드할 수 없다. 설치:"
    Write-Host "    winget install Rustlang.Rustup"
}

Write-Host ""
Write-Host "== Java (참조용 Java 판) =="
$java = 'java'
if ($env:JAVA_HOME) {
    $c = Join-Path $env:JAVA_HOME 'bin\java.exe'
    if (Test-Path $c) { $java = $c }
}
if (Get-Command $java -ErrorAction SilentlyContinue) {
    & $java -version 2>&1 | ForEach-Object { Write-Host "  $_" }
} else {
    Write-Host "  MISSING (Rust 판만 쓸 거면 없어도 된다)"
}

Write-Host ""
Write-Host "== 저장 위치 =="
Write-Host "  $(Join-Path $env:USERPROFILE '.multi-ai-cli')"
Write-Host ""
