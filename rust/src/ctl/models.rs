//! 모델 별칭 → 실제 모델 이름.
//!
//! `-m sol` 처럼 계열 이름만 주면 그 계열의 **가장 최신 버전**을 고른다.
//! codex 는 `~/.codex/models_cache.json` 에 쓸 수 있는 모델 목록을 두므로
//! 네트워크 없이 푼다. agy 는 `agy models` 가 네트워크로 목록을 가져오므로
//! 그 출력을 `~/.multi-ai-cli/agy_models.txt` 에 하루 동안 캐시해 쓴다 —
//! TUI 는 캐시만 보고(틱 안에서 막히면 안 된다) 기동 때 뒤에서 채운다.
//! claude 는 CLI 자체가 별칭(`opus` 등)을 받으니 그대로 넘긴다.
//! 못 풀면 준 이름을 그대로 넘긴다 — 틀렸다면 그 CLI 가 자기 말로 알려 준다.
use std::path::{Path, PathBuf};
use std::time::Duration;

/// TUI 용. 캐시만 본다.
pub fn resolve(agent: &str, alias: &str) -> String {
    match agent {
        "codex" => codex_cache_path().and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|json| resolve_codex(&json, alias)).unwrap_or_else(|| alias.to_string()),
        "agy" => std::fs::read_to_string(agy_cache_path()).ok()
            .and_then(|list| resolve_agy(&list, alias)).unwrap_or_else(|| alias.to_string()),
        _ => alias.to_string(),
    }
}

/// CLI(`mai ctl model`) 용. agy 캐시가 없거나 오래됐으면 지금 가져온다.
pub fn resolve_fetching(agent: &str, alias: &str) -> String {
    if agent == "agy" && agy_cache_stale() { let _ = refresh_agy_cache(); }
    resolve(agent, alias)
}

/// 기동 때 뒤에서 agy 캐시를 채운다. 실패해도 조용히 넘어간다.
pub fn warm_agy_cache() {
    if agy_cache_stale() { std::thread::spawn(|| { let _ = refresh_agy_cache(); }); }
}

const AGY_CACHE_TTL: Duration = Duration::from_secs(24 * 3600);
const AGY_FETCH_TIMEOUT: Duration = Duration::from_secs(30);

fn codex_cache_path() -> Option<PathBuf> {
    Some(Path::new(&crate::app::home_var()?).join(".codex").join("models_cache.json"))
}
fn agy_cache_path() -> PathBuf { crate::room::home().join("agy_models.txt") }

fn agy_cache_stale() -> bool {
    std::fs::metadata(agy_cache_path()).and_then(|m| m.modified()).ok()
        .and_then(|t| t.elapsed().ok()).map_or(true, |age| age > AGY_CACHE_TTL)
}

/// `agy models` 를 돌려 캐시에 쓴다. 30초 안에 안 끝나면 죽인다.
fn refresh_agy_cache() -> std::io::Result<()> {
    let (exe, prefix) = crate::pty::resolve_agent("agy").ok_or_else(|| std::io::Error::other("agy 를 찾지 못했다"))?;
    let mut child = std::process::Command::new(exe).args(prefix).arg("models")
        .stdin(std::process::Stdio::null()).stdout(std::process::Stdio::piped()).stderr(std::process::Stdio::null()).spawn()?;
    let mut stdout = child.stdout.take().ok_or_else(|| std::io::Error::other("stdout 없음"))?;
    let reader = std::thread::spawn(move || { let mut s = String::new(); use std::io::Read; let _ = stdout.read_to_string(&mut s); s });
    let started = std::time::Instant::now();
    loop {
        if child.try_wait()?.is_some() { break; }
        if started.elapsed() > AGY_FETCH_TIMEOUT { let _ = child.kill(); let _ = child.wait(); return Err(std::io::Error::other("agy models 시간 초과")); }
        std::thread::sleep(Duration::from_millis(100));
    }
    let text = reader.join().unwrap_or_default();
    if !text.lines().any(|l| l.contains('\t')) { return Err(std::io::Error::other("agy models 출력이 비었다")); }
    std::fs::create_dir_all(crate::room::home())?;
    std::fs::write(agy_cache_path(), text)
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
    let mut best: Option<(Vec<u64>, i64, String)> = None;
    for m in models {
        let Some(slug) = m.get("slug").and_then(|s| s.as_str()) else { continue };
        if slug.eq_ignore_ascii_case(alias) { return Some(slug.to_string()); }
        if m.get("visibility").and_then(|s| s.as_str()) == Some("hide") { continue; }
        if !tokens_match(slug, alias) { continue; }
        let version = version_of(slug);
        let priority = m.get("priority").and_then(|p| p.as_i64()).unwrap_or(i64::MAX);
        let better = match &best { None => true, Some((bv, bp, _)) => version > *bv || (version == *bv && priority < *bp) };
        if better { best = Some((version, priority, slug.to_string())); }
    }
    best.map(|(_, _, slug)| slug)
}

/// `agy models` 출력(`id<TAB>표시 이름` 줄들)에서 별칭에 맞는 가장 최신 id.
///
/// 규칙은 codex 와 같다: 토큰 통째 비교, 정확히 같으면 그것, 버전이 높은 쪽,
/// 같으면 목록에서 먼저 나온 쪽(agy 는 high → medium → low 순으로 낸다).
pub fn resolve_agy(list: &str, alias: &str) -> Option<String> {
    let mut best: Option<(Vec<u64>, String)> = None;
    for line in list.lines() {
        let id = line.split('\t').next().unwrap_or("").trim();
        if id.is_empty() { continue; }
        if id.eq_ignore_ascii_case(alias) { return Some(id.to_string()); }
        if !tokens_match(id, alias) { continue; }
        let version = version_of(id);
        if best.as_ref().map_or(true, |(bv, _)| version > *bv) { best = Some((version, id.to_string())); }
    }
    best.map(|(_, id)| id)
}

/// 별칭의 토큰(`-` 구분)이 전부 이름의 토큰에 있는가. `flash-low` 는 `gemini-3.8-flash-low` 에 맞는다.
fn tokens_match(name: &str, alias: &str) -> bool {
    let have: Vec<String> = name.split('-').map(|t| t.to_ascii_lowercase()).collect();
    alias.split('-').filter(|t| !t.is_empty()).all(|t| have.iter().any(|h| h == &t.to_ascii_lowercase()))
}

/// 이름에서 첫 버전 숫자를 뽑는다: `gpt-6.1-sol` → [6, 1], `gpt-6-sol` → [6].
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
    const AGY: &str = "gemini-3.8-flash-high\tGemini 3.8 Flash (High)\ngemini-3.8-flash-medium\tGemini 3.8 Flash (Medium)\ngemini-3.8-flash-low\tGemini 3.8 Flash (Low)\ngemini-3.1-pro-high\tGemini 3.1 Pro\nclaude-opus-4-6-thinking\tClaude Opus 4.6\ngpt-oss-120b-medium\tGPT-OSS\n";

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
    fn agy_목록에서_푼다() {
        assert_eq!(resolve_agy(AGY, "flash").as_deref(), Some("gemini-3.8-flash-high"));
        assert_eq!(resolve_agy(AGY, "flash-low").as_deref(), Some("gemini-3.8-flash-low"));
        assert_eq!(resolve_agy(AGY, "pro").as_deref(), Some("gemini-3.1-pro-high"));
        assert_eq!(resolve_agy(AGY, "opus").as_deref(), Some("claude-opus-4-6-thinking"));
        assert_eq!(resolve_agy(AGY, "gemini-3.8-flash-medium").as_deref(), Some("gemini-3.8-flash-medium"));
        assert_eq!(resolve_agy(AGY, "gem"), None);
        assert_eq!(resolve_agy("", "flash"), None);
    }
    #[test]
    fn 못_풀면_그대로_넘긴다() {
        assert_eq!(resolve("claude", "opus"), "opus");
        assert_eq!(resolve_codex("broken", "sol"), None);
    }
}
