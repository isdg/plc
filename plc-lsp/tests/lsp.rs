//! End to end: the built `plc-lsp` binary over stdio against a temp vault.

use std::fs;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::PathBuf;
use std::process::{Child, ChildStdin, ChildStdout, Command, Stdio};

use serde_json::{json, Value};
use tower_lsp::lsp_types::Url;

const A: &str = "# A
see [[Дар]] and [[b#Second|bb]] and [[b#^blk]]
ledger [[revolut]] and [[nope]] and [[Йога]]
[[#Local]]
## Local
";
const B: &str = "# B\n```sh\n# Second\n```\n## Second\nline ^blk\n[[Дар]]\n";
// Й written decomposed (И + combining breve), as some tools store it.
const YOGA_NFD: &str = "\u{0438}\u{0306}ога.md";

struct Vault(PathBuf);

impl Vault {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir()
            .join(format!("plc-lsp-{tag}-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let root = fs::canonicalize(&dir).unwrap();
        let files: [(&str, &str); 6] = [
            ("notes/a.md", A),
            ("notes/b.md", B),
            ("notes/me/дар.md", "дар — a novel\n"),
            (&format!("notes/{YOGA_NFD}"), ""),
            (".plc/config", "[accounts]\nrevolut\n"),
            (".git/hidden.md", "[[дар]]\n"),
        ];
        for (rel, text) in files {
            let path = root.join(rel);
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, text).unwrap();
        }
        Vault(root)
    }

    fn uri(&self, rel: &str) -> String {
        Url::from_file_path(self.0.join(rel)).unwrap().to_string()
    }
}

impl Drop for Vault {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

struct Lsp {
    child: Child,
    stdin: ChildStdin,
    out: BufReader<ChildStdout>,
    next: i64,
    server_requests: Vec<String>,
    init: Value,
}

impl Lsp {
    fn start(vault: &Vault, utf8: bool, watch: bool) -> Self {
        let mut child = Command::new(env!("CARGO_BIN_EXE_plc-lsp"))
            .env("PALACE_DIR", &vault.0)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .unwrap();
        let stdin = child.stdin.take().unwrap();
        let out = BufReader::new(child.stdout.take().unwrap());
        let mut lsp = Lsp {
            child,
            stdin,
            out,
            next: 0,
            server_requests: Vec::new(),
            init: Value::Null,
        };
        let encodings = if utf8 {
            json!(["utf-8", "utf-16"])
        } else {
            json!([])
        };
        lsp.init = lsp.request(
            "initialize",
            json!({
                "capabilities": {
                    "general": { "positionEncodings": encodings },
                    "workspace": {
                        "didChangeWatchedFiles": {
                            "dynamicRegistration": watch
                        }
                    }
                }
            }),
        );
        lsp.notify("initialized", json!({}));
        lsp
    }

    fn send(&mut self, msg: Value) {
        let body = msg.to_string();
        write!(self.stdin, "Content-Length: {}\r\n\r\n{body}", body.len())
            .unwrap();
        self.stdin.flush().unwrap();
    }

    fn recv(&mut self) -> Value {
        let mut len = 0;
        loop {
            let mut header = String::new();
            assert!(self.out.read_line(&mut header).unwrap() > 0, "EOF");
            let header = header.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some(n) = header.strip_prefix("Content-Length: ") {
                len = n.parse().unwrap();
            }
        }
        let mut body = vec![0; len];
        self.out.read_exact(&mut body).unwrap();
        serde_json::from_slice(&body).unwrap()
    }

    fn notify(&mut self, method: &str, params: Value) {
        self.send(
            json!({"jsonrpc": "2.0", "method": method, "params": params}),
        );
    }

    /// Send a request and return its result, answering any request the
    /// server makes meanwhile with `null`.
    fn request(&mut self, method: &str, params: Value) -> Value {
        self.next += 1;
        let id = self.next;
        self.send(json!({
            "jsonrpc": "2.0", "id": id, "method": method, "params": params
        }));
        loop {
            let msg = self.recv();
            if let Some(m) = msg.get("method").and_then(Value::as_str) {
                if let Some(rid) = msg.get("id") {
                    self.server_requests.push(m.to_string());
                    let reply =
                        json!({"jsonrpc": "2.0", "id": rid, "result": null});
                    self.send(reply);
                }
                continue;
            }
            if msg["id"] == json!(id) {
                assert!(msg.get("error").is_none(), "{method}: {msg}");
                return msg["result"].clone();
            }
        }
    }

    fn open(&mut self, uri: &str, text: &str) {
        let doc = json!({
            "uri": uri, "languageId": "markdown", "version": 1, "text": text
        });
        self.notify("textDocument/didOpen", json!({ "textDocument": doc }));
    }

    fn at(&mut self, method: &str, uri: &str, pos: (u32, u32)) -> Value {
        let mut params = json!({
            "textDocument": { "uri": uri },
            "position": { "line": pos.0, "character": pos.1 },
        });
        if method == "textDocument/references" {
            params["context"] = json!({ "includeDeclaration": false });
        }
        self.request(method, params)
    }
}

impl Drop for Lsp {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Line and UTF-16 column just inside the first `needle` in `text`.
fn inside(text: &str, needle: &str) -> (u32, u32) {
    let at = text.find(needle).unwrap() + 2;
    let line_start = text[..at].rfind('\n').map_or(0, |i| i + 1);
    let line = text[..line_start].matches('\n').count() as u32;
    (line, text[line_start..at].encode_utf16().count() as u32)
}

fn targets(result: &Value) -> Vec<(String, u64)> {
    let mut out: Vec<_> = result
        .as_array()
        .unwrap()
        .iter()
        .map(|l| {
            let uri = l["uri"].as_str().unwrap().to_string();
            (uri, l["range"]["start"]["line"].as_u64().unwrap())
        })
        .collect();
    out.sort();
    out
}

#[test]
fn definition_resolves_notes_anchors_and_ledger() {
    let vault = Vault::new("def");
    let mut lsp = Lsp::start(&vault, false, false);
    let a = vault.uri("notes/a.md");
    let mut gd =
        |needle| lsp.at("textDocument/definition", &a, inside(A, needle));

    assert_eq!(targets(&gd("[[Дар]]")), [(vault.uri("notes/me/дар.md"), 0)]);
    assert_eq!(targets(&gd("[[b#Second")), [(vault.uri("notes/b.md"), 4)]);
    assert_eq!(targets(&gd("[[b#^blk")), [(vault.uri("notes/b.md"), 5)]);
    assert_eq!(targets(&gd("[[revolut")), [(vault.uri(".plc/config"), 1)]);
    let yoga = vault.uri(&format!("notes/{YOGA_NFD}"));
    assert_eq!(targets(&gd("[[Йога")), [(yoga, 0)]);
    assert_eq!(targets(&gd("[[#Local")), [(a.clone(), 4)]);
    assert_eq!(gd("[[nope"), Value::Null);
    assert_eq!(lsp.at("textDocument/definition", &a, (0, 1)), Value::Null);
}

#[test]
fn references_on_a_link_and_backlinks_of_a_note() {
    let vault = Vault::new("refs");
    let mut lsp = Lsp::start(&vault, false, false);
    let a = vault.uri("notes/a.md");
    let want = [(a.clone(), 1), (vault.uri("notes/b.md"), 6)];

    let on_link = lsp.at("textDocument/references", &a, inside(A, "[[Дар]]"));
    assert_eq!(targets(&on_link), want);

    let dar = vault.uri("notes/me/дар.md");
    lsp.open(&dar, "дар — a novel\n");
    let backlinks = lsp.at("textDocument/references", &dar, (0, 0));
    assert_eq!(targets(&backlinks), want);

    let with_decl = lsp.request(
        "textDocument/references",
        json!({
            "textDocument": { "uri": dar },
            "position": { "line": 0, "character": 0 },
            "context": { "includeDeclaration": true }
        }),
    );
    assert_eq!(targets(&with_decl).len(), 3);
}

#[test]
fn completion_offers_notes_and_ledger_in_utf8() {
    let vault = Vault::new("comp");
    let mut lsp = Lsp::start(&vault, true, false);
    assert_eq!(lsp.init["capabilities"]["positionEncoding"], "utf-8");

    let new = vault.uri("notes/new.md");
    lsp.open(&new, "x [[да\n");
    let items = lsp.at("textDocument/completion", &new, (0, 8));
    let items = items.as_array().unwrap();
    let dar = items.iter().find(|i| i["label"] == "дар").unwrap();
    assert_eq!(dar["textEdit"]["newText"], "дар]]");
    assert_eq!(dar["textEdit"]["range"]["start"]["character"], 4);
    assert_eq!(dar["detail"], "notes/me/дар.md");
    let revolut = items.iter().find(|i| i["label"] == "revolut").unwrap();
    assert_eq!(revolut["detail"], "account");

    let resolved = lsp.request("completionItem/resolve", dar.clone());
    let doc = resolved["documentation"]["value"].as_str().unwrap();
    assert!(doc.contains("a novel"), "{doc}");

    lsp.open(&new, "x [[да]]\n");
    let items = lsp.at("textDocument/completion", &new, (0, 8));
    let items = items.as_array().unwrap();
    let dar = items.iter().find(|i| i["label"] == "дар").unwrap();
    assert_eq!(dar["textEdit"]["newText"], "дар");

    let closed = lsp.at("textDocument/completion", &new, (0, 12));
    assert_eq!(closed, Value::Null);
}

fn labels(items: &Value) -> Vec<String> {
    let items = items.as_array().unwrap().iter();
    items
        .map(|i| i["label"].as_str().unwrap().to_string())
        .collect()
}

fn file_event(lsp: &mut Lsp, uri: String, kind: u8) {
    let event = json!({ "uri": uri, "type": kind });
    lsp.notify(
        "workspace/didChangeWatchedFiles",
        json!({ "changes": [event] }),
    );
}

#[test]
fn file_events_keep_a_watching_index_fresh() {
    let vault = Vault::new("watch");
    let mut lsp = Lsp::start(&vault, false, true);
    let scratch = vault.uri("notes/scratch.md");
    lsp.open(&scratch, "[[late]] [[\n");
    let complete = |lsp: &mut Lsp| {
        labels(&lsp.at("textDocument/completion", &scratch, (0, 11)))
    };

    fs::write(vault.0.join("notes/fresh.md"), "").unwrap();
    file_event(&mut lsp, vault.uri("notes/fresh.md"), 1);
    assert!(complete(&mut lsp).contains(&"fresh".into()));

    fs::remove_file(vault.0.join("notes/fresh.md")).unwrap();
    file_event(&mut lsp, vault.uri("notes/fresh.md"), 3);
    assert!(!complete(&mut lsp).contains(&"fresh".into()));

    fs::write(vault.0.join(".plc/config"), "[accounts]\nmonzo\n").unwrap();
    file_event(&mut lsp, vault.uri(".plc/config"), 2);
    let names = complete(&mut lsp);
    assert!(
        names.contains(&"monzo".into()) && !names.contains(&"revolut".into())
    );

    // Unannounced files stay unseen: the watcher is trusted, no rescan.
    fs::write(vault.0.join("notes/late.md"), "").unwrap();
    let gd = lsp.at("textDocument/definition", &scratch, (0, 3));
    assert_eq!(gd, Value::Null);

    let registered = "client/registerCapability".to_string();
    assert!(lsp.server_requests.contains(&registered));
}

#[test]
fn a_blind_index_rescans_on_a_definition_miss() {
    let vault = Vault::new("blind");
    let mut lsp = Lsp::start(&vault, false, false);
    let scratch = vault.uri("notes/scratch.md");
    lsp.open(&scratch, "[[late]]\n");
    assert_eq!(
        lsp.at("textDocument/definition", &scratch, (0, 3)),
        Value::Null
    );

    fs::write(vault.0.join("notes/late.md"), "").unwrap();
    let gd = lsp.at("textDocument/definition", &scratch, (0, 3));
    assert_eq!(targets(&gd), [(vault.uri("notes/late.md"), 0)]);
    assert!(lsp.server_requests.is_empty());
}
