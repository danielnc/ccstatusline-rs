//! herdr (terminal workspace manager) labels for the pane/tab/workspace the
//! status line runs in.
//!
//! herdr exports `HERDR_ENV=1`, `HERDR_SOCKET_PATH`, `HERDR_WORKSPACE_ID`,
//! `HERDR_TAB_ID` and `HERDR_PANE_ID` into every pane. Its socket API speaks
//! newline-delimited JSON: one request line
//! `{"id":..,"method":"tab.get","params":{"tab_id":..}}` gets one response
//! line `{"id":..,"result":{"type":"tab_info","tab":{..,"label":..}}}`. A
//! round trip costs well under a millisecond, so no process is spawned and no
//! cache is kept; each render asks once per kind, with a short timeout.

use serde_json::Value;
use std::io::{BufRead, BufReader, Write};
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_millis(150);
const MAX_RESPONSE_BYTES: u64 = 1 << 20;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Workspace,
    Tab,
    Pane,
}

impl Kind {
    fn id_var(self) -> &'static str {
        match self {
            Kind::Workspace => "HERDR_WORKSPACE_ID",
            Kind::Tab => "HERDR_TAB_ID",
            Kind::Pane => "HERDR_PANE_ID",
        }
    }
    fn method(self) -> &'static str {
        match self {
            Kind::Workspace => "workspace.get",
            Kind::Tab => "tab.get",
            Kind::Pane => "pane.get",
        }
    }
    fn key(self) -> &'static str {
        match self {
            Kind::Workspace => "workspace",
            Kind::Tab => "tab",
            Kind::Pane => "pane",
        }
    }
}

/// What the widget shows: the live label when herdr answers, else the id.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Target {
    pub id: String,
    pub label: Option<String>,
}

/// None outside herdr (HERDR_ENV != 1 or no id for this kind).
pub fn lookup(kind: Kind, env: &dyn Fn(&str) -> Option<String>) -> Option<Target> {
    if env("HERDR_ENV").as_deref() != Some("1") {
        return None;
    }
    let id = env(kind.id_var()).filter(|v| !v.is_empty())?;
    let label = env("HERDR_SOCKET_PATH").filter(|v| !v.is_empty()).and_then(|sock| fetch_label(&sock, kind, &id));
    Some(Target { id, label })
}

pub fn lookup_process(kind: Kind) -> Option<Target> {
    use std::sync::OnceLock;
    static CACHE: [OnceLock<Option<Target>>; 3] = [OnceLock::new(), OnceLock::new(), OnceLock::new()];
    CACHE[kind as usize].get_or_init(|| lookup(kind, &|k| std::env::var(k).ok())).clone()
}

#[cfg(unix)]
fn fetch_label(socket: &str, kind: Kind, id: &str) -> Option<String> {
    let mut stream = std::os::unix::net::UnixStream::connect(socket).ok()?;
    stream.set_read_timeout(Some(TIMEOUT)).ok()?;
    stream.set_write_timeout(Some(TIMEOUT)).ok()?;
    let mut params = serde_json::Map::new();
    params.insert(format!("{}_id", kind.key()), Value::String(id.to_string()));
    let mut req = serde_json::Map::new();
    req.insert("id".into(), Value::String("ccstatusline".into()));
    req.insert("method".into(), Value::String(kind.method().into()));
    req.insert("params".into(), Value::Object(params));
    let mut line = serde_json::to_string(&Value::Object(req)).ok()?;
    line.push('\n');
    stream.write_all(line.as_bytes()).ok()?;
    let mut reader = BufReader::new(std::io::Read::take(stream, MAX_RESPONSE_BYTES));
    let mut resp = String::new();
    reader.read_line(&mut resp).ok()?;
    parse_label(&resp, kind)
}

#[cfg(not(unix))]
fn fetch_label(_socket: &str, _kind: Kind, _id: &str) -> Option<String> {
    None
}

fn parse_label(resp: &str, kind: Kind) -> Option<String> {
    let v: Value = serde_json::from_str(resp.trim()).ok()?;
    let label = v.get("result")?.get(kind.key())?.get("label")?.as_str()?;
    let label = crate::jsnum::js_trim(label);
    (!label.is_empty()).then(|| label.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::io::Read;

    fn env_of(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let m: HashMap<String, String> = pairs.iter().map(|(k, v)| (k.to_string(), v.to_string())).collect();
        move |k| m.get(k).cloned()
    }

    /// A one-shot fake herdr server; returns the socket path and the request it saw.
    fn fake_server(response: &'static str) -> (String, std::thread::JoinHandle<String>) {
        let dir = std::env::temp_dir().join(format!("ccsl-herdr-{}-{}", std::process::id(), rand_suffix()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("h.sock");
        let listener = std::os::unix::net::UnixListener::bind(&path).unwrap();
        let handle = std::thread::spawn(move || {
            let (mut s, _) = listener.accept().unwrap();
            let mut buf = Vec::new();
            let mut byte = [0u8; 1];
            while s.read(&mut byte).unwrap() == 1 && byte[0] != b'\n' {
                buf.push(byte[0]);
            }
            s.write_all(response.as_bytes()).unwrap();
            let _ = std::fs::remove_dir_all(dir);
            String::from_utf8(buf).unwrap()
        });
        (path.to_string_lossy().into_owned(), handle)
    }

    fn rand_suffix() -> u128 {
        std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos()
    }

    #[test]
    fn outside_herdr_is_none() {
        assert_eq!(lookup(Kind::Tab, &env_of(&[])), None);
        assert_eq!(lookup(Kind::Tab, &env_of(&[("HERDR_ENV", "0"), ("HERDR_TAB_ID", "w1:t1")])), None);
        assert_eq!(lookup(Kind::Pane, &env_of(&[("HERDR_ENV", "1"), ("HERDR_TAB_ID", "w1:t1")])), None);
    }

    #[test]
    fn falls_back_to_id_without_socket() {
        let t = lookup(Kind::Pane, &env_of(&[("HERDR_ENV", "1"), ("HERDR_PANE_ID", "w1:p1")])).unwrap();
        assert_eq!(t, Target { id: "w1:p1".into(), label: None });
        let t = lookup(
            Kind::Pane,
            &env_of(&[("HERDR_ENV", "1"), ("HERDR_PANE_ID", "w1:p1"), ("HERDR_SOCKET_PATH", "/nonexistent/h.sock")]),
        )
        .unwrap();
        assert_eq!(t.label, None);
    }

    #[test]
    fn reads_tab_label_from_socket() {
        let (sock, h) = fake_server(
            "{\"id\":\"ccstatusline\",\"result\":{\"type\":\"tab_info\",\"tab\":{\"tab_id\":\"w1:t1\",\"label\":\"ccstatusline-rs\"}}}\n",
        );
        let env = env_of(&[("HERDR_ENV", "1"), ("HERDR_TAB_ID", "w1:t1"), ("HERDR_SOCKET_PATH", &sock)]);
        let t = lookup(Kind::Tab, &env).unwrap();
        assert_eq!(t.label.as_deref(), Some("ccstatusline-rs"));
        let req: Value = serde_json::from_str(&h.join().unwrap()).unwrap();
        assert_eq!(req["method"], "tab.get");
        assert_eq!(req["params"]["tab_id"], "w1:t1");
    }

    #[test]
    fn null_pane_label_falls_back() {
        let (sock, h) = fake_server("{\"id\":\"ccstatusline\",\"result\":{\"type\":\"pane_info\",\"pane\":{\"pane_id\":\"w1:p1\",\"label\":null}}}\n");
        let env = env_of(&[("HERDR_ENV", "1"), ("HERDR_PANE_ID", "w1:p1"), ("HERDR_SOCKET_PATH", &sock)]);
        assert_eq!(lookup(Kind::Pane, &env).unwrap(), Target { id: "w1:p1".into(), label: None });
        let req: Value = serde_json::from_str(&h.join().unwrap()).unwrap();
        assert_eq!(req["method"], "pane.get");
        assert_eq!(req["params"]["pane_id"], "w1:p1");
    }

    #[test]
    fn error_response_falls_back() {
        let (sock, h) = fake_server("{\"id\":\"ccstatusline\",\"error\":{\"code\":\"not_found\",\"message\":\"x\"}}\n");
        let env = env_of(&[("HERDR_ENV", "1"), ("HERDR_WORKSPACE_ID", "w9"), ("HERDR_SOCKET_PATH", &sock)]);
        assert_eq!(lookup(Kind::Workspace, &env).unwrap().label, None);
        h.join().unwrap();
    }
}
