#!/usr/bin/env bash
# multi_ai_cli 실행 (macOS/POSIX).
# 사용법: ./scripts/run.sh [--workspace <path>] [--room <id>]
#
# macOS 터미널은 기본이 UTF-8 이라 chcp 에 해당하는 처리가 필요 없다.
# 다만 LANG 이 비어 있으면 Java 가 US-ASCII 로 떨어질 수 있어 명시한다.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
out="$root/out"

if [[ ! -f "$out/io/multiai/cli/Main.class" ]]; then
    echo "빌드 산출물이 없다. 먼저 컴파일한다."
    "$(dirname "${BASH_SOURCE[0]}")/compile.sh"
fi

export LANG="${LANG:-en_US.UTF-8}"
export LC_ALL="${LC_ALL:-$LANG}"

java_bin="java"
if [[ -n "${JAVA_HOME:-}" && -x "$JAVA_HOME/bin/java" ]]; then
    java_bin="$JAVA_HOME/bin/java"
fi

workspace="$(pwd)"
args=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --workspace) workspace="$2"; shift 2 ;;
        *) args+=("$1"); shift ;;
    esac
done

# TUI 는 터미널 크기를 알아야 칸을 나눈다. JVM 이 직접 알 수 없으므로 여기서 넘긴다.
cols="$(tput cols 2>/dev/null || echo 120)"
rows="$(tput lines 2>/dev/null || echo 30)"

exec "$java_bin" \
    -Dterm.cols="$cols" \
    -Dterm.rows="$rows" \
    -Dfile.encoding=UTF-8 \
    -Dsun.stdout.encoding=UTF-8 \
    -Dsun.stderr.encoding=UTF-8 \
    -cp "$out" io.multiai.cli.Main \
    --workspace "$workspace" "${args[@]+"${args[@]}"}"
