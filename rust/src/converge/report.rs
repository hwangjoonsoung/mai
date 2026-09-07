//! REPORT.md 생성.
//!
//! 형식의 참조 구현은 수동 예행연습 산출물인 REVIEW_REPORT.md 다. 거기서 얻은
//! 두 규칙을 유지한다.
//!   1. 기각한 지적도 이유와 함께 남긴다.
//!   2. 수렴자는 판정하지 않는다. 분류해서 사용자 앞에 놓는 것까지가 역할이다.

use super::engine::{Bucket, Outcome};
use anyhow::Result;
use std::path::{Path, PathBuf};

pub fn write(dir: &Path, subject: &str, r1: &Outcome, r2: Option<&Outcome>) -> Result<PathBuf> {
    std::fs::create_dir_all(dir)?;
    let f = dir.join("REPORT.md");
    std::fs::write(&f, render(subject, r1, r2))?;
    Ok(f)
}

pub fn render(subject: &str, r1: &Outcome, r2: Option<&Outcome>) -> String {
    let last = r2.unwrap_or(r1);
    let mut b = String::new();

    b.push_str("# REPORT — 교차검토 수렴\n\n");
    b.push_str(&format!("- 라운드: {}\n", if r2.is_some() { 2 } else { 1 }));
    let names: Vec<&str> = r1.reviews.iter().map(|r| r.reviewer_name.as_str()).collect();
    b.push_str(&format!("- 검토자: {}\n", names.join(", ")));

    // 두 라운드의 실패를 합쳐 알린다. r1 만 보면 2라운드 실패를 놓친다.
    let mut failed = r1.failed.clone();
    if let Some(x) = r2 {
        failed.extend(x.failed.clone());
    }
    failed.dedup();
    if !failed.is_empty() {
        b.push_str(&format!("- **PARTIAL — 응답 없음: {}**\n", failed.join(", ")));
    }
    b.push('\n');

    b.push_str("## 0. 판정 요약\n\n");
    b.push_str("| 검토자 | verdict | 핵심 우려 |\n|---|:---:|---|\n");
    for r in &last.reviews {
        let note = if r.valid() { &r.summary } else { &r.note };
        b.push_str(&format!(
            "| {} | {} | {} |\n",
            r.reviewer_name,
            r.verdict.label(),
            one_line(note)
        ));
    }
    b.push('\n');

    b.push_str("## 1. 안건\n\n");
    b.push_str(subject);
    b.push_str("\n\n");

    section(&mut b, "2. 합의 — 그대로 반영", last, Bucket::Agreed);
    section(&mut b, "3. 이견 — 판단 상충", last, Bucket::Disputed);
    section(&mut b, "4. 단독 지적", last, Bucket::Solo);

    b.push_str("## 5. 미해결 — 사용자 결정 필요\n\n");
    let mut unresolved = last.open_questions.clone();
    if let Some(x) = r2 {
        for f in x.findings.iter().filter(|f| f.bucket == Bucket::Disputed) {
            unresolved.push(format!("2라운드 후에도 좁혀지지 않음: {}", f.claim));
        }
    } else if r1.needs_round2 {
        unresolved.push(format!(
            "2라운드가 필요하나 실행되지 않았다 — {}",
            r1.round2_reason
        ));
    }
    if unresolved.is_empty() {
        b.push_str("없음.\n\n");
    } else {
        for q in &unresolved {
            b.push_str(&format!("- {q}\n"));
        }
        b.push('\n');
    }

    if let Some(x) = r2 {
        b.push_str("## 6. 2라운드 반론 결과\n\n");
        b.push_str(&format!("사유: {}\n\n", r1.round2_reason));
        for r in &x.reviews {
            b.push_str(&format!("### {} — {}\n\n", r.reviewer_name, r.verdict.label()));
            if r.valid() {
                b.push_str(&r.summary);
                b.push_str("\n\n");
            }
        }
    }

    b.push_str("---\n\n");
    b.push_str("> 이 보고서는 분류만 한다. **어느 쪽이 옳은지 판정하지 않는다.**\n");
    b.push_str("> 미해결 항목은 사용자가 결정한다. 최대 2라운드까지만 반론한다.\n");
    b
}

fn section(b: &mut String, title: &str, o: &Outcome, bucket: Bucket) {
    b.push_str(&format!("## {title}\n\n"));
    let mut fs: Vec<_> = o.findings.iter().filter(|f| f.bucket == bucket).collect();
    fs.sort_by_key(|f| f.severity);
    if fs.is_empty() {
        b.push_str("없음.\n\n");
        return;
    }
    for f in fs {
        b.push_str(&format!("### [{}] {}\n\n", f.severity.label(), f.claim));
        b.push_str(&format!("- 제기: {}\n", f.by.join(", ")));
        for (who, is) in &f.details {
            b.push_str(&format!("- **{}** — {}\n", who, one_line(&is.rationale)));
            if !is.suggestion.trim().is_empty() {
                b.push_str(&format!("  - 제안: {}\n", one_line(&is.suggestion)));
            }
        }
        b.push('\n');
    }
}

fn one_line(s: &str) -> String {
    s.replace('\n', " ").replace('|', "/").trim().to_string()
}
