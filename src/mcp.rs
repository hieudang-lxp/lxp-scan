use serde_json::{Value, json};
use std::io::{BufRead, Write};
use std::path::Path;

use crate::features::{clones, context, drift, dupes, impact};
use crate::output::report;

/// Minimal MCP stdio server: newline-delimited JSON-RPC 2.0 on stdin/stdout.
/// Exposes the scan commands as tools so coding agents (Claude Code) can pull
/// cross-repo ground truth themselves instead of hallucinating it. No SDK
/// dependency — the protocol subset needed (initialize / tools/list /
/// tools/call / ping) is small and stable.
pub fn serve(root: &Path) -> anyhow::Result<()> {
    let stdin = std::io::stdin();
    let mut stdout = std::io::stdout();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let Ok(message) = serde_json::from_str::<Value>(&line) else {
            continue;
        };
        if let Some(response) = handle_message(root, &message) {
            serde_json::to_writer(&mut stdout, &response)?;
            stdout.write_all(b"\n")?;
            stdout.flush()?;
        }
    }
    Ok(())
}

/// One request in, optional response out (notifications get none).
fn handle_message(root: &Path, message: &Value) -> Option<Value> {
    let method = message.get("method")?.as_str()?;
    // Requests carry an id; notifications (e.g. notifications/initialized)
    // must not be answered.
    let id = message.get("id")?.clone();
    let result = match method {
        "initialize" => json!({
            "protocolVersion": message["params"]["protocolVersion"]
                .as_str()
                .unwrap_or("2025-06-18"),
            "capabilities": { "tools": {} },
            "serverInfo": {
                "name": "lxp-scan",
                "version": env!("CARGO_PKG_VERSION"),
            },
        }),
        "ping" => json!({}),
        "tools/list" => json!({ "tools": tool_definitions() }),
        "tools/call" => call_tool(root, &message["params"]),
        _ => {
            return Some(json!({
                "jsonrpc": "2.0",
                "id": id,
                "error": { "code": -32601, "message": format!("method not found: {method}") },
            }));
        }
    };
    Some(json!({ "jsonrpc": "2.0", "id": id, "result": result }))
}

fn tool_definitions() -> Value {
    let symbol = json!({ "type": "string", "description": "Exported symbol name, e.g. Avatar" });
    let from = json!({ "type": "string", "description": "Substring filter on the resolved import source, e.g. lxp-common-components-js or lxp-web/src" });
    json!([
        {
            "name": "impact",
            "description": "Find every usage site of a symbol across all FE repos: file:line, import source, JSX renders and props. Use before changing a shared component/util to see the blast radius.",
            "inputSchema": {
                "type": "object",
                "properties": { "symbol": symbol, "from": from },
                "required": ["symbol"],
            },
        },
        {
            "name": "context",
            "description": "Build an LLM-ready context pack for a symbol: real definition (through barrel re-exports), props usage frequency, and representative usage excerpts. Use to ground work on a component you haven't read yet.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "symbol": symbol,
                    "from": from,
                    "sites": { "type": "integer", "description": "Max usage excerpts (default 8)" },
                },
                "required": ["symbol"],
            },
        },
        {
            "name": "drift",
            "description": "Show lxp-common-* / lxp-design-system version drift across the FE repos.",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "dupes",
            "description": "List same-name exported components declared in more than one repo — parallel implementations that are candidates for consolidation.",
            "inputSchema": { "type": "object", "properties": {} },
        },
        {
            "name": "clones",
            "description": "Find name-agnostic structural clones: top-level functions with identical normalized bodies across repos, even under different names (e.g. isEmail vs validateEmail). Complementary to dupes, which matches names only.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "symbol": { "type": "string", "description": "Only clusters containing this declaration name" },
                    "min_tokens": { "type": "integer", "description": "Minimum normalized body tokens (default 10) — lower to catch one-liners, raise to cut noise" },
                    "same_file": { "type": "boolean", "description": "Also report clusters whose members live in one file" },
                },
            },
        },
    ])
}

fn call_tool(root: &Path, params: &Value) -> Value {
    let name = params["name"].as_str().unwrap_or("");
    let args = &params["arguments"];
    let symbol = args["symbol"].as_str();
    let from = args["from"].as_str();
    let mut warnings = Vec::new();

    let output: anyhow::Result<String> = match (name, symbol) {
        ("impact", Some(symbol)) => {
            impact::run_impact(root, symbol, from, &mut warnings).map(|hits| {
                format!(
                    "{}\n{} usage site(s)\n",
                    report::impact_report(&hits),
                    hits.len()
                )
            })
        }
        ("context", Some(symbol)) => {
            let sites = args["sites"].as_u64().unwrap_or(8) as usize;
            context::build_context(root, symbol, from, sites, &mut warnings)
                .map(|pack| report::context_markdown(&pack, &root.display().to_string()))
        }
        ("drift", _) => crate::scan::discover::discover_repos(root, &mut warnings).map(|repos| {
            let rows = drift::compute_drift(&repos);
            let names: Vec<String> = repos.iter().map(|r| r.name.clone()).collect();
            report::drift_table(&rows, &names)
        }),
        ("dupes", _) => {
            dupes::find_dupes(root, &mut warnings).map(|groups| report::dupes_report(&groups))
        }
        ("clones", _) => {
            let mut opts = clones::CloneOptions {
                symbol: args["symbol"].as_str().map(String::from),
                same_file: args["same_file"].as_bool().unwrap_or(false),
                ..Default::default()
            };
            if let Some(n) = args["min_tokens"].as_u64() {
                opts.min_tokens = n as usize;
            }
            clones::find_clones(root, &opts, &mut warnings).map(|out| {
                format!(
                    "{}\n{} clone cluster(s)\n",
                    report::clones_report(&out),
                    out.clusters.len()
                )
            })
        }
        ("impact" | "context", None) => Err(anyhow::anyhow!("missing required argument: symbol")),
        _ => Err(anyhow::anyhow!("unknown tool: {name}")),
    };

    match output {
        Ok(mut text) => {
            if !warnings.is_empty() {
                text.push_str(&format!("\n({} warning(s) suppressed)\n", warnings.len()));
            }
            json!({ "content": [{ "type": "text", "text": text }], "isError": false })
        }
        Err(err) => json!({
            "content": [{ "type": "text", "text": format!("error: {err:#}") }],
            "isError": true,
        }),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn workspace() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/workspace")
    }

    fn request(method: &str, params: Value) -> Value {
        json!({ "jsonrpc": "2.0", "id": 1, "method": method, "params": params })
    }

    #[test]
    fn initialize_reports_tools_capability_and_echoes_version() {
        let resp = handle_message(
            &workspace(),
            &request("initialize", json!({ "protocolVersion": "2025-06-18" })),
        )
        .unwrap();
        assert_eq!(resp["result"]["protocolVersion"], "2025-06-18");
        assert!(resp["result"]["capabilities"]["tools"].is_object());
        assert_eq!(resp["result"]["serverInfo"]["name"], "lxp-scan");
    }

    #[test]
    fn notifications_get_no_response() {
        let note = json!({ "jsonrpc": "2.0", "method": "notifications/initialized" });
        assert!(handle_message(&workspace(), &note).is_none());
    }

    #[test]
    fn tools_list_exposes_all_five_tools() {
        let resp = handle_message(&workspace(), &request("tools/list", json!({}))).unwrap();
        let names: Vec<&str> = resp["result"]["tools"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["name"].as_str().unwrap())
            .collect();
        assert_eq!(names, vec!["impact", "context", "drift", "dupes", "clones"]);
    }

    #[test]
    fn tools_call_clones_returns_cluster_text() {
        let resp = handle_message(
            &workspace(),
            &request(
                "tools/call",
                json!({ "name": "clones", "arguments": { "symbol": "validateEmail" } }),
            ),
        )
        .unwrap();
        assert_eq!(resp["result"]["isError"], false);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("CLONE CLUSTER #1"), "{text}");
        assert!(text.contains("isEmail"), "{text}");
    }

    #[test]
    fn tools_call_impact_returns_grouped_text() {
        let resp = handle_message(
            &workspace(),
            &request(
                "tools/call",
                json!({ "name": "impact", "arguments": { "symbol": "Button", "from": "fake-lib" } }),
            ),
        )
        .unwrap();
        assert_eq!(resp["result"]["isError"], false);
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.contains("app-one"));
        assert!(text.contains("from fake-lib/components/Button"));
    }

    #[test]
    fn tools_call_context_returns_markdown_pack() {
        let resp = handle_message(
            &workspace(),
            &request(
                "tools/call",
                json!({ "name": "context", "arguments": { "symbol": "Button", "sites": 2 } }),
            ),
        )
        .unwrap();
        let text = resp["result"]["content"][0]["text"].as_str().unwrap();
        assert!(text.starts_with("# Context: Button"));
        assert!(text.contains("## Definition"));
    }

    #[test]
    fn missing_symbol_and_unknown_tool_are_tool_errors_not_crashes() {
        let missing = handle_message(
            &workspace(),
            &request("tools/call", json!({ "name": "impact", "arguments": {} })),
        )
        .unwrap();
        assert_eq!(missing["result"]["isError"], true);
        let unknown = handle_message(
            &workspace(),
            &request("tools/call", json!({ "name": "nope", "arguments": {} })),
        )
        .unwrap();
        assert_eq!(unknown["result"]["isError"], true);
    }

    #[test]
    fn unknown_method_returns_jsonrpc_error() {
        let resp = handle_message(&workspace(), &request("bogus/method", json!({}))).unwrap();
        assert_eq!(resp["error"]["code"], -32601);
    }
}
