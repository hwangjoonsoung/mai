#!/usr/bin/env bash
# Rust 판 빌드 (macOS/POSIX). REBUILD.md 기준의 현행 구현이다.
#
# 사용법: ./scripts/rust-build.sh [--release]
#
# Java 판(compile.sh)과 달리 cargo 하나면 된다. rustup 으로 깐 툴체인은
# PATH 에 없을 수 있어(`~/.cargo/env` 를 안 읽은 셸) 여기서 직접 끌어온다.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

if ! command -v cargo >/dev/null 2>&1; then
    if [[ -f "$HOME/.cargo/env" ]]; then
        # shellcheck disable=SC1091
        source "$HOME/.cargo/env"
    fi
fi
if ! command -v cargo >/dev/null 2>&1; then
    echo "cargo 가 없다. 먼저 Rust 툴체인을 깐다:" >&2
    echo "  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh" >&2
    exit 1
fi

cd "$root/rust"
cargo build "$@"

profile="debug"
for a in "$@"; do [[ "$a" == "--release" ]] && profile="release"; done
echo "빌드 완료 -> $root/rust/target/$profile/multi_ai_cli"
