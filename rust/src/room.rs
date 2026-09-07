//! 방 저장·재개와 설정. R5.
//!
//! 저장 구조는 Java 판과 같다. 대화 기록은 워크스페이스가 아니라 홈에 쌓는다.
//!
//!   ~/.multi-ai-cli/
//!     config.toml
//!     temp/                     공급자 임시 출력
//!     rooms/<room-id>/
//!       room.toml               방 메타데이터
//!       transcript.md           대화 기록 (SSOT)
//!       runs/<round>/converge/REPORT.md
//!
//! transcript 는 **길이 기반 프레이밍**을 쓴다. 여는 마커에 본문의 UTF-8 바이트
//! 수를 적고 정확히 그만큼 읽는다. 본문에 마커처럼 생긴 문자열이 들어와도
//! 안전하다 — 이스케이프를 아예 하지 않기 때문이다 (SPEC §7.6, 5라운드 검토 결과).

use anyhow::{Context, Result};
use std::{
    fs,
    path::{Path, PathBuf},
};

pub fn home() -> PathBuf {
    let base = crate::app::home_var()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
    base.join(".multi-ai-cli")
}

pub fn rooms_dir() -> PathBuf {
    home().join("rooms")
}

// ---------- 설정 ----------

pub struct Config {
    pub agy_model: String,
    pub ui_mode: String,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            agy_model: "gemini-3.1-pro-high".into(),
            ui_mode: "tui".into(),
        }
    }
}

impl Config {
    /// config.toml 을 읽는다. 없으면 기본값을 쓰고 파일을 만들어 둔다.
    ///
    /// 의존성을 늘리지 않으려고 toml 크레이트를 쓰지 않는다. 우리가 읽는 키가
    /// 몇 개뿐이라 `키 = "값"` 만 훑으면 충분하다.
    pub fn load() -> Self {
        let path = home().join("config.toml");
        let mut c = Config::default();
        let Ok(text) = fs::read_to_string(&path) else {
            let _ = fs::create_dir_all(home());
            let _ = fs::write(&path, DEFAULT_CONFIG);
            return c;
        };
        for line in text.lines() {
            let line = line.trim();
            if line.starts_with('#') {
                continue;
            }
            let Some((k, v)) = line.split_once('=') else { continue };
            let v = v.trim().trim_matches('"').trim();
            match k.trim() {
                "agy_model" if !v.is_empty() => c.agy_model = v.into(),
                "ui_mode" if !v.is_empty() => c.ui_mode = v.into(),
                _ => {}
            }
        }
        c
    }
}

const DEFAULT_CONFIG: &str = r#"# multi_ai_cli 설정

# agy 기본 모델. 검토 용도라 추론 강한 pro 를 기본값으로 쓴다.
# 쓸 수 있는 목록은 `agy models` 로 확인한다.
agy_model = "gemini-3.1-pro-high"

# 화면 모드. 지금은 tui 만 있다.
ui_mode = "tui"
"#;

// ---------- 방 ----------

pub struct Room {
    pub id: String,
    pub dir: PathBuf,
    pub workspace: PathBuf,
    pub round: u32,
    next_id: u32,
}

impl Room {
    /// 새 방을 만든다. id 는 생성 시각이다.
    pub fn create(workspace: &Path) -> Result<Self> {
        let id = timestamp();
        let dir = rooms_dir().join(&id);
        fs::create_dir_all(dir.join("runs")).context("방 디렉터리를 만들지 못했다")?;
        let r = Room {
            id,
            dir,
            workspace: workspace.to_path_buf(),
            round: 0,
            next_id: 1,
        };
        r.save_meta()?;
        Ok(r)
    }

    pub fn transcript(&self) -> PathBuf {
        self.dir.join("transcript.md")
    }

    pub fn run_dir(&self, round: u32) -> Result<PathBuf> {
        let d = self.dir.join("runs").join(format!("r{round:04}"));
        fs::create_dir_all(&d)?;
        Ok(d)
    }

    pub fn start_round(&mut self) -> u32 {
        self.round += 1;
        self.round
    }

    pub fn save_meta(&self) -> Result<()> {
        let text = format!(
            "id = \"{}\"\nworkspace = \"{}\"\nround = {}\nnext_id = {}\n",
            self.id,
            self.workspace.to_string_lossy().replace('\\', "\\\\"),
            self.round,
            self.next_id
        );
        fs::write(self.dir.join("room.toml"), text)?;
        Ok(())
    }

    /// 메시지를 덧붙인다.
    pub fn append(&mut self, sender: &str, status: &str, ms: u64, body: &str) -> Result<()> {
        let frame = frame(self.next_id, self.round, sender, status, ms, body);
        let mut f = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(self.transcript())?;
        use std::io::Write;
        f.write_all(&frame)?;
        self.next_id += 1;
        self.save_meta()
    }
}

fn timestamp() -> String {
    // 외부 시간 크레이트를 쓰지 않는다. 정렬만 되면 되므로 epoch 초로 충분하다.
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("r{secs}")
}

/// 저장된 방 목록. 최신순.
pub fn list() -> Vec<(String, PathBuf)> {
    let mut out = Vec::new();
    let Ok(rd) = fs::read_dir(rooms_dir()) else { return out };
    for e in rd.flatten() {
        if e.path().is_dir() {
            out.push((e.file_name().to_string_lossy().into_owned(), e.path()));
        }
    }
    out.sort_by(|a, b| b.0.cmp(&a.0));
    out
}

/// 방 메타에서 next_id 를 읽는다. 재동기화 상한으로 쓴다.
pub fn next_id_of(dir: &Path) -> u32 {
    let Ok(t) = fs::read_to_string(dir.join("room.toml")) else { return u32::MAX };
    for line in t.lines() {
        if let Some((k, v)) = line.split_once('=') {
            if k.trim() == "next_id" {
                return v.trim().parse().unwrap_or(u32::MAX);
            }
        }
    }
    u32::MAX
}

// ---------- transcript 프레이밍 ----------

/// 메시지 한 건을 프레임으로 만든다.
///
/// 디스크 개행은 LF 하나로 고정한다. CRLF 가 섞이면 bytes 계산과 닫는 마커
/// 매칭이 어긋난다 (교차검토 C1).
fn frame(id: u32, round: u32, sender: &str, status: &str, ms: u64, body: &str) -> Vec<u8> {
    let b = body.as_bytes();
    let header = format!(
        "<!-- msg id={id:04} round={round} sender={sender} status={status} ms={ms} bytes={} -->",
        b.len()
    );
    let mut out = Vec::with_capacity(header.len() + b.len() + 40);
    out.extend_from_slice(header.as_bytes());
    out.push(b'\n');
    out.extend_from_slice(b);
    out.push(b'\n');
    out.extend_from_slice(format!("<!-- /msg id={id:04} -->").as_bytes());
    out.push(b'\n');
    out
}

#[derive(Debug)]
pub struct Message {
    pub id: u32,
    pub sender: String,
    pub body: String,
    pub suspect: bool,
}

/// transcript 를 읽는다.
///
/// 손상되지 않은 파일은 무손실 왕복을 보장한다. 손상 이후 재동기화는
/// **best-effort** 이며 복구분을 SUSPECT 로 표시한다 — 어느 조건을 써도 완전하지
/// 않다는 것이 5라운드 교차검토의 결론이다.
pub fn read_transcript(path: &Path, next_id: u32) -> Vec<Message> {
    let Ok(all) = fs::read(path) else { return vec![] };
    let mut out = Vec::new();
    let (mut p, mut last_id) = (0usize, 0u32);
    let mut resyncing = false;

    while p < all.len() {
        let Some(nl) = find(&all, b"\n", p) else { break };
        let header = String::from_utf8_lossy(&all[p..nl]).to_string();
        let parsed = parse_header(&header);

        let mut ok = parsed.is_some();
        let (mut id, mut bytes, mut sender) = (0u32, 0usize, String::new());
        if let Some((i, b, s)) = parsed {
            id = i;
            bytes = b;
            sender = s;
        }
        // 재동기화 조건 — 단조 증가만 요구한다. `id == last+1` 로 강화하면
        // 손상으로 id 가 건너뛴 뒤 모든 후속 메시지를 잃는다 (실측, 교차검토 E2).
        if ok && resyncing {
            ok = id > last_id && id < next_id;
        }
        let body_start = nl + 1;
        let close = format!("\n<!-- /msg id={id:04} -->\n");
        if ok && (body_start + bytes > all.len() || !region_eq(&all, body_start + bytes, close.as_bytes())) {
            ok = false;
        }
        if !ok {
            resyncing = true;
            p = nl + 1;
            continue;
        }

        match std::str::from_utf8(&all[body_start..body_start + bytes]) {
            Ok(body) => {
                out.push(Message {
                    id,
                    sender,
                    body: body.to_string(),
                    suspect: resyncing,
                });
                last_id = id;
                resyncing = false;
            }
            Err(_) => {
                // strict 디코딩 실패. 원시 바이트는 runs/ 에 남아 있다.
                resyncing = true;
                last_id = id;
            }
        }
        p = body_start + bytes + close.len();
    }
    out
}

fn parse_header(h: &str) -> Option<(u32, usize, String)> {
    let inner = h.strip_prefix("<!-- msg ")?.strip_suffix("-->")?;
    let (mut id, mut bytes, mut sender) = (None, None, String::from("unknown"));
    for tok in inner.split_whitespace() {
        let Some((k, v)) = tok.split_once('=') else { continue };
        match k {
            "id" => id = v.parse::<u32>().ok(),
            "bytes" => bytes = v.parse::<usize>().ok(),
            "sender" => sender = v.to_string(),
            _ => {}
        }
    }
    Some((id?, bytes?, sender))
}

fn region_eq(a: &[u8], off: usize, n: &[u8]) -> bool {
    off + n.len() <= a.len() && &a[off..off + n.len()] == n
}

fn find(a: &[u8], n: &[u8], from: usize) -> Option<usize> {
    if from >= a.len() {
        return None;
    }
    a[from..]
        .windows(n.len())
        .position(|w| w == n)
        .map(|i| i + from)
}
