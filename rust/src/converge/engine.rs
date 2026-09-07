//! 분류와 수렴 판정.
//!
//! **수렴자는 판정하지 않는다.** 두 의견을 분류하고 쟁점을 좁혀 사용자 앞에 놓는
//! 것까지가 역할이다. 어느 쪽이 옳은지 단정하면 3자 구도의 의미가 사라진다.
//! **기각한 지적도 이유와 함께 남긴다** — 조용히 빠뜨리면 다음 라운드에 또 올라온다.

use super::review::{Issue, Review, Severity, Verdict};
use std::collections::BTreeMap;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Bucket {
    /// 둘 이상이 같은 쟁점을 제기하고 방향이 일치
    Agreed,
    /// 같은 쟁점에 판단이 상충
    Disputed,
    /// 한쪽만 제기
    Solo,
}

impl Bucket {
    pub fn label(self) -> &'static str {
        match self {
            Bucket::Agreed => "합의",
            Bucket::Disputed => "이견",
            Bucket::Solo => "단독 지적",
        }
    }
}

#[derive(Clone, Debug)]
pub struct Finding {
    pub bucket: Bucket,
    pub claim: String,
    pub severity: Severity,
    pub by: Vec<String>,
    /// 검토자 id -> 그가 쓴 issue
    pub details: Vec<(String, Issue)>,
}

pub struct Outcome {
    pub reviews: Vec<Review>,
    pub findings: Vec<Finding>,
    pub open_questions: Vec<String>,
    pub needs_round2: bool,
    pub round2_reason: String,
    pub failed: Vec<String>,
}

impl Outcome {
    pub fn partial(&self) -> bool {
        !self.failed.is_empty()
    }

    /// 전원 AGREE 이고 지적이 없으면 즉시 종료한다.
    pub fn unanimous_agree(&self) -> bool {
        !self.reviews.is_empty()
            && self.reviews.iter().all(|r| r.verdict == Verdict::Agree)
            && self.reviews.iter().all(|r| r.issues.is_empty())
    }

    pub fn count(&self, b: Bucket) -> usize {
        self.findings.iter().filter(|f| f.bucket == b).count()
    }
}

pub fn consolidate(all: Vec<Review>, failed: Vec<String>) -> Outcome {
    let valid: Vec<&Review> = all.iter().filter(|r| r.valid()).collect();

    // 같은 쟁점을 claim 의 정규화 키로 묶는다. 완전 자동 매칭은 불가능하므로
    // 겹치지 않으면 단독 지적으로 남기고 사용자가 판단하게 둔다.
    let mut by_key: BTreeMap<String, (String, Severity, Vec<String>, Vec<(String, Issue)>)> =
        BTreeMap::new();
    for r in &valid {
        for is in &r.issues {
            let key = normalize(&is.claim);
            let e = by_key.entry(key).or_insert_with(|| {
                (is.claim.clone(), is.severity, Vec::new(), Vec::new())
            });
            if e.1 > is.severity {
                e.1 = is.severity; // 더 심각한 쪽으로
            }
            if !e.2.contains(&r.reviewer_id) {
                e.2.push(r.reviewer_id.clone());
            }
            e.3.push((r.reviewer_id.clone(), is.clone()));
        }
    }

    let mut findings: Vec<Finding> = by_key
        .into_values()
        .map(|(claim, sev, by, details)| Finding {
            bucket: if by.len() >= 2 { Bucket::Agreed } else { Bucket::Solo },
            claim,
            severity: sev,
            by,
            details,
        })
        .collect();

    // 판정이 갈리면 그 자체를 최우선 이견으로 올린다. 평균 내지 않는다.
    let mut verdicts: Vec<Verdict> = valid.iter().map(|r| r.verdict).collect();
    verdicts.sort_by_key(|v| v.label());
    verdicts.dedup();
    if verdicts.len() > 1 {
        let labels: Vec<&str> = verdicts.iter().map(|v| v.label()).collect();
        findings.insert(
            0,
            Finding {
                bucket: Bucket::Disputed,
                claim: format!("판정이 갈렸다: {}", labels.join(" / ")),
                severity: Severity::Major,
                by: valid.iter().map(|r| r.reviewer_id.clone()).collect(),
                details: valid
                    .iter()
                    .map(|r| {
                        (
                            r.reviewer_id.clone(),
                            Issue {
                                id: "verdict".into(),
                                severity: Severity::Major,
                                claim: format!("판정 {}", r.verdict.label()),
                                rationale: r.summary.clone(),
                                suggestion: String::new(),
                            },
                        )
                    })
                    .collect(),
            },
        );
    }

    let open_questions: Vec<String> = valid
        .iter()
        .flat_map(|r| {
            r.open_questions
                .iter()
                .map(move |q| format!("{}: {}", r.reviewer_name, q))
        })
        .collect();

    // 2라운드 조건 — 이견, 또는 critical·major 단독 지적
    let disagreement = findings.iter().any(|f| f.bucket == Bucket::Disputed);
    let major_solo = findings
        .iter()
        .any(|f| f.bucket == Bucket::Solo && f.severity.at_least_major());
    let reason = if disagreement {
        "판정 또는 쟁점에 이견이 있다"
    } else if major_solo {
        "critical·major 단독 지적이 있다"
    } else {
        ""
    };

    Outcome {
        needs_round2: (disagreement || major_solo) && valid.len() >= 2,
        round2_reason: reason.to_string(),
        reviews: all,
        findings,
        open_questions,
        failed,
    }
}

/// claim 매칭용 정규화. 공백·구두점·대소문자를 무시한다.
fn normalize(s: &str) -> String {
    let t: String = s
        .to_lowercase()
        .chars()
        .filter(|c| c.is_alphanumeric())
        .collect();
    t.chars().take(60).collect()
}
