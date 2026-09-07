//! 교차검토 응답 스키마와 프롬프트. 세 참여자에게 동일하게 강제한다.
//!
//! Java 판에서 실측으로 확정한 사실을 그대로 가져온다.
//!   - 모든 object 에 `additionalProperties: false` 가 필요하다.
//!     codex 는 OpenAI strict 구조화 출력을 쓰므로 없으면 400 으로 거부한다.
//!   - 스키마는 파일 경로로 준다. 인라인 JSON 인자는 Windows 에서 깨지기 쉽다.
//!   - CLI 레벨 강제가 항상 되는 것은 아니므로 프롬프트에도 스키마를 싣는다.

use anyhow::Result;
use std::path::{Path, PathBuf};

pub const SCHEMA: &str = r#"{
  "type": "object",
  "additionalProperties": false,
  "required": ["verdict", "summary", "issues", "open_questions"],
  "properties": {
    "verdict": { "type": "string", "enum": ["AGREE", "CONCERNS", "BLOCK"] },
    "summary": { "type": "string" },
    "issues": {
      "type": "array",
      "items": {
        "type": "object",
        "additionalProperties": false,
        "required": ["id", "severity", "claim", "rationale", "suggestion"],
        "properties": {
          "id":         { "type": "string" },
          "severity":   { "type": "string", "enum": ["critical", "major", "minor"] },
          "claim":      { "type": "string" },
          "rationale":  { "type": "string" },
          "suggestion": { "type": "string" }
        }
      }
    },
    "open_questions": { "type": "array", "items": { "type": "string" } }
  }
}"#;

/// 스키마를 파일로 떨어뜨린다. 워크스페이스가 아니라 앱 temp 에 만든다.
pub fn write_to(temp_dir: &Path) -> Result<PathBuf> {
    std::fs::create_dir_all(temp_dir)?;
    let f = temp_dir.join("review-schema.json");
    std::fs::write(&f, SCHEMA)?;
    Ok(f)
}

/// 1라운드 검토 요청.
pub fn round1(subject: &str) -> String {
    format!(
        "너는 독립 검토자다. 아래 안건을 검토하고 **지정된 JSON 스키마에 맞춰서만** 답하라.\n\
         \n\
         - 동의를 위한 동의를 하지 마라. 근거 없는 지적도 하지 마라.\n\
         - verdict 는 AGREE(진행 가능) / CONCERNS(보완 필요) / BLOCK(이대로 불가) 중 하나다.\n\
         - 각 issue 의 id 는 응답 안에서 고유해야 한다.\n\
         - 판단에 필요한 정보가 없으면 지어내지 말고 open_questions 에 적어라.\n\
         - 파일을 수정하지 마라. 검토만 한다.\n\
         \n\
         ## 안건\n\
         \n\
         {subject}\n\
         {}",
        schema_block()
    )
}

/// 2라운드 반론. 상대 의견을 붙이되 출처는 밝히지 않는다.
pub fn round2(subject: &str, opposing: &str) -> String {
    format!(
        "너는 독립 검토자다. **2라운드**다. 아래는 같은 안건을 검토한 다른 AI 의\n\
         의견이다. 누가 말했는지는 알려주지 않는다. 출처가 아니라 논지로 판단하라.\n\
         \n\
         읽고 나서 **네 입장을 유지할지 철회할지** 밝히고, 같은 스키마로 답하라.\n\
         \n\
         - 유지하려면 상대 논지의 어디가 왜 틀렸는지 짚어라. 못 짚으면 유지가 아니다.\n\
         - 틀렸으면 철회하라. 여기서 철회하는 것에는 아무 불이익이 없다.\n\
         - 양보를 위한 양보도, 버티기 위한 버티기도 하지 마라.\n\
         - 새 쟁점을 꺼내지 마라.\n\
         \n\
         ## 안건\n\
         \n\
         {subject}\n\
         \n\
         ## 다른 검토자의 의견\n\
         \n\
         {opposing}\n\
         {}",
        schema_block()
    )
}

/// 프롬프트에 스키마를 직접 싣는다.
///
/// CLI 레벨 강제가 항상 되는 것은 아니다 — 계정 권한이나 인자 전달 문제로
/// 거부될 수 있다. 프롬프트에 넣어두면 그래도 형식이 맞는다.
fn schema_block() -> String {
    format!(
        "\n## 응답 형식 — 이 JSON 스키마를 정확히 따르라\n\
         \n\
         ```json\n{SCHEMA}\n```\n\
         \n\
         **JSON 객체 하나만 출력하라.** 설명, 인사말, 코드펜스 밖의 문장을 쓰지 마라.\n"
    )
}
