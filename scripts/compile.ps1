# multi_ai_cli 컴파일. SPEC D12 — 외부 Java 라이브러리를 쓰지 않는다.
# 사용법:  .\scripts\compile.ps1

$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$src  = Join-Path $root 'src\main\java'
$out  = Join-Path $root 'out'

if (-not (Test-Path $out)) { New-Item -ItemType Directory -Path $out | Out-Null }

$javac = 'javac'
if ($env:JAVA_HOME) {
    $candidate = Join-Path $env:JAVA_HOME 'bin\javac.exe'
    if (Test-Path $candidate) { $javac = $candidate }
}

$files = Get-ChildItem -Path $src -Filter *.java -Recurse | ForEach-Object { $_.FullName }
Write-Host "컴파일 대상 $($files.Count) 개 파일"

$listFile = Join-Path $env:TEMP "multiai-sources.txt"
# BOM 없이 써야 한다. Windows PowerShell 5.1 의 Out-File -Encoding utf8 은 BOM 을
# 붙이는데, javac 는 @목록파일의 BOM 을 첫 파일명의 일부로 읽어 "Invalid filename" 이 된다.
[System.IO.File]::WriteAllLines($listFile, $files, (New-Object System.Text.UTF8Encoding $false))

& $javac -encoding UTF-8 -d $out "@$listFile"
if ($LASTEXITCODE -ne 0) {
    Remove-Item $listFile -ErrorAction SilentlyContinue
    throw "컴파일 실패 (exit $LASTEXITCODE)"
}
Remove-Item $listFile -ErrorAction SilentlyContinue
Write-Host "컴파일 완료 -> $out"
