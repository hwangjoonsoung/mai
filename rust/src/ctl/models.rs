//! 모델 별칭 → 실제 모델 이름.
//!
//! `-m sol` 처럼 계열 이름만 주면 그 계열의 **가장 최신 버전**을 고른다.
//! codex 는 `~/.codex/models_cache.json` 에 쓸 수 있는 모델 목록을 두므로
//! 네트워크 없이 푼다. claude 는 CLI 자체가 별칭(`opus` 등)을 받으니 그대로
//! 넘기고, agy 는 목록을 네트워크로 가져와야 해서(`agy models`) 그대로 넘긴다.
//! 못 풀면 준 이름을 그대로 넘긴다 — 틀렸다면 그 CLI 가 자기 말로 알려 준다.
use std::path::Path;

pub fn resolve(agent: &str, alias: &str) -> String {
    match agent {
        "codex" => codex_cache_path().and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|json| resolve_codex(&json, alias)).unwrap_or_else(|| alias.to_string()),
        _ => alias.to_string(),
    }
}

fn codex_cache_path() -> Option<std::path::PathBuf> {
    Some(Path::new(&crate::app::home_var()?).join(".codex").join("models_cache.json"))
}

/// `models_cache.json` 의 `models[]` 에서 별칭에 맞는 가장 최신 slug.
///
/// 별칭은 slug 를 `-` 로 나눈 토큰과 **통째로** 비교한다(`sol` 은 `gpt-6.1-sol` 에
/// 맞고 `solar` 에는 맞지 않는다). slug 와 똑같으면 그게 답이다. 숨김(`hide`)
/// 모델은 뺀다. 최신은 slug 안의 버전 숫자(`6.1` > `6` > `5.6`)로 고르고,
/// 같으면 목록의 priority 가 작은 쪽이다.
pub fn resolve_codex(json: &str, alias: &str) -> Option<String> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let models = v.get("models")?.as_array()?;
    let want = alias.to_ascii_lowercase();
    let mut best: Option<(Vec<u64>, i64, String)> = None;
    for m in models {
        let Some(slug) = m.get("slug").and_then(|s| s.as_str()) else { continue };
        if slug.eq_ignore_ascii_case(alias) { return Some(slug.to_string()); }
        if m.get("visibility").and_then(|s| s.as_str()) == Some("hide") { continue; }
        if !slug.split('-').any(|t| t.eq_ignore_ascii_case(&want)) { continue; }
        let version = version_of(slug);
        let priority = m.get("priority").and_then(|p| p.as_i64()).unwrap_or(i64::MAX);
        let better = match &best { None => true, Some((bv, bp, _)) => version > *bv || (version == *bv && priority < *bp) };
        if better { best = Some((version, priority, slug.to_string())); }
    }
    best.map(|(_, _, slug)| slug)
}

/// slug 에서 첫 버전 숫자를 뽑는다: `gpt-6.1-sol` → [6, 1], `gpt-6-sol` → [6].
fn version_of(slug: &str) -> Vec<u64> {
    slug.split('-')
        .find(|t| t.chars().next().is_some_and(|c| c.is_ascii_digit()))
        .map(|t| t.split('.').filter_map(|n| n.parse().ok()).collect())
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    const CACHE: &str = r#"{"models":[
        {"slug":"gpt-6.1-sol","visibility":"list","priority":1},
        {"slug":"gpt-6-astra","visibility":"list","priority":3},
        {"slug":"gpt-6.1-terra","visibility":"list","priority":2},
        {"slug":"gpt-6-sol","visibility":"list","priority":4},
        {"slug":"gpt-reserve","visibility":"hide","priority":5},
        {"slug":"gpt-5.6-sol","visibility":"list","priority":7},
        {"slug":"gpt-7-solar","visibility":"list","priority":0},
        {"slug":"gpt-9-sol","visibility":"hide","priority":0}
    ]}"#;

    #[test]
    fn 계열_이름은_최신_버전으로_푼다() {
        assert_eq!(resolve_codex(CACHE, "sol").as_deref(), Some("gpt-6.1-sol"));
        assert_eq!(resolve_codex(CACHE, "Terra").as_deref(), Some("gpt-6.1-terra"));
        assert_eq!(resolve_codex(CACHE, "astra").as_deref(), Some("gpt-6-astra"));
    }
    #[test]
    fn 정확한_slug_는_그대로이고_숨김과_부분일치는_뺀다() {
        assert_eq!(resolve_codex(CACHE, "gpt-6-sol").as_deref(), Some("gpt-6-sol"));
        assert_eq!(resolve_codex(CACHE, "gpt-reserve").as_deref(), Some("gpt-reserve"));
        assert_eq!(resolve_codex(CACHE, "reserve"), None);
        assert_eq!(resolve_codex(CACHE, "nope"), None);
        assert_eq!(version_of("gpt-6.1-sol"), vec![6, 1]);
        assert_eq!(version_of("codex-auto-review"), Vec::<u64>::new());
    }
    #[test]
    fn 못_풀면_그대로_넘긴다() {
        assert_eq!(resolve("claude", "opus"), "opus");
        assert_eq!(resolve("agy", "flash"), "flash");
        assert_eq!(resolve_codex("broken", "sol"), None);
    }
}
