//! 서브에이전트 관측.
//!
//! **프로세스 트리로는 볼 수 없다.** 실측 결과다 — Explore 서브에이전트 둘을
//! 띄우고 끝날 때까지 지켜봤는데 자손 프로세스는 하나도 늘지 않았다(오히려
//! 줄었다). 서브에이전트는 별도 프로세스가 아니라 같은 프로세스 안의 작업이다.
//!
//! 대신 에이전트가 **자기 화면 아래에 명부를 그린다.**
//!
//! ```text
//!   ● main
//!   ◯ Explore  Find all TOML files                9s · ↓ 26.5k tokens
//!   ◯ Explore  Count Rust source files            6s · ↓ 24.3k tokens
//! ```
//!
//! 그래서 화면에서 읽는다. 남의 TUI 를 읽는 것이라 형식이 바뀌면 깨진다.
//! 깨질 때 **아무것도 안 보이도록** 만들었다 — 틀린 것을 자신 있게 보여주는
//! 것보다 낫다. `● main` 이라는 정확한 앵커를 찾지 못하면 빈 목록을 준다.

/// 명부 한 줄.
#[derive(Clone, PartialEq, Eq)]
pub struct Sub {
    /// 에이전트 종류 (Explore, Plan, …)
    pub kind: String,
    /// 무엇을 시켰는지
    pub desc: String,
    /// 도는 중인가. ● 는 끝난 것, ◯ 는 도는 중이다.
    pub running: bool,
}

/// 명부의 시작을 알리는 줄. 이것을 못 찾으면 읽지 않는다.
const ANCHOR: &str = "● main";

/// 명부 줄의 첫 글자로 쓰이는 글리프들.
const GLYPHS: [char; 6] = ['●', '◯', '◉', '○', '◐', '◑'];

/// 화면에서 서브에이전트 명부를 읽는다.
///
/// 앵커 다음 줄부터 글리프로 시작하는 줄을 모은다. 글리프가 아닌 줄이 나오면
/// 거기서 멈춘다. 앵커가 여러 번 보이면 **마지막 것**을 쓴다 — 위쪽은 대화
/// 본문에 우연히 섞인 글자일 수 있고, 명부는 늘 화면 맨 아래에 있다.
pub fn scan(screen: &vt100::Screen) -> Vec<Sub> {
    let (rows, cols) = screen.size();
    let lines: Vec<String> = (0..rows)
        .map(|r| screen.contents_between(r, 0, r, cols))
        .collect();

    let Some(anchor) = lines.iter().rposition(|l| l.trim() == ANCHOR) else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for line in lines.iter().skip(anchor + 1) {
        let t = line.trim();
        let Some(first) = t.chars().next() else { break };
        if !GLYPHS.contains(&first) {
            break;
        }
        if let Some(s) = parse(t) {
            out.push(s);
        }
    }
    out
}

/// `◯ Explore  Find all TOML files          9s · ↓ 26.5k tokens` 한 줄을 쪼갠다.
///
/// 설명과 오른쪽 지표는 **두 칸 이상의 공백**으로 갈린다. 설명 안에는 한 칸
/// 공백만 오므로 이 경계는 안전하다.
fn parse(line: &str) -> Option<Sub> {
    let mut ch = line.chars();
    let glyph = ch.next()?;
    let rest = ch.as_str().trim_start();

    // 종류와 나머지를 가른다.
    let (kind, tail) = rest.split_once(char::is_whitespace)?;
    if kind.is_empty() {
        return None;
    }
    // 설명 — 두 칸 이상 공백 앞까지.
    let tail = tail.trim_start();
    let desc = match tail.find("  ") {
        Some(i) => &tail[..i],
        None => tail,
    };
    Some(Sub {
        kind: kind.to_string(),
        desc: desc.trim().to_string(),
        // ◯ 는 도는 중, ● 는 끝난 것이다.
        running: glyph != '●',
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn 명부를_읽는다() {
        let mut p = vt100::Parser::new(8, 100, 0);
        p.process(
            b"prose line\r\n\
              \x20 \xe2\x97\x8f main\r\n\
              \x20 \xe2\x97\xaf Explore  Find all TOML files            9s\r\n\
              \x20 \xe2\x97\xaf Explore  Count Rust source files        6s\r\n",
        );
        let s = scan(&p.screen());
        assert_eq!(s.len(), 2);
        assert_eq!(s[0].kind, "Explore");
        assert_eq!(s[0].desc, "Find all TOML files");
        assert!(s[0].running);
    }

    #[test]
    fn 앵커가_없으면_빈_목록() {
        let mut p = vt100::Parser::new(4, 60, 0);
        p.process("● I'll launch both Explore agents.\r\n".as_bytes());
        assert!(scan(&p.screen()).is_empty());
    }
}
