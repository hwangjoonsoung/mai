#!/usr/bin/env python3
"""Claude Code Stop 훅 — mai 안에서 검증 없이 턴을 끝내려 하면 한 번 막는다.

prompts/verify.md 는 «답변 초안이나 구현이 준비되면 mai ctl ask 로 검증을
요청한다» 고 지시한다. 이 훅은 그 지시를 잊었을 때를 위한 뒷단이다.

판정:
  - MAI_CTL 이 없거나(=mai 밖) MAI_ROLE=verifier(=자기가 검증자)면 관여하지 않는다.
  - stop_hook_active 면(이미 이 훅이 한 번 막아 이어진 턴) 다시 막지 않는다.
  - 이번 턴(마지막 사용자 프롬프트 이후)에 파일 편집이 없고 마지막 답도 짧으면
    잡담으로 보고 막지 않는다.
  - 그 밖에는 `mai ctl ask` 가 성공할 때 남기는 마커
    (<MAI_CTL_DIR>/verified-<MAI_PANE>) 가 이번 턴 안에 갱신됐어야 한다.
    아니면 {"decision":"block","reason":…} 으로 막는다.

mai ctl ask 가 0 이 아닌 코드로 끝나면 마커를 남기지 않으므로, 그때는 지침대로
«검증하지 못했다» 고 적고 끝내야 하는데 이 훅이 한 번 더 막을 수 있다 — 두 번째
시도는 stop_hook_active 라 통과한다.
"""
import json, os, sys, datetime

EDIT_TOOLS = {"Edit", "Write", "MultiEdit", "NotebookEdit"}
LONG_ANSWER = 400


def main():
    ctl = os.environ.get("MAI_CTL")
    pane = os.environ.get("MAI_PANE")
    if not ctl or not pane or os.environ.get("MAI_ROLE") == "verifier":
        return allow()
    try:
        data = json.load(sys.stdin)
    except Exception:
        return allow()
    if data.get("stop_hook_active"):
        return allow()
    path = data.get("transcript_path")
    if not path or not os.path.isfile(path):
        return allow()
    prompt_at, edited, last_len = scan(path)
    if prompt_at is None:
        return allow()
    if not edited and last_len < LONG_ANSWER:
        return allow()
    marker = os.path.join(os.environ.get("MAI_CTL_DIR") or os.path.dirname(ctl), f"verified-{pane}")
    try:
        if os.path.getmtime(marker) >= prompt_at:
            return allow()
    except OSError:
        pass
    reason = ("mai 검증 지침: 최종 응답 전에 `mai ctl ask <다른 에이전트> \"<검증할 내용>\"` 으로 "
              "검증을 요청하고 결과를 반영하라. 종료 코드가 0 이 아니면 «검증하지 못했다» 고 명시하라. "
              "(hooks/mai-verify-stop.py)")
    print(json.dumps({"decision": "block", "reason": reason}, ensure_ascii=False))
    return 0


def allow():
    return 0


def parse_ts(s):
    try:
        return datetime.datetime.fromisoformat(s.replace("Z", "+00:00")).timestamp()
    except Exception:
        return None


def scan(path):
    """마지막 사용자 프롬프트의 시각, 그 뒤 편집 도구 사용 여부, 마지막 답 길이."""
    prompt_at, edited, last_len = None, False, 0
    with open(path, encoding="utf-8", errors="replace") as f:
        for line in f:
            try:
                d = json.loads(line)
            except Exception:
                continue
            t = d.get("type")
            msg = d.get("message") or {}
            content = msg.get("content")
            if t == "user" and not d.get("isMeta"):
                is_prompt = isinstance(content, str) or (isinstance(content, list) and not any(isinstance(c, dict) and c.get("type") == "tool_result" for c in content))
                if is_prompt:
                    ts = parse_ts(d.get("timestamp", "")) or 0
                    prompt_at, edited, last_len = ts, False, 0
            elif t == "assistant" and prompt_at is not None and isinstance(content, list):
                for c in content:
                    if not isinstance(c, dict):
                        continue
                    if c.get("type") == "tool_use" and c.get("name") in EDIT_TOOLS:
                        edited = True
                    if c.get("type") == "text":
                        last_len = len(c.get("text") or "")
    return prompt_at, edited, last_len


if __name__ == "__main__":
    sys.exit(main())
