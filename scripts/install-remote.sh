#!/bin/sh
# Rust 없이 설치한다 (macOS · Linux). GitHub 최신 Release 의 실행 파일을 내려받는다.
#
#   curl -fsSL https://raw.githubusercontent.com/hwangjoonsoung/mai/main/scripts/install-remote.sh | sh
#
# 업데이트도 같은 한 줄이다. 놓는 자리는 install.sh 와 같다(~/.local/bin/mai).
# 소스 빌드 심링크가 있었다면 이 설치가 그 자리를 대신한다.
set -eu

case "$(uname -s)-$(uname -m)" in
    Darwin-arm64)  asset=mai-macos-arm64 ;;
    Darwin-x86_64) asset=mai-macos-x64 ;;
    Linux-x86_64)  asset=mai-linux-x64 ;;
    *) echo "지원하지 않는 환경이다: $(uname -s) $(uname -m) — 소스에서 빌드한다 (README §2)" >&2; exit 1 ;;
esac

url="https://github.com/hwangjoonsoung/mai/releases/latest/download/$asset"
dir="$HOME/.local/bin"
mkdir -p "$dir"
tmp="$dir/.mai.download.$$"
trap 'rm -f "$tmp"' EXIT

echo "내려받는 중: $url"
curl -fsSL "$url" -o "$tmp"
chmod +x "$tmp"
# curl 로 받으면 보통 격리 표시가 없지만, 있으면 Gatekeeper 가 막으므로 지운다.
if [ "$(uname -s)" = Darwin ]; then xattr -d com.apple.quarantine "$tmp" 2>/dev/null || true; fi
mv -f "$tmp" "$dir/mai"
echo "설치했다: $dir/mai"

case ":$PATH:" in
    *":$dir:"*) echo "끝. 아무 경로에서 'mai' 를 친다." ;;
    *)
        echo ""
        echo "$dir 가 PATH 에 없다. 셸 rc(~/.zshrc 등)에 아래를 넣고 새 터미널을 연다:"
        echo "  export PATH=\"\$HOME/.local/bin:\$PATH\""
        ;;
esac
