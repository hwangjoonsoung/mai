//! 스키마를 통과한 검토자 응답 하나.

use serde::Deserialize;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Verdict {
    Agree,
    Concerns,
    Block,
    Unknown,
}

impl Verdict {
    pub fn label(self) -> &'static str {
        match self {
            Verdict::Agree => "AGREE",
            Verdict::Concerns => "CONCERNS",
            Verdict::Block => "BLOCK",
            Verdict::Unknown => "응답 없음",
        }
    }

    fn parse(s: &str) -> Self {
        match s.to_ascii_uppercase().as_str() {
            "AGREE" => Verdict::Agree,
            "CONCERNS" => Verdict::Concerns,
            "BLOCK" => Verdict::Block,
            _ => Verdict::Unknown,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub enum Severity {
    Critical,
    Major,
    Minor,
}

impl Severity {
    fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "critical" => Severity::Critical,
            "major" => Severity::Major,
            _ => Severity::Minor,
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Severity::Critical => "critical",
            Severity::Major => "major",
            Severity::Minor => "minor",
        }
    }

    /// 2라운드를 발동시키는 심각도인가.
    pub fn at_least_major(self) -> bool {
        matches!(self, Severity::Critical | Severity::Major)
    }
}

#[derive(Clone, Debug)]
pub struct Issue {
    pub id: String,
    pub severity: Severity,
    pub claim: String,
    pub rationale: String,
    pub suggestion: String,
}

#[derive(Clone, Debug)]
pub struct Review {
    pub reviewer_id: String,
    pub reviewer_name: String,
    pub verdict: Verdict,
    pub summary: String,
    pub issues: Vec<Issue>,
    pub open_questions: Vec<String>,
    pub note: String,
}

impl Review {
    pub fn valid(&self) -> bool {
        self.verdict != Verdict::Unknown
    }

    fn invalid(id: &str, name: &str, note: &str) -> Self {
        Self {
            reviewer_id: id.into(),
            reviewer_name: name.into(),
            verdict: Verdict::Unknown,
            summary: String::new(),
            issues: vec![],
            open_questions: vec![],
            note: note.into(),
        }
    }

    /// 공급자 원시 출력에서 구조화 응답을 뽑는다.
    ///
    /// agy·claude 는 `--output-format json` 에서 최상위에 `structured_output` 을
    /// 담는다. codex 는 `-o` 파일에 최종 메시지(=JSON)를 쓴다. 앞뒤에 배너가
    /// 붙을 수 있으므로 텍스트에서 객체를 찾아 파싱한다.
    pub fn parse(id: &str, name: &str, raw: &str) -> Self {
        let Some(root) = find_object(raw) else {
            return Self::invalid(id, name, "JSON 객체를 찾지 못했다");
        };
        // 래퍼가 있으면 그 안이 본체다.
        let body = root.get("structured_output").unwrap_or(&root).clone();

        let verdict = Verdict::parse(body.get("verdict").and_then(|v| v.as_str()).unwrap_or(""));
        if verdict == Verdict::Unknown {
            return Self::invalid(id, name, "verdict 가 스키마를 벗어났다");
        }

        let issues = body
            .get("issues")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .map(|m| Issue {
                        id: str_of(m, "id", "?"),
                        severity: Severity::parse(&str_of(m, "severity", "minor")),
                        claim: str_of(m, "claim", ""),
                        rationale: str_of(m, "rationale", ""),
                        suggestion: str_of(m, "suggestion", ""),
                    })
                    .collect()
            })
            .unwrap_or_default();

        let open_questions = body
            .get("open_questions")
            .and_then(|v| v.as_array())
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str())
                    .filter(|s| !s.trim().is_empty())
                    .map(String::from)
                    .collect()
            })
            .unwrap_or_default();

        Self {
            reviewer_id: id.into(),
            reviewer_name: name.into(),
            verdict,
            summary: str_of(&body, "summary", ""),
            issues,
            open_questions,
            note: String::new(),
        }
    }
}

fn str_of(v: &serde_json::Value, key: &str, def: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or(def)
        .to_string()
}

/// 텍스트 안에서 최상위 JSON 객체 하나를 찾는다.
///
/// CLI 가 JSON 앞뒤에 배너나 코드펜스를 붙이는 경우가 있어 통째 파싱이 실패한다.
fn find_object(text: &str) -> Option<serde_json::Value> {
    let bytes = text.as_bytes();
    let mut start = 0usize;
    while let Some(rel) = text[start..].find('{') {
        let from = start + rel;
        // 여는 중괄호부터 균형이 맞는 곳까지 잘라 파싱을 시도한다.
        let mut depth = 0i32;
        let mut in_str = false;
        let mut esc = false;
        for (i, &b) in bytes[from..].iter().enumerate() {
            if in_str {
                if esc {
                    esc = false;
                } else if b == b'\\' {
                    esc = true;
                } else if b == b'"' {
                    in_str = false;
                }
                continue;
            }
            match b {
                b'"' => in_str = true,
                b'{' => depth += 1,
                b'}' => {
                    depth -= 1;
                    if depth == 0 {
                        let slice = &text[from..from + i + 1];
                        if let Ok(v) = serde_json::from_str::<serde_json::Value>(slice) {
                            if v.is_object() {
                                return Some(v);
                            }
                        }
                        break;
                    }
                }
                _ => {}
            }
        }
        start = from + 1;
    }
    None
}

/// serde 파생이 필요 없는 최소 구조. 향후 확장 대비로 남겨둔다.
#[derive(Deserialize)]
#[allow(dead_code)]
struct RawReview {
    verdict: String,
    summary: String,
}
