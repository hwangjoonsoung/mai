#!/usr/bin/env bash
# 착수 전 환경 점검 (macOS/POSIX). SPEC §7.10 완료 기준의 /status 를 셸에서 미리 확인한다.
set -uo pipefail

echo
echo "== CLI 설치 확인 =="
for name in claude codex agy; do
    if path="$(command -v "$name" 2>/dev/null)"; then
        printf '  %-8s %s\n' "$name" "$path"
        printf '  %-8s %s\n' '' "$("$name" --version 2>&1 | head -1)"
    else
        printf '  %-8s MISSING\n' "$name"
    fi
done

echo
echo "== Codex 실행 경로 (SPEC §6.3) =="
# 셸 래퍼는 쓰지 않는다. 네이티브 바이너리 또는 node + codex.js 만 쓴다.
found=""
for root in /opt/homebrew/lib /usr/local/lib "$HOME/.npm-global/lib"; do
    base="$root/node_modules/@openai/codex"
    [[ -d "$base/node_modules" ]] || continue
    hit="$(find "$base/node_modules" -maxdepth 8 -name codex -type f -perm -111 -path '*/vendor/*' 2>/dev/null | head -1)"
    if [[ -n "$hit" ]]; then echo "  tier 1  네이티브: $hit"; found=1; break; fi
    if [[ -f "$base/bin/codex.js" ]]; then echo "  tier 2  node + $base/bin/codex.js"; found=1; break; fi
done
[[ -n "$found" ]] || echo "  tier 3  지원 불가 — Codex 참여자는 비활성된다"

echo
echo "== agy 사용 가능 모델 (SPEC D13) =="
if command -v agy >/dev/null 2>&1; then
    models="$(agy models 2>&1 | grep '^gemini-' | head -12 || true)"
    if [[ -n "$models" ]]; then echo "$models" | sed 's/^/  /'
    else echo "  모델 조회 실패 — 인증 상태를 확인하라"; fi
else
    echo "  agy MISSING"
fi

echo
echo "== Rust 툴체인 (현행 구현) =="
if ! command -v cargo >/dev/null 2>&1 && [[ -f "$HOME/.cargo/env" ]]; then
    # shellcheck disable=SC1091
    source "$HOME/.cargo/env"
fi
if command -v cargo >/dev/null 2>&1; then
    printf '  %-8s %s\n' cargo "$(cargo --version)"
    printf '  %-8s %s\n' rustc "$(rustc --version 2>&1)"
else
    echo "  MISSING — Rust 판을 빌드할 수 없다. 설치:"
    echo "    curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh"
fi

echo
echo "== Java (참조용 Java 판) =="
java_bin="java"
[[ -n "${JAVA_HOME:-}" && -x "$JAVA_HOME/bin/java" ]] && java_bin="$JAVA_HOME/bin/java"
if command -v "$java_bin" >/dev/null 2>&1; then
    "$java_bin" -version 2>&1 | sed 's/^/  /'
else
    echo "  MISSING (Rust 판만 쓸 거면 없어도 된다)"
fi

echo
echo "== 저장 위치 =="
echo "  $HOME/.multi-ai-cli"
echo
