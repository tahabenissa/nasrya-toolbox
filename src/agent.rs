//! The `a` key / agent icon: ask an AI to read the output pane and propose
//! next steps, as structured JSON.
//!
//! The provider is the user's choice (`A` opens the picker, saved in settings):
//!   - Claude   — the `claude` CLI (reuses your Claude Code login, no API key)
//!   - Gemini / DeepSeek / OpenAI — their OpenAI-compatible HTTP API, called
//!     through `curl` with the key from an env var
//!   - Ollama   — a local OpenAI-compatible server (http://localhost:11434)
//!
//! The same providers serve the side chat (`T`, see chat.rs), through `chat`.
//!
//! Whichever provider, the AI is the brain only — it gets no tool execution.
//! It returns suggestions that each name a winsploit catalog tool and its args;
//! winsploit runs a chosen one through its normal launch path (install /
//! conflict / arg checks, then an explicit Enter), so the agent can never run
//! an arbitrary shell command.

use std::io::Write;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

use serde::{Deserialize, Serialize};

use crate::detect;
use crate::runner::AppEvent;

/// Which AI the agent talks to. Stored in settings, so it's `Copy`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Provider {
    #[default]
    Claude,
    Gemini,
    Deepseek,
    Openai,
    Ollama,
}

/// Whether a provider can be used right now, else what it needs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Availability {
    Ready,
    NeedsCli(&'static str),
    NeedsKey(&'static str),
    NeedsCurl,
}

impl Provider {
    pub const ALL: [Provider; 5] = [
        Provider::Claude,
        Provider::Gemini,
        Provider::Deepseek,
        Provider::Openai,
        Provider::Ollama,
    ];

    /// Name shown in the picker.
    pub fn label(self) -> &'static str {
        match self {
            Provider::Claude => "Claude  (claude CLI)",
            Provider::Gemini => "Gemini  (API key)",
            Provider::Deepseek => "DeepSeek  (API key)",
            Provider::Openai => "OpenAI  (API key)",
            Provider::Ollama => "Ollama  (local)",
        }
    }

    /// Short name for the status line.
    pub fn short(self) -> &'static str {
        match self {
            Provider::Claude => "claude",
            Provider::Gemini => "gemini",
            Provider::Deepseek => "deepseek",
            Provider::Openai => "openai",
            Provider::Ollama => "ollama",
        }
    }

    /// The default model, used unless the user sets an override.
    pub fn default_model(self) -> &'static str {
        match self {
            Provider::Claude => "sonnet",
            Provider::Gemini => "gemini-2.0-flash",
            Provider::Deepseek => "deepseek-chat",
            Provider::Openai => "gpt-4o-mini",
            Provider::Ollama => "llama3.1",
        }
    }

    /// For HTTP providers: the OpenAI-compatible base URL and the env var the
    /// API key is read from (None for Ollama). `None` overall means a CLI
    /// provider (Claude).
    fn http(self) -> Option<(&'static str, Option<&'static str>)> {
        match self {
            Provider::Claude => None,
            Provider::Gemini => Some((
                "https://generativelanguage.googleapis.com/v1beta/openai",
                Some("GEMINI_API_KEY"),
            )),
            Provider::Deepseek => Some(("https://api.deepseek.com", Some("DEEPSEEK_API_KEY"))),
            Provider::Openai => Some(("https://api.openai.com/v1", Some("OPENAI_API_KEY"))),
            Provider::Ollama => Some(("http://localhost:11434/v1", None)),
        }
    }

    /// Can this provider run right now?
    pub fn availability(self) -> Availability {
        match self.http() {
            None => {
                if detect::resolve("claude").is_some() {
                    Availability::Ready
                } else {
                    Availability::NeedsCli("claude")
                }
            }
            Some((_, key_env)) => {
                if detect::resolve("curl").is_none() {
                    return Availability::NeedsCurl;
                }
                match key_env {
                    Some(k) if std::env::var_os(k).is_none() => Availability::NeedsKey(k),
                    // Ollama (no key) is "ready" if curl is there; a dead server
                    // is reported when the call is actually made.
                    _ => Availability::Ready,
                }
            }
        }
    }

    /// A short human note for the picker: "ready" or what's missing.
    pub fn status_note(self) -> String {
        match self.availability() {
            Availability::Ready => "ready".into(),
            Availability::NeedsCli(e) => format!("install the {e} CLI"),
            Availability::NeedsKey(k) => format!("set {k}"),
            Availability::NeedsCurl => "needs curl on PATH".into(),
        }
    }
}

/// The structured analysis the AI returns.
#[derive(Clone, Debug, Default, Deserialize)]
pub struct Analysis {
    #[serde(default)]
    pub summary: String,
    #[serde(default)]
    pub findings: Vec<String>,
    #[serde(default)]
    pub suggestions: Vec<Suggestion>,
}

/// One proposed next step: a catalog tool plus the args to run it with.
#[derive(Clone, Debug, Deserialize)]
pub struct Suggestion {
    pub tool: String,
    #[serde(default)]
    pub args: String,
    #[serde(default)]
    pub rationale: String,
}

/// Build the single prompt fed to the AI.
pub fn build_prompt(tool_names: &[String], ran: &str, output_tail: &str) -> String {
    format!(
        "You are a penetration-testing assistant embedded in \"winsploit\", a Windows \
security-tool launcher. You are analysing the output of a tool the user just ran, as part \
of an AUTHORISED engagement.\n\n\
The command that produced the output:\n\
$ {ran}\n\n\
Recent output (may be truncated):\n\
<<<OUTPUT\n{output_tail}\nOUTPUT\n\n\
winsploit can run ONLY these tools. Suggest follow-ups using these names EXACTLY, and omit \
a suggestion if none fit:\n{names}\n\n\
Respond with ONLY a JSON object (no prose, no markdown fences) of exactly this shape:\n\
{{\"summary\": \"...\", \"findings\": [\"...\"], \"suggestions\": \
[{{\"tool\": \"<one of the names above>\", \"args\": \"<args to pass>\", \"rationale\": \"<why>\"}}]}}\n\n\
Keep summary to 1-3 sentences. At most 5 findings and 4 suggestions. If the output is an \
error or shows nothing useful, say so in summary and return empty findings and suggestions.",
        ran = ran,
        output_tail = output_tail,
        names = tool_names.join(", "),
    )
}

/// Pull the first balanced `{...}` block out of `s` (the AI is asked for bare
/// JSON, but may still wrap it in prose or fences).
pub fn extract_json(s: &str) -> Option<&str> {
    let start = s.find('{')?;
    let bytes = s.as_bytes();
    let mut depth = 0i32;
    let mut in_str = false;
    let mut escaped = false;
    for i in start..bytes.len() {
        let c = bytes[i] as char;
        if in_str {
            if escaped {
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if c == '"' {
                in_str = false;
            }
            continue;
        }
        match c {
            '"' => in_str = true,
            '{' => depth += 1,
            '}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&s[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

/// Parse an `Analysis` out of whatever text the model produced.
fn analysis_from_text(text: &str) -> Result<Analysis, String> {
    let json =
        extract_json(text).ok_or_else(|| "the AI didn't return JSON — try again".to_string())?;
    serde_json::from_str::<Analysis>(json)
        .map_err(|e| format!("the AI's JSON didn't match the expected shape ({e})"))
}

// ---- Claude Code `--output-format json` envelope -----------------------

#[derive(Deserialize)]
struct Envelope {
    #[serde(default)]
    result: String,
    #[serde(default)]
    is_error: bool,
}

/// The model's text out of a Claude Code JSON-envelope stdout.
fn envelope_text(stdout: &str) -> Result<String, String> {
    let env: Envelope = serde_json::from_str(stdout.trim())
        .map_err(|e| format!("couldn't read claude's response ({e})"))?;
    if env.is_error {
        return Err(format!("claude reported an error: {}", env.result.trim()));
    }
    Ok(env.result)
}

/// Parse a Claude Code JSON-envelope stdout into an `Analysis`.
#[cfg(test)]
pub fn parse_envelope(stdout: &str) -> Result<Analysis, String> {
    analysis_from_text(&envelope_text(stdout)?)
}

// ---- OpenAI-compatible chat response -----------------------------------

#[derive(Deserialize)]
struct ChatResponse {
    #[serde(default)]
    choices: Vec<ChatChoice>,
    #[serde(default)]
    error: Option<ChatError>,
}
#[derive(Deserialize)]
struct ChatChoice {
    #[serde(default)]
    message: ChatMessage,
}
#[derive(Deserialize, Default)]
struct ChatMessage {
    #[serde(default)]
    content: String,
}
#[derive(Deserialize)]
struct ChatError {
    #[serde(default)]
    message: String,
}

/// Build the JSON body for an OpenAI-compatible `/chat/completions` call.
pub fn build_http_body(model: &str, prompt: &str) -> String {
    serde_json::json!({
        "model": model,
        "messages": [{ "role": "user", "content": prompt }],
        "temperature": 0.2,
        "stream": false,
    })
    .to_string()
}

/// The model's text out of an OpenAI-compatible chat response.
fn chat_text(stdout: &str) -> Result<String, String> {
    let resp: ChatResponse = serde_json::from_str(stdout.trim()).map_err(|e| {
        format!(
            "couldn't read the AI's response ({e}) — raw: {:.200}",
            stdout.trim()
        )
    })?;
    if let Some(err) = resp.error {
        return Err(format!("the API returned an error: {}", err.message.trim()));
    }
    let content = resp
        .choices
        .first()
        .map(|c| c.message.content.as_str())
        .unwrap_or("");
    if content.is_empty() {
        return Err("the API returned no content".into());
    }
    Ok(content.to_string())
}

/// Parse an OpenAI-compatible chat response into an `Analysis`.
#[cfg(test)]
pub fn parse_chat(stdout: &str) -> Result<Analysis, String> {
    analysis_from_text(&chat_text(stdout)?)
}

// ---- running the chosen provider ---------------------------------------

/// Run the chosen provider (with `model`) on a background thread; the result
/// arrives as `AppEvent::Agent`.
pub fn analyze(
    provider: Provider,
    model: String,
    tool_names: Vec<String>,
    ran: String,
    output_tail: String,
    tx: Sender<AppEvent>,
) {
    thread::spawn(move || {
        let prompt = build_prompt(&tool_names, &ran, &output_tail);
        let result = ask_text(provider, &model, &prompt).and_then(|t| analysis_from_text(&t));
        let _ = tx.send(AppEvent::Agent(result));
    });
}

/// Send `prompt` to the chat panel's provider on a background thread; the
/// model's raw text arrives as `AppEvent::Chat { req, .. }`.
pub fn chat(provider: Provider, model: String, prompt: String, req: u64, tx: Sender<AppEvent>) {
    thread::spawn(move || {
        let result = ask_text(provider, &model, &prompt);
        let _ = tx.send(AppEvent::Chat { req, result });
    });
}

/// One blocking round trip: `prompt` in, the model's text out.
fn ask_text(provider: Provider, model: &str, prompt: &str) -> Result<String, String> {
    match provider.http() {
        None => run_claude(model, prompt),
        Some((base_url, key_env)) => run_http(base_url, model, key_env, prompt),
    }
}

/// The system prompt the `claude` CLI gets instead of its own coding-agent
/// one (which is ~49k tokens of overhead on every call). No quotes or shell
/// characters: it travels as one argument, even through a `.cmd` shim.
const CLAUDE_SYSTEM_PROMPT: &str = "You are a JSON-only assistant embedded in a security tool \
launcher. You have no tools. Follow the output format you are given exactly.";

/// A scratch folder for the `claude` CLI to run in, holding a settings file
/// that turns hooks off. Returns the settings file's path.
fn claude_scratch() -> Result<(std::path::PathBuf, std::path::PathBuf), String> {
    let dir = std::env::temp_dir().join("winsploit-ai");
    std::fs::create_dir_all(&dir).map_err(|e| format!("couldn't make {}: {e}", dir.display()))?;
    let settings = dir.join("settings.json");
    std::fs::write(&settings, r#"{"disableAllHooks": true}"#)
        .map_err(|e| format!("couldn't write {}: {e}", settings.display()))?;
    Ok((dir, settings))
}

/// The arguments for one isolated `claude` call.
///
/// The prompt carries a tool's output, which is untrusted text, and the CLI
/// inherits the user's own Claude Code setup: their permission mode, hooks
/// (a Stop hook once hijacked a request and edited a file), MCP servers and
/// memory files. So the call gets no tools at all (`--tools ""`), no MCP
/// servers, no hooks, no slash commands and no session record, and runs in a
/// scratch folder. `--bare` would skip more, but it never reads the login.
fn claude_args(model: &str, settings: &std::path::Path) -> Vec<String> {
    let mut args: Vec<String> = [
        "-p",
        "--output-format",
        "json",
        "--model",
        model,
        "--tools",
        "",
        "--strict-mcp-config",
        "--disable-slash-commands",
        "--no-session-persistence",
        "--system-prompt",
        CLAUDE_SYSTEM_PROMPT,
        "--settings",
    ]
    .iter()
    .map(|s| s.to_string())
    .collect();
    args.push(settings.display().to_string());
    args
}

/// The `claude` CLI in isolated print mode, returning the text of its JSON envelope.
fn run_claude(model: &str, prompt: &str) -> Result<String, String> {
    let path =
        detect::resolve("claude").ok_or_else(|| "the claude CLI isn't on PATH".to_string())?;
    let (dir, settings) = claude_scratch()?;
    let out = feed(
        Command::new(path)
            .args(claude_args(model, &settings))
            .current_dir(dir),
        prompt,
    )?;
    if !out.status.success() {
        let err = String::from_utf8_lossy(&out.stderr).to_lowercase();
        // fail closed: never fall back to a call without the isolation flags
        if err.contains("unknown option") || err.contains("unknown argument") {
            return Err("your claude CLI is too old for winsploit's isolated mode \
                        (it needs --tools); update it with `claude update`, or press A and \
                        pick another provider"
                .into());
        }
        return Err(exit_message("claude", &out));
    }
    envelope_text(&String::from_utf8_lossy(&out.stdout))
}

fn run_http(
    base_url: &str,
    model: &str,
    key_env: Option<&str>,
    prompt: &str,
) -> Result<String, String> {
    let curl = detect::resolve("curl").ok_or_else(|| "curl isn't on PATH".to_string())?;
    let url = format!("{}/chat/completions", base_url.trim_end_matches('/'));
    let body = build_http_body(model, prompt);

    let mut cmd = Command::new(curl);
    cmd.arg("-sS")
        .arg("-X")
        .arg("POST")
        .arg(&url)
        .arg("-H")
        .arg("Content-Type: application/json");
    if let Some(env) = key_env {
        let key = std::env::var(env).map_err(|_| format!("{env} is not set"))?;
        // NOTE: the key is passed as a curl header argument; on a shared box it
        // is briefly visible in the process list. Fine for a single-user host.
        cmd.arg("-H").arg(format!("Authorization: Bearer {key}"));
    }
    cmd.arg("--data-binary").arg("@-"); // body comes in on stdin

    let out = feed(&mut cmd, &body)?;
    if !out.status.success() {
        return Err(exit_message("curl", &out));
    }
    chat_text(&String::from_utf8_lossy(&out.stdout))
}

/// Spawn `cmd`, write `input` to its stdin, and collect its output.
fn feed(cmd: &mut Command, input: &str) -> Result<std::process::Output, String> {
    let mut child = cmd
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("couldn't start the request: {e}"))?;
    if let Some(mut stdin) = child.stdin.take() {
        stdin
            .write_all(input.as_bytes())
            .map_err(|e| format!("couldn't send the prompt: {e}"))?;
        // dropping stdin closes it, so the child sees EOF and starts working
    }
    child
        .wait_with_output()
        .map_err(|e| format!("the request didn't finish: {e}"))
}

fn exit_message(what: &str, out: &std::process::Output) -> String {
    let err = String::from_utf8_lossy(&out.stderr);
    let msg = err.trim();
    if msg.is_empty() {
        format!("{what} exited with {}", out.status)
    } else {
        format!("{what} failed: {msg}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extract_json_finds_the_balanced_object() {
        let s = "sure:\n```json\n{\"a\": {\"b\": 1}}\n```\nhope that helps";
        assert_eq!(extract_json(s), Some("{\"a\": {\"b\": 1}}"));
    }

    #[test]
    fn extract_json_ignores_braces_inside_strings() {
        let s = r#"{"cmd": "echo }{ ", "n": 1}"#;
        assert_eq!(extract_json(s), Some(s));
    }

    #[test]
    fn parses_a_claude_envelope() {
        let inner = r#"{"summary":"2 ports","findings":["22 ssh"],"suggestions":[{"tool":"nuclei","args":"-u http://t","rationale":"scan"}]}"#;
        let envelope = serde_json::json!({ "is_error": false, "result": inner }).to_string();
        let a = parse_envelope(&envelope).expect("parse");
        assert_eq!(a.summary, "2 ports");
        assert_eq!(a.suggestions[0].tool, "nuclei");
    }

    #[test]
    fn parses_an_openai_chat_response() {
        let content = r#"Here you go: {"summary":"ok","findings":[],"suggestions":[{"tool":"ffuf","args":"-u x"}]}"#;
        let resp =
            serde_json::json!({ "choices": [{ "message": { "content": content } }] }).to_string();
        let a = parse_chat(&resp).expect("parse");
        assert_eq!(a.summary, "ok");
        assert_eq!(a.suggestions[0].tool, "ffuf");
    }

    #[test]
    fn the_claude_call_is_isolated_from_the_users_setup() {
        let args = claude_args(
            "sonnet",
            std::path::Path::new("C:/tmp/winsploit-ai/settings.json"),
        );
        let at = |flag: &str| {
            args.iter()
                .position(|a| a == flag)
                .unwrap_or_else(|| panic!("{flag} missing: {args:?}"))
        };
        // no tools at all: an empty value right after --tools
        assert_eq!(args[at("--tools") + 1], "");
        for flag in [
            "--strict-mcp-config",
            "--disable-slash-commands",
            "--no-session-persistence",
        ] {
            at(flag);
        }
        assert_eq!(
            args[at("--settings") + 1],
            "C:/tmp/winsploit-ai/settings.json"
        );
        assert_eq!(args[at("--model") + 1], "sonnet");
        assert_eq!(args[at("--system-prompt") + 1], CLAUDE_SYSTEM_PROMPT);
        // never the dangerous switches
        for bad in [
            "--dangerously-skip-permissions",
            "--allow-dangerously-skip-permissions",
            "--permission-mode",
            "--allowedTools",
        ] {
            assert!(!args.iter().any(|a| a == bad), "{bad} must not be passed");
        }
        // the prompt travels as one plain argument, even through a .cmd shim
        assert!(!CLAUDE_SYSTEM_PROMPT.contains(['"', '%', '&', '|', '<', '>', '^', '\n']));
    }

    #[test]
    fn the_scratch_folder_turns_hooks_off() {
        let (dir, settings) = claude_scratch().unwrap();
        assert!(dir.is_dir());
        let json: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(&settings).unwrap()).unwrap();
        assert_eq!(json["disableAllHooks"], true);
        // the folder is neutral: not the working folder, not a project
        assert_ne!(dir, std::env::current_dir().unwrap());
        assert!(!dir.join("CLAUDE.md").exists());
    }

    #[test]
    fn the_text_helpers_return_the_model_text_untouched() {
        // the chat panel wants the raw text, not an Analysis
        let envelope = serde_json::json!({ "is_error": false, "result": "hello {\"a\":1}" });
        assert_eq!(
            envelope_text(&envelope.to_string()).unwrap(),
            "hello {\"a\":1}"
        );
        let resp = serde_json::json!({ "choices": [{ "message": { "content": "hi there" } }] });
        assert_eq!(chat_text(&resp.to_string()).unwrap(), "hi there");
        // errors still surface
        let bad = serde_json::json!({ "is_error": true, "result": "rate limited" });
        assert!(envelope_text(&bad.to_string())
            .unwrap_err()
            .contains("rate limited"));
        assert!(envelope_text("not json").is_err());
        assert!(chat_text(r#"{"choices": []}"#).is_err());
        assert!(chat_text(r#"{"error": {"message": "nope"}}"#).is_err());
    }

    #[test]
    fn surfaces_api_errors() {
        let resp = r#"{"error": {"message": "invalid api key"}}"#;
        assert!(parse_chat(resp).is_err());
    }

    #[test]
    fn http_body_has_model_and_prompt() {
        let b = build_http_body("deepseek-chat", "hello");
        assert!(b.contains("deepseek-chat"));
        assert!(b.contains("hello"));
        assert!(b.contains("messages"));
    }

    #[test]
    fn provider_presets_are_sane() {
        assert_eq!(Provider::ALL.len(), 5);
        for p in Provider::ALL {
            assert!(!p.label().is_empty());
            assert!(!p.short().is_empty());
            assert!(!p.status_note().is_empty());
            assert!(!p.default_model().is_empty());
        }
        // Claude is the CLI provider; the rest are OpenAI-compatible HTTP.
        assert!(Provider::Claude.http().is_none());
        assert!(Provider::Deepseek.http().is_some());
    }
}
