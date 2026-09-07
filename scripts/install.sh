#!/usr/bin/env bash
# `mai` 한 단어로 어디서든 띄울 수 있게 설치한다.
#
#   ./scripts/install.sh            ~/.local/bin/mai 심링크 (권장)
#   ./scripts/install.sh --alias    셸 rc 에 alias 추가
#   ./scripts/install.sh --uninstall
#
# 심링크를 기본으로 삼는다. alias 는 대화형 셸에서만 살아 있어 스크립트나
# 다른 도구가 부를 때 없는 것이 되지만, 심링크는 PATH 에 있는 실제 명령이다.
set -euo pipefail

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
target="$root/scripts/mai"
link_dir="$HOME/.local/bin"
link="$link_dir/mai"

# 로그인 셸에 맞는 rc 를 고른다.
case "${SHELL:-}" in
    */zsh) rc="$HOME/.zshrc" ;;
    */bash) rc="$HOME/.bashrc" ;;
    *) rc="$HOME/.profile" ;;
esac
marker="# multi_ai_cli"

case "${1:-}" in
--uninstall)
    [[ -L "$link" ]] && rm -f "$link" && echo "지웠다: $link"
    if [[ -f "$rc" ]] && grep -q "$marker" "$rc"; then
        # 마커가 붙은 줄과 그 다음 alias 줄을 지운다.
        tmp="$(mktemp)"
        grep -v -e "$marker" -e "^alias mai=" "$rc" > "$tmp"
        mv "$tmp" "$rc"
        echo "지웠다: $rc 의 alias"
    fi
    exit 0
    ;;
--alias)
    line="alias mai='$target'"
    if [[ -f "$rc" ]] && grep -q "^alias mai=" "$rc"; then
        echo "이미 있다: $rc"
    else
        printf '\n%s\n%s\n' "$marker" "$line" >> "$rc"
        echo "추가했다: $rc"
    fi
    echo "새 셸을 열거나  source $rc  를 실행한다."
    exit 0
    ;;
esac

mkdir -p "$link_dir"
ln -sf "$target" "$link"
echo "설치했다: $link -> $target"

# PATH 에 없으면 알려 준다. 있는 줄 알고 헤매는 게 제일 나쁘다.
case ":${PATH}:" in
    *":$link_dir:"*) echo "PATH 에 있다. 새 셸에서 'mai' 로 바로 띄운다." ;;
    *)
        echo
        echo "$link_dir 가 PATH 에 없다. rc 에 아래를 넣는다:"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        echo "($rc)"
        ;;
esac
