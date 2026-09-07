//! 에이전트가 띄운 «대화상자»가 화면에 있는지 본다.
//!
//! 왜 필요한가 — 우리는 첫 질문을 **키 입력으로 주입한다.** 그런데 새 경로에서
//! 처음 뜨는 에이전트는 신뢰 여부나 MCP 서버 사용을 먼저 묻는다. 그 상태로
//! 주입하면 이런 일이 난다(실측).
//!
//! ```text
//!   주입 직전:  ❯ Continue without using this MCP server
//!               Enter to confirm · Esc to cancel
//!   주입 6초 후: ❯ Try "fix typecheck errors"      ← 질문이 통째로 사라졌다
//! ```
//!
//! 우리가 보낸 개행이 사용자 대신 항목을 확정해 버렸고, 질문은 삼켜졌다.
//! 사용자 입장에서는 「화살표와 Enter 가 안 먹는」 것으로 보인다 — 누르기도 전에
//! 상자가 닫혀 있기 때문이다.
//!
//! 그래서 상자가 떠 있는 동안에는 주입을 미룬다. 사용자가 직접 답하게 둔다.
//!
//! 판정은 상자 바닥의 안내 문구로 한다. 이것도 남의 TUI 를 읽는 일이라 형식이
//! 바뀌면 못 알아본다. 그때는 **예전 동작(바로 주입)으로 돌아갈 뿐** 새 고장이
//! 생기지는 않는다.

/// 상자 바닥에 붙는 안내들. 하나라도 보이면 상자가 떠 있다고 본다.
const MARKERS: [&str; 5] = [
    "Enter to confirm",
    "Esc to cancel",
    "Do you trust",
    "esc to cancel",
    "to interrupt",
];

/// 안내 문구가 화면에 있는가.
///
/// `to interrupt` 는 «작업 중» 표시라 상자는 아니지만, 그때도 주입하면 안 된다.
/// 이미 무언가 돌고 있다는 뜻이기 때문이다.
pub fn open(screen: &vt100::Screen) -> bool {
    let (rows, cols) = screen.size();
    (0..rows).any(|r| {
        let line = screen.contents_between(r, 0, r, cols);
        MARKERS.iter().any(|m| line.contains(m))
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn screen(text: &str) -> vt100::Parser {
        let mut p = vt100::Parser::new(10, 120, 0);
        for line in text.lines() {
            p.process(line.as_bytes());
            p.process(b"\r\n");
        }
        p
    }

    #[test]
    fn 확인_안내가_보이면_상자로_본다() {
        let p = screen(
            "  New MCP server found in this project: foo\n\
             \x20 > Continue without using this MCP server\n\
             \x20 Enter to confirm · Esc to cancel\n",
        );
        assert!(open(&p.screen()));
    }

    #[test]
    fn 평범한_화면은_상자가_아니다() {
        let p = screen("❯ Try \"fix typecheck errors\"\n  auto mode on\n");
        assert!(!open(&p.screen()));
    }
}
