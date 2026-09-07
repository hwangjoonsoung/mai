#!/usr/bin/env bash
# multi_ai_cli 컴파일 (macOS/POSIX). SPEC D12 — 외부 Java 라이브러리를 쓰지 않는다.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
src="$root/src/main/java"
out="$root/out"
mkdir -p "$out"

javac_bin="javac"
if [[ -n "${JAVA_HOME:-}" && -x "$JAVA_HOME/bin/javac" ]]; then
    javac_bin="$JAVA_HOME/bin/javac"
fi

list="$(mktemp)"
trap 'rm -f "$list"' EXIT
find "$src" -name '*.java' > "$list"
echo "컴파일 대상 $(wc -l < "$list" | tr -d ' ') 개 파일"

"$javac_bin" -encoding UTF-8 -d "$out" "@$list"
echo "컴파일 완료 -> $out"
