#!/usr/bin/env bash
# Rust 판 실행 (macOS/POSIX).
#
# 사용법:
#   ./scripts/rust-run.sh                       현재 디렉터리를 워크스페이스로 TUI 실행
#   ./scripts/rust-run.sh --workspace ~/proj    대상 워크스페이스 지정
#   ./scripts/rust-run.sh --selftest            PTY+VT 파이프라인 점검
#   ./scripts/rust-run.sh --which               참여자를 어떻게 띄우는지 확인
#   ./scripts/rust-run.sh --trust               현재 디렉터리를 각 에이전트에 신뢰 등록
#
# 크기는 앱이 crossterm 으로 직접 읽는다. Java 판처럼 tput 으로 넘길 필요가 없다.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
bin="$root/rust/target/debug/multi_ai_cli"

# release 가 있고 더 새것이면 그쪽을 쓴다.
rel="$root/rust/target/release/multi_ai_cli"
if [[ -x "$rel" && ( ! -x "$bin" || "$rel" -nt "$bin" ) ]]; then
    bin="$rel"
fi

if [[ ! -x "$bin" ]]; then
    echo "빌드 산출물이 없다. 먼저 빌드한다."
    "$(dirname "${BASH_SOURCE[0]}")/rust-build.sh"
    bin="$root/rust/target/debug/multi_ai_cli"
fi

# 터미널이 UTF-8 이 아니면 한글이 깨진다. macOS 기본은 UTF-8 이지만 명시한다.
export LANG="${LANG:-en_US.UTF-8}"
export LC_ALL="${LC_ALL:-$LANG}"

workspace=""
args=()
while [[ $# -gt 0 ]]; do
    case "$1" in
        --workspace) workspace="$2"; shift 2 ;;
        *) args+=("$1"); shift ;;
    esac
done

# 앱은 자기 cwd 를 기본 공간으로 삼는다. 워크스페이스 지정은 곧 cwd 변경이다.
[[ -n "$workspace" ]] && cd "$workspace"

exec "$bin" "${args[@]+"${args[@]}"}"
