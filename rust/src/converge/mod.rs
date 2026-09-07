//! 구조화 수렴. INTENT.md §2 가 말하는 이 프로젝트의 결승선이다.
//!
//! 흐름
//!   1라운드 — 검토자들이 서로 못 본 상태에서 동일 스키마로 독립 답변
//!   분류    — 합의 / 이견 / 단독 지적 / 미해결
//!   2라운드 — 이견 또는 critical·major 단독 지적이 있을 때만. 최대 1회
//!   보고서  — REPORT.md
//!
//! **수렴자로 지명된 참여자는 검토 대상에서 제외한다.** 자기 답을 자기가
//! 분류하면 자기 채점이 된다. 지명이 없으면 규칙 기반 분류만 한다.

pub mod engine;
pub mod headless;
pub mod report;
pub mod review;
pub mod schema;

use anyhow::Result;
use engine::Outcome;
use review::Review;
use std::path::{Path, PathBuf};

pub struct Progress<'a> {
    pub stage: &'a dyn Fn(&str),
    pub done: &'a dyn Fn(&Review),
}

pub struct Session<'a> {
    /// (id, 표시명) 목록. 수렴자는 호출부에서 이미 제외돼 있다.
    pub reviewers: Vec<(String, String)>,
    pub workspace: PathBuf,
    pub out_dir: PathBuf,
    pub agy_model: Option<&'a str>,
}

pub struct Result_ {
    pub round1: Option<Outcome>,
    pub round2: Option<Outcome>,
    pub report: Option<PathBuf>,
    pub aborted: Option<String>,
}

impl<'a> Session<'a> {
    pub fn run(&self, subject: &str, p: &Progress) -> Result<Result_> {
        if self.reviewers.len() < 2 {
            return Ok(Result_ {
                round1: None,
                round2: None,
                report: None,
                aborted: Some("검토자가 2명 미만이다. 수렴에는 최소 2명이 필요하다.".into()),
            });
        }
        let temp = headless::temp_dir();
        let schema_file = schema::write_to(&temp)?;

        (p.stage)(&format!("1라운드 — {}명 독립 검토", self.reviewers.len()));
        let r1 = self.round(&schema::round1(subject), &schema_file, &temp, p)?;

        if r1.reviews.iter().all(|r| !r.valid()) {
            // 전원 실패면 중단한다. 분류를 지어내지 않는다.
            return Ok(Result_ {
                round1: Some(r1),
                round2: None,
                report: None,
                aborted: Some("전원 응답 실패 — 인증·쿼터·네트워크를 확인하라.".into()),
            });
        }
        if r1.unanimous_agree() {
            (p.stage)("전원 AGREE 이고 지적이 없다 — 즉시 종료 (2라운드 없음)");
            let rep = report::write(&self.out_dir, subject, &r1, None)?;
            return Ok(Result_ { round1: Some(r1), round2: None, report: Some(rep), aborted: None });
        }

        let mut r2 = None;
        if r1.needs_round2 {
            (p.stage)(&format!("2라운드 — {}", r1.round2_reason));
            r2 = Some(self.rebuttal(subject, &r1, &schema_file, &temp, p)?);
        } else {
            (p.stage)("2라운드 조건 미충족 — 1라운드로 종료");
        }

        let rep = report::write(&self.out_dir, subject, &r1, r2.as_ref())?;
        Ok(Result_ { round1: Some(r1), round2: r2, report: Some(rep), aborted: None })
    }

    /// 한 라운드. 스키마 위반 시 해당 참여자만 1회 재시도한다.
    fn round(&self, prompt: &str, schema: &Path, temp: &Path, p: &Progress) -> Result<Outcome> {
        let mut reviews = Vec::new();
        let mut failed = Vec::new();

        // 병렬로 부른다. 하나가 늦어도 나머지를 막지 않는다.
        let calls: Vec<headless::Call> = std::thread::scope(|s| {
            let hs: Vec<_> = self
                .reviewers
                .iter()
                .map(|(id, name)| {
                    let (id, name) = (id.clone(), name.clone());
                    let ws = self.workspace.clone();
                    let (schema, temp) = (schema.to_path_buf(), temp.to_path_buf());
                    let prompt = prompt.to_string();
                    let model = self.agy_model.map(String::from);
                    s.spawn(move || {
                        headless::invoke(
                            &id, &name, &prompt, &ws, &schema, &temp, model.as_deref(),
                        )
                    })
                })
                .collect();
            hs.into_iter().filter_map(|h| h.join().ok()).collect()
        });

        for c in calls {
            self.dump_raw(&c, "r1");
            let mut sr = to_review(&c);
            if !sr.valid() {
                (p.stage)(&format!("{} 응답이 스키마를 벗어났다 — 1회 재시도", c.name));
                let retry = headless::invoke(
                    &c.id, &c.name, prompt, &self.workspace, schema, temp, self.agy_model,
                );
                self.dump_raw(&retry, "r1-retry");
                sr = to_review(&retry);
            }
            if sr.valid() {
                (p.done)(&sr);
            } else {
                let path = self.out_dir.join(format!("{}.r1.raw.txt", c.id));
                failed.push(format!("{} ({}) — 원문 {}", sr.reviewer_name, sr.note, path.display()));
            }
            reviews.push(sr);
        }
        Ok(engine::consolidate(reviews, failed))
    }

    /// 2라운드. 각 검토자에게 **상대의 의견만** 첨부한다 — 비대칭 문맥이다.
    fn rebuttal(
        &self,
        subject: &str,
        r1: &Outcome,
        schema: &Path,
        temp: &Path,
        p: &Progress,
    ) -> Result<Outcome> {
        let mut reviews = Vec::new();
        let mut failed = Vec::new();
        for (id, name) in &self.reviewers {
            let opposing = render_others(r1, id);
            if opposing.trim().is_empty() {
                continue;
            }
            let prompt = schema::round2(subject, &opposing);
            let c = headless::invoke(
                id, name, &prompt, &self.workspace, schema, temp, self.agy_model,
            );
            let sr = to_review(&c);
            if sr.valid() {
                (p.done)(&sr);
            } else {
                failed.push(format!("{} ({})", sr.reviewer_name, sr.note));
            }
            reviews.push(sr);
        }
        Ok(engine::consolidate(reviews, failed))
    }
}

impl<'a> Session<'a> {
    /// 공급자 원시 출력을 그대로 남긴다.
    ///
    /// 파싱이 실패했을 때 무엇이 왔는지 볼 수 없으면 원인을 좁힐 수 없다.
    /// 성공해도 남긴다 — 나중에 형식이 바뀌면 대조할 근거가 된다.
    fn dump_raw(&self, c: &headless::Call, tag: &str) {
        let _ = std::fs::create_dir_all(&self.out_dir);
        let path = self.out_dir.join(format!("{}.{tag}.raw.txt", c.id));
        let body = match &c.failure {
            Some(f) => format!("[실행 실패] {f}"),
            None => c.text.clone(),
        };
        let _ = std::fs::write(path, body);
    }
}

fn to_review(c: &headless::Call) -> Review {
    if let Some(f) = &c.failure {
        return Review::parse(&c.id, &c.name, &format!("실행 실패: {f}"));
    }
    Review::parse(&c.id, &c.name, &c.text)
}

/// 자기 자신을 뺀 나머지 검토자의 의견을 사람이 읽을 형태로.
fn render_others(r1: &Outcome, self_id: &str) -> String {
    let mut b = String::new();
    for r in &r1.reviews {
        if r.reviewer_id == self_id || !r.valid() {
            continue;
        }
        b.push_str(&format!("- 판정: {}\n", r.verdict.label()));
        b.push_str(&format!("- 요약: {}\n", r.summary));
        for is in &r.issues {
            b.push_str(&format!(
                "- [{}] {}\n  근거: {}\n",
                is.severity.label(),
                is.claim,
                is.rationale
            ));
        }
        b.push('\n');
    }
    b.trim().to_string()
}
