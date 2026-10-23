//! Incremental ingestion must give exactly what a fresh full parse gives.

use super::*;
use std::io::Write;

struct TempDir(PathBuf);

impl TempDir {
    fn new(tag: &str) -> TempDir {
        static N: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = N.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let p = std::env::temp_dir().join(format!("ccsl-rs-transcript-{tag}-{}-{n}", std::process::id()));
        let _ = std::fs::remove_dir_all(&p);
        std::fs::create_dir_all(&p).unwrap();
        TempDir(p)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn ts(sec: u64) -> String {
    crate::timefmt::iso_string(1_792_580_000_000.0 + sec as f64 * 1000.0)
}

/// A transcript with every record kind the analyses look at.
fn lines() -> Vec<String> {
    let mut v = vec![
        format!(r#"{{"type":"user","timestamp":"{}","message":{{"role":"user","content":"hello"}}}}"#, ts(0)),
        format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{"usage":{{"input_tokens":10,"output_tokens":20,"cache_read_input_tokens":300,"cache_creation_input_tokens":40}}}}}}"#,
            ts(3)
        ),
        "not json at all".into(),
        format!(r#"{{"type":"custom-title","customTitle":"first name","timestamp":"{}"}}"#, ts(4)),
        format!(
            r#"{{"type":"user","timestamp":"{}","message":{{"content":"<local-command-stdout>Set effort level to high</local-command-stdout>"}}}}"#,
            ts(5)
        ),
        format!(
            r#"{{"type":"assistant","isSidechain":true,"agentId":"sub1","timestamp":"{}","message":{{"usage":{{"input_tokens":1000,"output_tokens":1000}}}}}}"#,
            ts(6)
        ),
        format!(
            r#"{{"type":"assistant","isApiErrorMessage":true,"timestamp":"{}","message":{{"usage":{{"input_tokens":7,"output_tokens":7}}}}}}"#,
            ts(7)
        ),
        format!(
            r#"{{"type":"user","timestamp":"{}","toolUseResult":{{"agentId":"sub1","status":"completed"}},"message":{{"content":[{{"type":"tool_result"}}]}}}}"#,
            ts(10)
        ),
    ];
    for i in 0..300u64 {
        let t = 20 + i * 7;
        v.push(format!(r#"{{"type":"user","timestamp":"{}","message":{{"content":"q{i}"}}}}"#, ts(t)));
        let stop = if i % 3 == 0 { r#""stop_reason":null,"# } else { r#""stop_reason":"end_turn","# };
        v.push(format!(
            r#"{{"type":"assistant","timestamp":"{}","message":{{{stop}"usage":{{"input_tokens":{},"output_tokens":{},"cache_read_input_tokens":{},"cache_creation_input_tokens":5}}}}}}"#,
            ts(t + 2 + i % 4),
            i + 1,
            2 * i + 3,
            1000 + i
        ));
        if i == 120 {
            v.push(format!(
                r#"{{"type":"system","subtype":"compact_boundary","timestamp":"{}","compactMetadata":{{"trigger":"auto","preTokens":150000,"postTokens":20000}}}}"#,
                ts(t + 6)
            ));
        }
        if i == 200 {
            v.push(format!(
                r#"{{"type":"system","subtype":"compact_boundary","timestamp":"{}","compactMetadata":{{"trigger":"manual","preTokens":90000}}}}"#,
                ts(t + 6)
            ));
            v.push(format!(r#"{{"type":"custom-title","customTitle":"renamed","timestamp":"{}"}}"#, ts(t + 6)));
        }
        if i == 250 {
            v.push(format!(
                r#"{{"type":"user","timestamp":"{}","message":{{"content":"<local-command-stdout>Set model to opus with max effort</local-command-stdout>"}}}}"#,
                ts(t + 6)
            ));
        }
    }
    v
}

fn subagent_lines() -> Vec<String> {
    (0..40u64)
        .flat_map(|i| {
            let t = 100 + i * 5;
            [
                format!(r#"{{"type":"user","isSidechain":true,"timestamp":"{}","message":{{"content":"s"}}}}"#, ts(t)),
                format!(
                    r#"{{"type":"assistant","isSidechain":true,"timestamp":"{}","message":{{"usage":{{"input_tokens":3,"output_tokens":9}}}}}}"#,
                    ts(t + 3)
                ),
            ]
        })
        .collect()
}

#[derive(Debug, PartialEq)]
struct Outputs {
    tokens: TokenMetrics,
    duration: Option<String>,
    compaction: Compaction,
    thinking: Option<Effort>,
    name: Option<String>,
    speed: SpeedMetrics,
    w30: SpeedMetrics,
    w120: SpeedMetrics,
}

fn outputs(a: &Analysis) -> Outputs {
    assert!(a.main_complete && a.speed_complete);
    Outputs {
        tokens: a.agg.tokens.finish(),
        duration: state::format_session_duration(a.agg.first_ts, a.agg.last_ts),
        compaction: a.agg.compaction.clone(),
        thinking: a.agg.thinking.clone(),
        name: a.agg.session_name.clone(),
        speed: a.agg.speed.session_average(),
        w30: a.agg.speed.windowed(30.0),
        w120: a.agg.speed.windowed(120.0),
    }
}

fn write(path: &Path, content: &str) {
    std::fs::write(path, content).unwrap();
}

fn append(path: &Path, content: &str) {
    let mut f = std::fs::OpenOptions::new().append(true).open(path).unwrap();
    f.write_all(content.as_bytes()).unwrap();
}

fn s(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

#[test]
fn incremental_matches_full_parse() {
    let dir = TempDir::new("inc");
    let t = dir.0.join("session.jsonl");
    let cache = dir.0.join("cache");
    std::fs::create_dir_all(dir.0.join("subagents")).unwrap();
    let sub = dir.0.join("subagents").join("agent-sub1.jsonl");
    let all = lines();
    let sub_all = subagent_lines();

    // grow both files in uneven steps, rendering in between; the last step
    // leaves an unterminated line
    let cuts = [0usize, 1, 2, 5, 9, 50, 51, 300, 420, all.len()];
    let mut sub_written = 0;
    write(&t, "");
    write(&sub, "");
    for w in cuts.windows(2) {
        let chunk: String = all[w[0]..w[1]].iter().map(|l| format!("{l}\n")).collect();
        append(&t, &chunk);
        let sub_to = (sub_written + 13).min(sub_all.len());
        let sub_chunk: String = sub_all[sub_written..sub_to].iter().map(|l| format!("{l}\n")).collect();
        append(&sub, &sub_chunk);
        sub_written = sub_to;
        let inc = analyze(&s(&t), true, Some(&cache), None);
        let full = analyze(&s(&t), true, None, None);
        assert_eq!(outputs(&inc), outputs(&full), "after {} lines", w[1]);
    }
    let rest: String = sub_all[sub_written..].iter().map(|l| format!("{l}\n")).collect();
    append(&sub, &rest);
    append(&t, r#"{"type":"custom-title","customTitle":"partial"}"#);
    let inc = analyze(&s(&t), true, Some(&cache), None);
    let full = analyze(&s(&t), true, None, None);
    assert_eq!(outputs(&inc), outputs(&full));
    assert_eq!(outputs(&inc).name.as_deref(), Some("partial"));
    // the unterminated line was not saved: completing it changes nothing
    append(&t, "\n");
    let inc = analyze(&s(&t), true, Some(&cache), None);
    assert_eq!(outputs(&inc), outputs(&full));

    let o = outputs(&full);
    assert_eq!(o.compaction.count, 2.0);
    assert_eq!(o.thinking, Some(Effort { value: "max".into(), known: true }));
    assert!(o.speed.total_duration_ms > 0.0 && o.w30.total_duration_ms > 0.0);
}

#[test]
fn time_budget_resumes() {
    let dir = TempDir::new("budget");
    let t = dir.0.join("s.jsonl");
    let cache = dir.0.join("cache");
    let body: String = lines().iter().map(|l| format!("{l}\n")).collect();
    write(&t, &body);
    let full = analyze(&s(&t), false, None, None);
    let mut renders = 0;
    let done = loop {
        renders += 1;
        let a = analyze(&s(&t), false, Some(&cache), Some(Duration::ZERO));
        if a.main_complete {
            break a;
        }
        assert!(renders < 100);
    };
    assert!(renders > 3, "zero budget should take several renders, took {renders}");
    assert_eq!(done.agg.tokens.finish(), full.agg.tokens.finish());
    assert_eq!(done.agg.compaction, full.agg.compaction);
    assert_eq!(done.agg.session_name, full.agg.session_name);
}

#[test]
fn rewritten_file_is_rebuilt() {
    let dir = TempDir::new("rewrite");
    let t = dir.0.join("s.jsonl");
    let cache = dir.0.join("cache");
    let body: String = lines().iter().map(|l| format!("{l}\n")).collect();
    write(&t, &body);
    let before = analyze(&s(&t), false, Some(&cache), None).agg.tokens.finish();
    // replaced in place with the same length (the last record differs), then shrunk
    let cut = body.rfind("\"cache_creation_input_tokens\":5").unwrap();
    let other = format!("{}\"cache_creation_input_tokens\":9{}", &body[..cut], &body[cut + 31..]);
    assert_eq!(other.len(), body.len());
    write(&t, &other);
    let a = analyze(&s(&t), false, Some(&cache), None);
    assert_eq!(a.agg.tokens.finish().cache_creation_tokens, before.cache_creation_tokens + 4.0);
    write(&t, &lines()[..3].iter().map(|l| format!("{l}\n")).collect::<String>());
    let a = analyze(&s(&t), false, Some(&cache), None);
    assert_eq!(a.agg.tokens.finish().output_tokens, 20.0);
}

#[test]
fn missing_and_directory_are_empty() {
    let dir = TempDir::new("missing");
    let a = analyze(&s(&dir.0.join("nope.jsonl")), true, None, None);
    assert!(a.empty);
    let a = analyze(&s(&dir.0), true, None, None);
    assert!(a.empty);
}

#[test]
fn cache_round_trips() {
    let dir = TempDir::new("codec");
    let t = dir.0.join("s.jsonl");
    let body: String = lines().iter().map(|l| format!("{l}\n")).collect();
    write(&t, &body);
    let cache_dir = dir.0.join("cache");
    analyze(&s(&t), false, Some(&cache_dir), None);
    let file = std::fs::read_dir(&cache_dir).unwrap().next().unwrap().unwrap().path();
    let bytes = std::fs::read(file).unwrap();
    let c = codec::decode(&bytes).unwrap();
    assert_eq!(codec::encode(&c), bytes);
    assert!(codec::decode(&bytes[..bytes.len() - 1]).is_none());
}

fn pair(i: u64, t: u64) -> String {
    let user = format!(r#"{{"type":"user","timestamp":"{}","message":{{"content":"q"}}}}"#, ts(t));
    let assistant = format!(
        r#"{{"type":"assistant","timestamp":"{}","message":{{"stop_reason":"end_turn","usage":{{"input_tokens":{},"output_tokens":{}}}}}}}"#,
        ts(t + 2),
        i % 7,
        i % 11 + 1
    );
    format!("{user}\n{assistant}\n")
}

/// Enough intervals to freeze, a subagent spanning the whole session, and
/// later an interval that reaches back into the frozen part.
#[test]
fn frozen_intervals_and_reach_back() {
    let dir = TempDir::new("freeze");
    let t = dir.0.join("s.jsonl");
    let cache = dir.0.join("cache");
    std::fs::create_dir_all(dir.0.join("s").join("subagents")).unwrap();
    let sub = dir.0.join("s").join("subagents").join("agent-x.jsonl");
    let mut main = String::from("{\"type\":\"user\",\"toolUseResult\":{\"agentId\":\"x\"}}\n");
    let mut subagent = String::new();
    for i in 0..3000u64 {
        main += &pair(i, i * 10);
        if i % 5 == 0 {
            subagent += &pair(i, i * 10 + 1);
        }
    }
    write(&t, &main);
    write(&sub, &subagent);
    let full = analyze(&s(&t), true, None, None);
    let inc = analyze(&s(&t), true, Some(&cache), None);
    assert_eq!(outputs(&inc), outputs(&full));
    // the saved state is frozen now; normal appends stay exact
    for i in 3000..3100u64 {
        append(&t, &pair(i, i * 10));
        append(&sub, &pair(i, i * 10 + 3));
    }
    let inc = analyze(&s(&t), true, Some(&cache), None);
    let full = analyze(&s(&t), true, None, None);
    assert_eq!(outputs(&inc), outputs(&full));
    // a user record from the distant past: the next render cannot be exact
    // from the frozen state, says so, and the one after rebuilds
    append(&t, &pair(0, 5));
    let inc = analyze(&s(&t), true, Some(&cache), None);
    assert!(!inc.speed_complete);
    let inc = analyze(&s(&t), true, Some(&cache), None);
    let full = analyze(&s(&t), true, None, None);
    assert_eq!(outputs(&inc), outputs(&full));
}

#[test]
fn lines_longer_than_a_read_chunk() {
    let dir = TempDir::new("long");
    let t = dir.0.join("s.jsonl");
    let big = "x".repeat(READ_CHUNK * 2 + 17);
    let body = format!(
        "{}\n{{\"type\":\"user\",\"message\":{{\"content\":\"{big}\"}}}}\n{}\n{{\"type\":\"custom-title\",\"customTitle\":\"{}\"}}",
        lines()[1],
        lines()[1],
        &big[..10]
    );
    write(&t, &body);
    let a = analyze(&s(&t), false, Some(&dir.0.join("cache")), None);
    assert_eq!(a.agg.tokens.finish().output_tokens, 40.0);
    assert_eq!(a.agg.session_name.as_deref(), Some("xxxxxxxxxx"));
}

#[test]
fn speed_toggled_on_later_rebuilds() {
    let dir = TempDir::new("toggle");
    let t = dir.0.join("s.jsonl");
    let cache = dir.0.join("cache");
    let body: String = lines().iter().map(|l| format!("{l}\n")).collect();
    write(&t, &body);
    analyze(&s(&t), false, Some(&cache), None);
    append(&t, &pair(1, 99_000));
    let inc = analyze(&s(&t), true, Some(&cache), None);
    let full = analyze(&s(&t), true, None, None);
    assert_eq!(outputs(&inc), outputs(&full));
}
