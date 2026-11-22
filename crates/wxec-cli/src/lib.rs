use std::collections::BTreeMap;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde_json::{json, Map, Value};

use wxec_core::config;
use wxec_core::dispatcher::Dispatcher;
use wxec_core::error::{Result, WErr};
use wxec_core::parsers::NoParsers;
use wxec_core::stats;
use wxec_core::types::Tier;
use wxec_parsers::ParserRegistry;

pub struct Global {
    pub root: PathBuf,
    pub session: String,
    pub format: Option<String>,
    pub budget: Option<u64>,
    pub config: Option<PathBuf>,
    pub no_project_config: bool,
    pub tier: Option<String>,
    pub verbosity: Option<String>,
}

impl Default for Global {
    fn default() -> Self {
        Self {
            root: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            session: "default".to_string(),
            format: None,
            budget: None,
            config: None,
            no_project_config: false,
            tier: None,
            verbosity: None,
        }
    }
}

const BOOLEANS: &[&str] = &[
    "outline",
    "sizes",
    "staged",
    "stage",
    "auto-stage",
    "stop-on-error",
    "fail-fast",
    "all",
    "allow-empty",
    "set-upstream",
    "create",
    "delete",
    "overwrite",
    "stats",
    "json",
    "quiet",
    "force",
    "word",
    "regex",
    "dry_run",
    "dry-run",
    "cached",
    "stat",
    "name-only",
    "name-status",
    "numstat",
    "shortstat",
];

pub fn run(argv: Vec<String>) -> i32 {
    match run_inner(argv) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("{}", e.render());
            e.cli_exit()
        }
    }
}

fn run_inner(argv: Vec<String>) -> Result<i32> {
    if argv.is_empty() {
        print_help();
        return Ok(0);
    }
    if argv.first().map(String::as_str) == Some("--version")
        || argv.first().map(String::as_str) == Some("-V")
    {
        println!("w {}", env!("CARGO_PKG_VERSION"));
        return Ok(0);
    }
    if matches!(
        argv.first().map(String::as_str),
        Some("-h" | "--help" | "help")
    ) {
        print_help();
        return Ok(0);
    }

    let (global, tool, rest) = parse_global(&argv)?;
    let Some(tool) = tool else {
        print_help();
        return Ok(0);
    };

    let dd = rest.iter().position(|a| a == "--");
    let help_idx = rest.iter().position(|a| a == "--help" || a == "-h");
    if let Some(h) = help_idx {
        let before_dd = dd.map(|d| h < d).unwrap_or(true);
        let show_help = if matches!(tool.as_str(), "run" | "sh") {
            before_dd && (h == 0 || rest.len() == 1)
        } else {
            before_dd
        };
        if show_help {
            let registry = wxec_tools::registry();
            if let Some(t) = registry.get(&tool) {
                println!("w {}: {}", t.name(), t.description());
                println!("tier: {}", t.min_tier());
                println!(
                    "{}",
                    serde_json::to_string_pretty(&t.schema()).unwrap_or_default()
                );
                return Ok(0);
            }
            print_help();
            return Ok(0);
        }
    }

    match tool.as_str() {
        "gain" => return cmd_gain(&rest),
        "discover" => return cmd_discover(&rest),
        "parser" => return cmd_parser(&rest),
        "handles" => return cmd_handles(&rest, &global),
        "config" => return cmd_config(&rest, &global),
        _ => {}
    }

    if tool == "root" || tool == "help" {
        print_help();
        return Ok(0);
    }

    let mut obj = resolve_tool_args(&tool, &rest)?;

    if let Some(format) = &global.format {
        obj.insert("format".into(), Value::String(format.clone()));
    }
    if let Some(budget) = global.budget {
        obj.insert("budget".into(), Value::Number(budget.into()));
    }
    if let Some(path) = obj.get("calls_file").and_then(Value::as_str) {
        let text = std::fs::read_to_string(path)
            .map_err(|e| WErr::new("invalid_args", format!("calls_file '{path}': {e}")))?;
        let calls: Value = serde_json::from_str(&text).map_err(|e| {
            WErr::invalid_args(
                format!("calls_file '{path}': {e}"),
                "expected a JSON array of {tool,args}",
            )
        })?;
        obj.remove("calls_file");
        obj.insert("calls".into(), calls);
    }

    let mut dispatcher = build_dispatcher(&global, true)?;
    if let Some(v) = &global.verbosity {
        dispatcher.config_mut().output.verbosity = v.clone();
    }
    if let Some(tier) = &global.tier {
        let t: Tier = tier.parse()?;
        dispatcher.set_session_tier(&global.session, t)?;
    } else {
        dispatcher.set_session_tier(&global.session, Tier::GitWrite)?;
    }
    let rendered = dispatcher.call(&tool, &Value::Object(obj), &global.session);
    let mut out = std::io::stdout().lock();
    out.write_all(rendered.text.as_bytes())
        .map_err(|e| WErr::internal(format!("stdout: {e}")))?;
    out.flush().ok();
    Ok(rendered.cli_code)
}

pub fn build_dispatcher(global: &Global, with_parsers: bool) -> Result<Dispatcher> {
    let (cfg, warnings) = config::load(
        &global.root,
        global.config.as_deref(),
        global.no_project_config,
    )?;
    for w in warnings {
        eprintln!("# {w}");
    }
    let registry = Arc::new(wxec_tools::registry());
    if with_parsers {
        let parsers = Arc::new(ParserRegistry::for_root(
            &global.root,
            global.no_project_config,
        ));
        Dispatcher::new(cfg, &global.root, registry, parsers)
    } else {
        Dispatcher::new(cfg, &global.root, registry, Arc::new(NoParsers))
    }
}

fn parse_global(argv: &[String]) -> Result<(Global, Option<String>, Vec<String>)> {
    let mut g = Global::default();
    let mut i = 0usize;
    while i < argv.len() {
        let a = &argv[i];
        match a.as_str() {
            "--root" => {
                g.root = PathBuf::from(next(argv, &mut i, "--root")?);
            }
            "--session" => {
                g.session = next(argv, &mut i, "--session")?;
            }
            "--format" => {
                g.format = Some(next(argv, &mut i, "--format")?);
            }
            "--budget" => {
                let v = next(argv, &mut i, "--budget")?;
                g.budget = Some(v.parse::<u64>().map_err(|_| {
                    WErr::invalid_args(format!("bad --budget '{v}'"), "--budget 600")
                })?);
            }
            "--config" => {
                g.config = Some(PathBuf::from(next(argv, &mut i, "--config")?));
            }
            "--tier" => {
                g.tier = Some(next(argv, &mut i, "--tier")?);
            }
            "--verbosity" => {
                g.verbosity = Some(next(argv, &mut i, "--verbosity")?);
            }
            "--no-project-config" => {
                g.no_project_config = true;
                i += 1;
            }
            "-q" | "--quiet" => {
                i += 1;
            }
            "-h" | "--help" => {
                return Ok((g, None, Vec::new()));
            }
            other if other.starts_with('-') => {
                return Err(WErr::invalid_args(
                    format!("unknown global option '{other}'"),
                    "globals: --root --session --format --budget --config --tier --verbosity --no-project-config",
                ));
            }
            _ => {
                return Ok((g, Some(canonical_tool(a)), argv[i + 1..].to_vec()));
            }
        }
    }
    Ok((g, None, Vec::new()))
}

fn canonical_tool(name: &str) -> String {
    match name {
        "r" => "read",
        "s" => "search",
        "e" => "edit",
        "t" => "tree",
        "x" => "expand",
        "g" => "git",
        "b" => "batch",
        "u" => "run",
        "c" => "check",
        "p" => "replace",
        _ => name,
    }
    .to_string()
}

fn next(argv: &[String], i: &mut usize, flag: &str) -> Result<String> {
    if *i + 1 >= argv.len() {
        return Err(WErr::invalid_args(
            format!("{flag} requires a value"),
            format!("{flag} <value>"),
        ));
    }
    *i += 2;
    Ok(argv[*i - 1].clone())
}

fn kebab_to_snake(s: &str) -> String {
    s.replace('-', "_")
}

type ParsedToolArgs = (Map<String, Value>, Vec<String>, Vec<String>);

fn parse_tool_args(tokens: &[String], tool: &str) -> Result<ParsedToolArgs> {
    let mut obj = Map::new();
    let mut positionals = Vec::new();
    let mut after_dd = Vec::new();
    let mut i = 0usize;
    while i < tokens.len() {
        let tok = &tokens[i];
        if tok == "--" {
            if tool == "git" {
                positionals.push("--".to_string());
                positionals.extend(tokens[i + 1..].iter().cloned());
            } else {
                after_dd.extend(tokens[i + 1..].iter().cloned());
            }
            break;
        }
        if tok == "-m" && tool == "git" {
            i += 1;
            if i >= tokens.len() {
                return Err(WErr::invalid_args(
                    "-m requires a message",
                    "-m \"message\"",
                ));
            }
            insert_value(&mut obj, "message", Value::String(tokens[i].clone()));
            i += 1;
            continue;
        }
        if tool == "git" {
            if tok == "-n" {
                i += 1;
                if i >= tokens.len() {
                    return Err(WErr::invalid_args(
                        "-n requires a count",
                        "w git log -n <count>",
                    ));
                }
                let n = parse_count(&tokens[i], "-n")?;
                obj.insert("limit".into(), Value::Number((n as i64).into()));
                i += 1;
                continue;
            }
            if let Some(nstr) = tok.strip_prefix("-n") {
                if !nstr.is_empty() && nstr.chars().all(|c| c.is_ascii_digit()) {
                    let n = parse_count(nstr, "-n")?;
                    obj.insert("limit".into(), Value::Number((n as i64).into()));
                    i += 1;
                    continue;
                }
            }
            if let Some(nstr) = tok.strip_prefix('-') {
                if !nstr.is_empty() && nstr.chars().all(|c| c.is_ascii_digit()) {
                    let n = parse_count(nstr, "-<count>")?;
                    obj.insert("limit".into(), Value::Number((n as i64).into()));
                    i += 1;
                    continue;
                }
            }
            if tok == "-U" {
                i += 1;
                if i >= tokens.len() {
                    return Err(WErr::invalid_args(
                        "-U requires a number",
                        "w git diff -U <n>",
                    ));
                }
                let n: u32 = tokens[i].parse().map_err(|_| {
                    WErr::invalid_args(format!("bad -U value '{}'", tokens[i]), "-U <number>")
                })?;
                obj.insert("context".into(), Value::Number((n as i64).into()));
                i += 1;
                continue;
            }
            if let Some(nstr) = tok.strip_prefix("-U") {
                if !nstr.is_empty() && nstr.chars().all(|c| c.is_ascii_digit()) {
                    let n: u32 = nstr.parse().map_err(|_| {
                        WErr::invalid_args(format!("bad -U value '{nstr}'"), "-U<number>")
                    })?;
                    obj.insert("context".into(), Value::Number((n as i64).into()));
                    i += 1;
                    continue;
                }
            }
            if tok == "--unified" {
                i += 1;
                if i >= tokens.len() {
                    return Err(WErr::invalid_args(
                        "--unified requires a number",
                        "w git diff --unified <n>",
                    ));
                }
                let n: u32 = tokens[i].parse().map_err(|_| {
                    WErr::invalid_args(
                        format!("bad --unified value '{}'", tokens[i]),
                        "--unified <number>",
                    )
                })?;
                obj.insert("context".into(), Value::Number((n as i64).into()));
                i += 1;
                continue;
            }
            if let Some(nstr) = tok.strip_prefix("--unified=") {
                if !nstr.is_empty() {
                    let n: u32 = nstr.parse().map_err(|_| {
                        WErr::invalid_args(
                            format!("bad --unified value '{nstr}'"),
                            "--unified=<number>",
                        )
                    })?;
                    obj.insert("context".into(), Value::Number((n as i64).into()));
                    i += 1;
                    continue;
                }
            }
        }
        if tool == "search" && (tok == "-e" || tok == "--regexp") {
            i += 1;
            if i >= tokens.len() {
                return Err(WErr::invalid_args(
                    format!("{tok} requires a pattern"),
                    "w search -e <pattern> [path]",
                ));
            }
            obj.entry("pattern".to_string())
                .or_insert_with(|| Value::String(tokens[i].clone()));
            i += 1;
            continue;
        }
        if tool == "git"
            && matches!(
                tok.as_str(),
                "--oneline" | "--no-pager" | "--graph" | "--decorate"
            )
        {
            i += 1;
            continue;
        }
        if tok.starts_with("--no-") {
            let key = kebab_to_snake(tok.trim_start_matches("--no-"));
            if matches!(tool, "run" | "sh") {
                positionals.push(tok.clone());
                i += 1;
                continue;
            }
            insert_value(&mut obj, &key, Value::Bool(false));
            i += 1;
            continue;
        }
        if let Some(flag) = tok.strip_prefix("--") {
            let (raw_key, inline) = match flag.split_once('=') {
                Some((k, v)) => (k, Some(v.to_string())),
                None => (flag, None),
            };
            if tool == "search" && raw_key == "regexp" {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        if i >= tokens.len() {
                            return Err(WErr::invalid_args(
                                "--regexp requires a pattern",
                                "w search --regexp <pattern> [path]",
                            ));
                        }
                        tokens[i].clone()
                    }
                };
                obj.entry("pattern".to_string())
                    .or_insert_with(|| Value::String(v));
                i += 1;
                continue;
            }
            let key = kebab_to_snake(raw_key);
            let run_control = matches!(tool, "run")
                && matches!(key.as_str(), "cwd" | "timeout_ms" | "stdin" | "env");
            let sh_control =
                matches!(tool, "sh") && matches!(key.as_str(), "cwd" | "timeout_ms" | "stdin");
            if matches!(tool, "run" | "sh") && !run_control && !sh_control {
                positionals.push(tok.clone());
                i += 1;
                continue;
            }
            if key == "env" {
                let v = match inline {
                    Some(v) => v,
                    None => {
                        i += 1;
                        if i >= tokens.len() {
                            return Err(WErr::invalid_args(
                                "--env requires KEY=VALUE",
                                "--env KEY=VALUE",
                            ));
                        }
                        tokens[i].clone()
                    }
                };
                let (k, val) = v.split_once('=').ok_or_else(|| {
                    WErr::invalid_args(format!("bad --env '{v}'"), "--env KEY=VALUE")
                })?;
                let env = obj
                    .entry("env".to_string())
                    .or_insert_with(|| Value::Object(Map::new()));
                if let Some(map) = env.as_object_mut() {
                    map.insert(k.to_string(), Value::String(val.to_string()));
                }
                i += 1;
                continue;
            }
            let is_bool = BOOLEANS.contains(&raw_key);
            let value = match inline {
                Some(v) => parse_scalar_for(&key, &v),
                None if is_bool => {
                    if i + 1 < tokens.len() && (tokens[i + 1] == "true" || tokens[i + 1] == "false")
                    {
                        i += 1;
                        Value::Bool(tokens[i] == "true")
                    } else {
                        Value::Bool(true)
                    }
                }
                None => {
                    i += 1;
                    if i >= tokens.len() {
                        return Err(WErr::invalid_args(
                            format!("--{raw_key} requires a value"),
                            format!("--{raw_key} <value>"),
                        ));
                    }
                    parse_scalar_for(&key, &tokens[i])
                }
            };
            insert_value(&mut obj, &key, value);
            i += 1;
            continue;
        }
        if !matches!(tool, "run" | "sh") {
            if tok.starts_with('-') && !tok.starts_with("--") && tok.len() > 1 {
                return Err(WErr::invalid_args(
                    format!("{tool}: unknown option '{tok}'"),
                    "use positional args or --flag=value; run `w <tool> --help`",
                ));
            }
            if let Some((raw_key, raw_value)) = tok.split_once('=') {
                let valid = !raw_key.is_empty()
                    && raw_key
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-');
                let key = kebab_to_snake(raw_key);
                if valid && !raw_key.starts_with('-') && is_known_arg_key(&key) {
                    insert_value(&mut obj, &key, parse_scalar_for(&key, raw_value));
                    i += 1;
                    continue;
                }
            }
        }
        positionals.push(tok.clone());
        i += 1;
    }
    Ok((obj, positionals, after_dd))
}

fn parse_count(s: &str, flag: &str) -> Result<u32> {
    let n: u32 = s.parse().map_err(|_| {
        WErr::invalid_args(format!("bad {flag} value '{s}'"), format!("{flag} <count>"))
    })?;
    if n == 0 {
        return Err(WErr::invalid_args(
            format!("{flag} must be at least 1"),
            format!("{flag} <count>"),
        ));
    }
    Ok(n)
}

fn is_known_arg_key(key: &str) -> bool {
    matches!(
        key,
        "all"
            | "allow_empty"
            | "argv"
            | "branch"
            | "budget"
            | "cached"
            | "calls"
            | "calls_file"
            | "content"
            | "context"
            | "create"
            | "cwd"
            | "delete"
            | "depth"
            | "dry_run"
            | "end"
            | "env"
            | "expect_hash"
            | "find"
            | "force"
            | "format"
            | "glob"
            | "head"
            | "kind"
            | "limit"
            | "lines"
            | "max_files"
            | "message"
            | "mode"
            | "name_only"
            | "name_status"
            | "numstat"
            | "offset"
            | "op"
            | "outline"
            | "overwrite"
            | "path"
            | "paths"
            | "pathspec"
            | "pattern"
            | "ref"
            | "reference"
            | "regex"
            | "remote"
            | "replace"
            | "set_upstream"
            | "shortstat"
            | "sizes"
            | "stage"
            | "auto_stage"
            | "stop_on_error"
            | "fail_fast"
            | "staged"
            | "start"
            | "stat"
            | "stdin"
            | "symbol"
            | "tail"
            | "timeout_ms"
            | "verbosity"
            | "word"
    )
}

fn parse_scalar_for(key: &str, s: &str) -> Value {
    let numeric = matches!(
        key,
        "depth"
            | "limit"
            | "context"
            | "offset"
            | "head"
            | "tail"
            | "timeout_ms"
            | "budget"
            | "start"
            | "end"
            | "max_files"
    );
    let structured = matches!(key, "paths" | "pathspec" | "calls" | "argv");
    if numeric {
        if let Ok(n) = s.parse::<i64>() {
            return Value::Number(n.into());
        }
    }
    if structured || s.starts_with('[') || s.starts_with('{') {
        if let Ok(v) = serde_json::from_str::<Value>(s) {
            return v;
        }
    }
    if matches!(
        key,
        "all"
            | "stage"
            | "auto_stage"
            | "stop_on_error"
            | "fail_fast"
            | "staged"
            | "force"
            | "outline"
            | "sizes"
            | "delete"
            | "create"
            | "overwrite"
            | "set_upstream"
            | "allow_empty"
    ) {
        if s == "true" {
            return Value::Bool(true);
        }
        if s == "false" {
            return Value::Bool(false);
        }
    }
    Value::String(s.to_string())
}

fn insert_value(obj: &mut Map<String, Value>, key: &str, value: Value) {
    match obj.get_mut(key) {
        None => {
            obj.insert(key.to_string(), value);
        }
        Some(Value::Array(arr)) => arr.push(value),
        Some(existing) => {
            let old = existing.clone();
            *existing = Value::Array(vec![old, value]);
        }
    }
}

fn split_cmd_tokens(cmd: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut cur = String::new();
    let mut in_quote = None;
    for c in cmd.chars() {
        match (c, in_quote) {
            ('"' | '\'', None) => in_quote = Some(c),
            (q, Some(cur_q)) if q == cur_q => in_quote = None,
            (' ' | '\t', None) => {
                if !cur.is_empty() {
                    tokens.push(std::mem::take(&mut cur));
                }
            }
            (c, _) => cur.push(c),
        }
    }
    if !cur.is_empty() {
        tokens.push(cur);
    }
    tokens
}

fn split_file_line_suffix(p: &str) -> Option<(String, String)> {
    let parts: Vec<&str> = p.rsplitn(3, ':').collect();
    if parts.len() == 3
        && !parts[0].is_empty()
        && !parts[1].is_empty()
        && parts[0].chars().all(|c| c.is_ascii_digit())
        && parts[1].chars().all(|c| c.is_ascii_digit())
    {
        let start = parts[1];
        let end = parts[0];
        let file = parts[2];
        if !file.is_empty() {
            return Some((file.to_string(), format!("{start}:{end}")));
        }
    }
    if parts.len() >= 2 && !parts[0].is_empty() && parts[0].chars().all(|c| c.is_ascii_digit()) {
        let line = parts[0];
        let file = &p[..p.len() - line.len() - 1];
        if !file.is_empty() {
            return Some((file.to_string(), format!("{line}:{line}")));
        }
    }
    None
}

fn fill_positionals(tool: &str, obj: &mut Map<String, Value>, pos: &[String]) -> Result<()> {
    let mut put = |key: &str, val: &str| {
        if !obj.contains_key(key) {
            obj.insert(key.to_string(), Value::String(val.to_string()));
        }
    };
    match tool {
        "tree" => {
            if let Some(p) = pos.first() {
                put("path", p);
            }
        }
        "read" => {
            let mut file_paths = Vec::new();
            for p in pos {
                if p.contains(':') && p.chars().all(|c| c.is_ascii_digit() || c == ':') {
                    put("lines", p);
                } else if !p.starts_with('-') {
                    if let Some((file, lines_str)) = split_file_line_suffix(p) {
                        put("lines", &lines_str);
                        file_paths.push(file);
                    } else {
                        file_paths.push(p.clone());
                    }
                }
            }
            if file_paths.len() == 1 {
                put("path", &file_paths[0]);
            } else if file_paths.len() > 1 {
                obj.insert(
                    "paths".into(),
                    Value::Array(file_paths.into_iter().map(Value::String).collect()),
                );
            }
        }
        "blame" => {
            if let Some(p) = pos.first() {
                put("path", p);
            }
        }
        "edit" => {
            if let Some(p) = pos.first() {
                if let Some((file, lines_str)) = split_file_line_suffix(p) {
                    put("path", &file);
                    put("lines", &lines_str);
                    if let Some(r) = pos.get(1) {
                        put("replace", r);
                    }
                } else {
                    put("path", p);
                    if let Some(arg1) = pos.get(1) {
                        let is_line_spec = (arg1.contains(':')
                            && arg1.chars().all(|c| c.is_ascii_digit() || c == ':'))
                            || (!arg1.is_empty() && arg1.chars().all(|c| c.is_ascii_digit()));
                        if is_line_spec {
                            put("lines", arg1);
                            if let Some(r) = pos.get(2) {
                                put("replace", r);
                            }
                        } else {
                            put("find", arg1);
                            if let Some(r) = pos.get(2) {
                                put("replace", r);
                            }
                        }
                    }
                }
            }
        }
        "search" => {
            if let Some(p) = pos.first() {
                put("pattern", p);
            }
            if let Some(p) = pos.get(1) {
                put("path", p);
            }
        }
        "replace" => {
            if let Some(p) = pos.first() {
                put("find", p);
            }
            if let Some(p) = pos.get(1) {
                put("replace", p);
            }
            if let Some(p) = pos.get(2) {
                put("path", p);
            }
        }
        "expand" => {
            if let Some(p) = pos.first() {
                put("ref", p);
            }
        }
        "check" => {
            for p in pos {
                if matches!(p.as_str(), "all" | "build" | "lint" | "types" | "test") {
                    put("kind", p);
                } else {
                    put("path", p);
                }
            }
        }
        "batch" => {
            if !obj.contains_key("calls") {
                if let Some(first) = pos.first() {
                    let v = first.strip_prefix("calls=").unwrap_or(first);
                    if v.trim_start().starts_with('[') {
                        if pos.len() > 1 {
                            return Err(WErr::invalid_args(
                                format!("{tool}: unexpected positional '{}'", pos[1]),
                                format!("see `w {tool} --help`"),
                            ));
                        }
                        obj.insert("calls".into(), parse_scalar_for("calls", v));
                    } else {
                        let mut calls = Vec::new();
                        for p in pos {
                            for subcmd in split_cmd_chain(p) {
                                calls.push(parse_subcommand_to_call(&subcmd)?);
                            }
                        }
                        obj.insert("calls".into(), Value::Array(calls));
                    }
                }
            }
        }
        _ => {
            if !pos.is_empty() {
                return Err(WErr::invalid_args(
                    format!("{tool}: unexpected positional '{}'", pos[0]),
                    format!("see `w {tool} --help`"),
                ));
            }
        }
    }
    Ok(())
}

fn build_git_args(obj: &mut Map<String, Value>, pos: &[String]) -> Result<()> {
    let Some(op) = pos
        .first()
        .cloned()
        .or_else(|| obj.get("op").and_then(Value::as_str).map(str::to_string))
    else {
        return Err(WErr::invalid_args(
            "git requires an op",
            "git status|diff|log|show|blame|branch|commit|push",
        ));
    };
    obj.insert("op".into(), Value::String(op.clone()));
    let rest = &pos[1.min(pos.len())..];
    match op.as_str() {
        "commit" => {
            let (msg_side, path_side) = if let Some(idx) = rest.iter().position(|t| t == "--") {
                (&rest[..idx], &rest[idx + 1..])
            } else {
                (rest, &[] as &[String])
            };
            if !obj.contains_key("message") && !msg_side.is_empty() {
                obj.insert("message".into(), Value::String(msg_side[0].clone()));
                let mut ps = msg_side[1..].to_vec();
                ps.extend(path_side.iter().cloned());
                if !ps.is_empty() && !obj.contains_key("pathspec") {
                    obj.insert(
                        "pathspec".into(),
                        Value::Array(ps.into_iter().map(Value::String).collect()),
                    );
                }
            } else if obj.contains_key("message") {
                let mut ps = msg_side.to_vec();
                ps.extend(path_side.iter().cloned());
                if !ps.is_empty() && !obj.contains_key("pathspec") {
                    obj.insert(
                        "pathspec".into(),
                        Value::Array(ps.into_iter().map(Value::String).collect()),
                    );
                }
            } else if !path_side.is_empty() && !obj.contains_key("pathspec") {
                obj.insert(
                    "pathspec".into(),
                    Value::Array(path_side.iter().cloned().map(Value::String).collect()),
                );
            }
        }
        "diff" => {
            if let Some(idx) = rest.iter().position(|t| t == "--") {
                let left = &rest[..idx];
                let right = &rest[idx + 1..];
                if left.len() > 1 {
                    return Err(WErr::invalid_args(
                        "git diff: too many refs before --",
                        "git diff [ref] [-- paths...]",
                    ));
                }
                if let Some(r) = left.first() {
                    if !obj.contains_key("ref") {
                        obj.insert("ref".into(), Value::String(r.clone()));
                    }
                }
                if !right.is_empty() && !obj.contains_key("pathspec") {
                    obj.insert(
                        "pathspec".into(),
                        Value::Array(right.iter().cloned().map(Value::String).collect()),
                    );
                }
            } else if !rest.is_empty() && !obj.contains_key("pathspec") && !obj.contains_key("ref")
            {
                if rest.iter().any(|t| t.starts_with('-')) {
                    let paths: Vec<String> = rest
                        .iter()
                        .filter(|t| !t.starts_with('-'))
                        .cloned()
                        .collect();
                    if !paths.is_empty() {
                        obj.insert(
                            "pathspec".into(),
                            Value::Array(paths.into_iter().map(Value::String).collect()),
                        );
                    }
                } else if rest.len() == 1 {
                    obj.insert("ref".into(), Value::String(rest[0].clone()));
                } else {
                    obj.insert("ref".into(), Value::String(rest[0].clone()));
                    obj.insert(
                        "pathspec".into(),
                        Value::Array(rest[1..].iter().cloned().map(Value::String).collect()),
                    );
                }
            }
        }
        "show" => {
            if let Some(idx) = rest.iter().position(|t| t == "--") {
                let left = &rest[..idx];
                let right = &rest[idx + 1..];
                if let Some(r) = left.first() {
                    if !obj.contains_key("ref") {
                        obj.insert("ref".into(), Value::String(r.clone()));
                    }
                }
                if !right.is_empty() && !obj.contains_key("pathspec") {
                    obj.insert(
                        "pathspec".into(),
                        Value::Array(right.iter().cloned().map(Value::String).collect()),
                    );
                }
            } else {
                if let Some(r) = rest.first() {
                    if !obj.contains_key("ref") {
                        obj.insert("ref".into(), Value::String(r.clone()));
                    }
                }
                if rest.len() > 1 && !obj.contains_key("pathspec") {
                    obj.insert(
                        "pathspec".into(),
                        Value::Array(rest[1..].iter().cloned().map(Value::String).collect()),
                    );
                }
            }
            if !obj.contains_key("ref") {
                obj.insert("ref".into(), Value::String("HEAD".into()));
            }
        }
        "log" => {
            let mut ref_seen = obj.contains_key("ref");
            let mut it = rest.iter();
            while let Some(tok) = it.next() {
                if tok == "-n" {
                    let Some(v) = it.next() else {
                        return Err(WErr::invalid_args("-n requires a value", "-n <count>"));
                    };
                    obj.insert("limit".into(), Value::Number(parse_count(v, "-n")?.into()));
                } else if let Some(raw) = tok.strip_prefix("-n") {
                    obj.insert(
                        "limit".into(),
                        Value::Number(parse_count(raw, "-n")?.into()),
                    );
                } else if matches!(
                    tok.as_str(),
                    "--oneline" | "--no-pager" | "--graph" | "--decorate"
                ) {
                    continue;
                } else if tok.starts_with('-') {
                    return Err(WErr::invalid_args(
                        format!("git log: unsupported flag '{tok}'"),
                        "supported: -n <count>, --limit <count>",
                    ));
                } else if !obj.contains_key("limit") && tok.chars().all(|c| c.is_ascii_digit()) {
                    let n = parse_count(tok, "count")?;
                    obj.insert("limit".into(), Value::Number((n as i64).into()));
                } else if !ref_seen {
                    obj.insert("ref".into(), Value::String(tok.clone()));
                    ref_seen = true;
                } else {
                    return Err(WErr::invalid_args(
                        format!("git log: unexpected positional '{tok}'"),
                        "git log [-n <count>] [ref]",
                    ));
                }
            }
        }
        "blame" => {
            if let Some(p) = rest.first() {
                if !obj.contains_key("path") {
                    obj.insert("path".into(), Value::String(p.clone()));
                }
            }
        }
        "push" => {
            if let Some(r) = rest.first() {
                if !obj.contains_key("remote") {
                    obj.insert("remote".into(), Value::String(r.clone()));
                }
            }
            if let Some(b) = rest.get(1) {
                if !obj.contains_key("branch") {
                    obj.insert("branch".into(), Value::String(b.clone()));
                }
            }
        }
        _ => {}
    }
    Ok(())
}

fn resolve_tool_args(tool: &str, rest: &[String]) -> Result<Map<String, Value>> {
    let (mut obj, positionals, after_dd) = parse_tool_args(rest, tool)?;
    let merged_pos = if matches!(tool, "run" | "sh") {
        positionals.clone()
    } else {
        let mut p = positionals.clone();
        p.extend(after_dd.clone());
        p
    };
    if tool == "run" || tool == "sh" {
        if tool == "run" {
            let mut argv = positionals.clone();
            if argv.is_empty() {
                argv = after_dd.clone();
            } else if !after_dd.is_empty() {
                argv.extend(after_dd.clone());
            }
            if !argv.is_empty() {
                obj.insert(
                    "argv".into(),
                    Value::Array(argv.into_iter().map(Value::String).collect()),
                );
            }
        } else {
            let cmd = if !positionals.is_empty() {
                positionals.join(" ")
            } else {
                after_dd.join(" ")
            };
            if !cmd.is_empty() {
                obj.insert("command".into(), Value::String(cmd));
            }
        }
    } else if tool == "git" {
        build_git_args(&mut obj, &merged_pos)?;
    } else if tool == "search" {
        if obj.contains_key("pattern") {
            if merged_pos.len() == 1 {
                obj.insert("path".into(), Value::String(merged_pos[0].clone()));
            } else if merged_pos.len() > 1 {
                let paths: Vec<Value> = merged_pos.iter().cloned().map(Value::String).collect();
                obj.insert("paths".into(), Value::Array(paths));
            }
        } else if !merged_pos.is_empty() {
            obj.insert("pattern".into(), Value::String(merged_pos[0].clone()));
            if merged_pos.len() == 2 {
                obj.insert("path".into(), Value::String(merged_pos[1].clone()));
            } else if merged_pos.len() > 2 {
                let paths: Vec<Value> =
                    merged_pos[1..].iter().cloned().map(Value::String).collect();
                obj.insert("paths".into(), Value::Array(paths));
            }
        }
    } else {
        fill_positionals(tool, &mut obj, &merged_pos)?;
    }
    Ok(obj)
}

fn split_cmd_chain(input: &str) -> Vec<String> {
    let mut parts = Vec::new();
    let mut cur = String::new();
    let mut in_quote = None;
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        match (c, in_quote) {
            ('"' | '\'', None) => {
                in_quote = Some(c);
                cur.push(c);
            }
            (q, Some(cur_q)) if q == cur_q => {
                in_quote = None;
                cur.push(c);
            }
            (';', None) => {
                let trimmed = cur.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
                cur.clear();
            }
            ('&', None) if chars.peek() == Some(&'&') => {
                chars.next();
                let trimmed = cur.trim();
                if !trimmed.is_empty() {
                    parts.push(trimmed.to_string());
                }
                cur.clear();
            }
            (c, _) => cur.push(c),
        }
    }
    let trimmed = cur.trim();
    if !trimmed.is_empty() {
        parts.push(trimmed.to_string());
    }
    parts
}

fn parse_subcommand_to_call(cmd_str: &str) -> Result<Value> {
    let mut raw_tokens = split_cmd_tokens(cmd_str);
    if raw_tokens.is_empty() {
        return Err(WErr::invalid_args(
            "empty command in batch",
            "w batch \"<tool> [args]\" ...",
        ));
    }
    if raw_tokens[0] == "w" || raw_tokens[0] == "wxec" {
        raw_tokens.remove(0);
        if raw_tokens.is_empty() {
            return Err(WErr::invalid_args(
                "empty command in batch",
                "w batch \"<tool> [args]\" ...",
            ));
        }
    }
    let sub_tool = canonical_tool(&raw_tokens[0]);
    let sub_rest = &raw_tokens[1..];
    let sub_obj = resolve_tool_args(&sub_tool, sub_rest)?;
    Ok(serde_json::json!({
        "tool": sub_tool,
        "args": Value::Object(sub_obj)
    }))
}

// ---------------------------------------------------------------------------
// Utility commands
// ---------------------------------------------------------------------------

fn parse_since(tokens: &[String]) -> Option<i64> {
    let idx = tokens.iter().position(|t| t == "--since")?;
    let spec = tokens.get(idx + 1)?;
    since_ms(spec)
}

fn since_ms(spec: &str) -> Option<i64> {
    let now = wxec_core::types::now_ms();
    let s = spec.trim();
    if let Some(days) = s.strip_suffix('d').and_then(|n| n.parse::<i64>().ok()) {
        return days
            .checked_mul(86_400_000)
            .and_then(|ms| now.checked_sub(ms));
    }
    if let Some(hours) = s.strip_suffix('h').and_then(|n| n.parse::<i64>().ok()) {
        return hours
            .checked_mul(3_600_000)
            .and_then(|ms| now.checked_sub(ms));
    }
    if let Some(mins) = s.strip_suffix('m').and_then(|n| n.parse::<i64>().ok()) {
        return mins.checked_mul(60_000).and_then(|ms| now.checked_sub(ms));
    }
    if let Some(days) = days_from_ymd(s) {
        return days.checked_mul(86_400_000);
    }
    None
}

fn days_from_ymd(s: &str) -> Option<i64> {
    let mut parts = s.split('-');
    let y: i64 = parts.next()?.parse().ok()?;
    let m: i64 = parts.next()?.parse().ok()?;
    let d: i64 = parts.next()?.parse().ok()?;
    if !(1..=12).contains(&m) || !(1..=31).contains(&d) {
        return None;
    }
    let y_adj = if m <= 2 { y.checked_sub(1)? } else { y };
    let era = y_adj.div_euclid(400);
    let yoe = y_adj.checked_sub(era.checked_mul(400)?)?;
    let mp = if m > 2 {
        m.checked_sub(3)?
    } else {
        m.checked_add(9)?
    };
    let doy = 153_i64
        .checked_mul(mp)?
        .checked_add(2)?
        .checked_div(5)?
        .checked_add(d.checked_sub(1)?)?;
    let doe = yoe
        .checked_mul(365)?
        .checked_add(yoe.div_euclid(4))?
        .checked_sub(yoe.div_euclid(100))?
        .checked_add(doy)?;
    era.checked_mul(146_097)?
        .checked_add(doe)?
        .checked_sub(719_468)
}

fn cmd_gain(tokens: &[String]) -> Result<i32> {
    let json = tokens.iter().any(|t| t == "--json");
    let since = parse_since(tokens);
    let s = stats::gain(since);
    if json {
        println!("{}", serde_json::to_string(&s)?);
    } else {
        println!(
            "calls {}, out {} tokens, raw {} tokens, saved {} tokens",
            s.calls, s.out_tokens, s.raw_tokens, s.saved_tokens
        );
        if !s.by_tool.is_empty() {
            let tools: Vec<String> = s.by_tool.iter().map(|(k, v)| format!("{k} {v}")).collect();
            println!("by_tool {}", tools.join(", "));
        }
    }
    Ok(0)
}

fn cmd_discover(tokens: &[String]) -> Result<i32> {
    let json = tokens.iter().any(|t| t == "--json");
    let since = parse_since(tokens);
    let s = stats::discover(since);
    if json {
        println!("{}", serde_json::to_string(&s)?);
    } else {
        println!(
            "calls {}, sh {}, fallback {}",
            s.calls, s.sh_calls, s.fallback_calls
        );
        if !s.per_tool.is_empty() {
            let tools: Vec<String> = s.per_tool.iter().map(|(k, v)| format!("{k} {v}")).collect();
            println!("by_tool {}", tools.join(", "));
        }
    }
    Ok(0)
}

fn cmd_parser(tokens: &[String]) -> Result<i32> {
    if tokens.first().map(String::as_str) != Some("test") {
        return Err(WErr::invalid_args(
            "parser requires subcommand 'test'",
            "w parser test <parser.toml> <sample>",
        ));
    }
    let toml_path = tokens.get(1).ok_or_else(|| {
        WErr::invalid_args(
            "missing parser file",
            "w parser test <parser.toml> <sample>",
        )
    })?;
    let sample_path = tokens.get(2).ok_or_else(|| {
        WErr::invalid_args(
            "missing sample file",
            "w parser test <parser.toml> <sample>",
        )
    })?;
    let parser = wxec_parsers::TomlParser::load(Path::new(toml_path))?;
    let sample = std::fs::read_to_string(sample_path)
        .map_err(|e| WErr::new("not_found", format!("sample '{sample_path}': {e}")))?;
    let parsed = parser.parse(&sample, "");
    match parsed {
        Some(run) => {
            let out = json!({
                "parser": parser.name,
                "summary": run.summary,
                "tests": run.tests,
                "diagnostics": run.diagnostics,
            });
            println!("{}", serde_json::to_string_pretty(&out)?);
            Ok(0)
        }
        None => Err(WErr::with_hint(
            "parse_failed",
            format!("parser '{}' produced no records", parser.name),
            "check [map.test_result] JSON paths and [parse].format",
        )),
    }
}

fn cmd_handles(tokens: &[String], global: &Global) -> Result<i32> {
    if tokens.first().map(String::as_str) != Some("gc") {
        return Err(WErr::invalid_args(
            "handles requires subcommand 'gc'",
            "w handles gc",
        ));
    }
    let mut dispatcher = build_dispatcher(global, false)?;
    dispatcher.open_session(&global.session)?;
    let removed = dispatcher.gc_handles(&global.session)?;
    println!("handles gc: {removed} removed");
    Ok(0)
}

fn cmd_config(tokens: &[String], global: &Global) -> Result<i32> {
    match tokens.first().map(String::as_str) {
        Some("show") => {
            let (cfg, warnings) = config::load(
                &global.root,
                global.config.as_deref(),
                global.no_project_config,
            )?;
            for w in warnings {
                eprintln!("# {w}");
            }
            print!("{}", config::render(&cfg));
            Ok(0)
        }
        Some("trust") => {
            config::trust_root(&global.root)?;
            let root = global
                .root
                .canonicalize()
                .unwrap_or_else(|_| global.root.clone());
            println!("trusted {}", root.display());
            Ok(0)
        }
        _ => Err(WErr::invalid_args(
            "config requires subcommand",
            "w config show | w config trust",
        )),
    }
}

fn print_help() {
    println!(
        "w - token-efficient workspace engine for agents ({})",
        env!("CARGO_PKG_VERSION")
    );
    println!("usage: w <command> [args]");
    println!();
    println!("ultra-short commands:");
    println!("  w s <pat> [path]                search (alias: s)");
    println!("  w r <paths...> [lines]          read (supports file:1:40, multi-file)");
    println!("  w e <path> [find/lines] [repl]  edit (exact/fuzzy anchor or line range)");
    println!("  w p <find> <repl> [path]        replace across files");
    println!("  w u <command...>                run executable (e.g. w u cargo test -q)");
    println!("  w c [kind]                      check (all|build|test|lint)");
    println!("  w g <status|diff|commit|push>   git operations");
    println!("  w b \"<cmd 1>\" \"<cmd 2>\"         batch (supports ; and && chaining)");
    println!("  w x <id>                        expand handle (~id or bare id)");
}

// ---------------------------------------------------------------------------
// MCP entry
// ---------------------------------------------------------------------------

pub fn run_mcp(argv: Vec<String>) -> i32 {
    if argv.iter().any(|a| a == "--version" || a == "-V") {
        println!("w {}", env!("CARGO_PKG_VERSION"));
        return 0;
    }
    if argv.iter().any(|a| a == "--help" || a == "-h") {
        print_help();
        return 0;
    }
    let (global, tool, _rest) = match parse_global(&argv) {
        Ok(v) => v,
        Err(e) => {
            eprintln!("{}", e.render());
            return e.cli_exit();
        }
    };
    if tool.is_some() {
        return run(argv);
    }
    let tier = match global.tier.as_deref() {
        Some(t) => match t.parse::<wxec_core::types::Tier>() {
            Ok(tier) => Some(tier),
            Err(e) => {
                eprintln!("{}", e.render());
                return e.cli_exit();
            }
        },
        None => Some(wxec_core::types::Tier::GitWrite),
    };
    match wxec_adapters::serve_stdio(
        &global.root,
        global.config.as_deref(),
        global.no_project_config,
        tier,
    ) {
        Ok(()) => 0,
        Err(e) => {
            eprintln!("{}", e.render());
            e.cli_exit()
        }
    }
}

pub fn util_map(pairs: &[(&str, i64)]) -> BTreeMap<String, i64> {
    config::counts_map(pairs)
}

#[cfg(test)]
mod tests {
    use super::build_git_args;
    use serde_json::{Map, Value};

    fn log_args(rest: &[&str]) -> Map<String, Value> {
        let mut obj = Map::new();
        let mut pos = vec!["log".to_string()];
        pos.extend(rest.iter().map(|s| s.to_string()));
        build_git_args(&mut obj, &pos).unwrap();
        obj
    }

    #[test]
    fn git_log_short_count_is_parsed() {
        assert_eq!(log_args(&["-n", "50"])["limit"], Value::from(50));
    }

    #[test]
    fn git_log_attached_count_and_ref_are_parsed() {
        let a = log_args(&["-n50", "main"]);
        assert_eq!(a["limit"], Value::from(50));
        assert_eq!(a["ref"], Value::from("main"));
    }

    #[test]
    fn git_log_display_flags_are_ignored() {
        let a = log_args(&["--oneline", "-n", "5"]);
        assert_eq!(a["limit"], Value::from(5));
    }

    #[test]
    fn git_log_missing_count_errors() {
        let mut obj = Map::new();
        let pos = vec!["log".to_string(), "-n".to_string()];
        assert!(build_git_args(&mut obj, &pos).is_err());
    }

    #[test]
    fn git_log_unknown_short_flag_errors() {
        let mut obj = Map::new();
        let pos = vec!["log".to_string(), "-p".to_string()];
        assert!(build_git_args(&mut obj, &pos).is_err());
    }

    #[test]
    fn git_log_bare_number_count_is_parsed() {
        assert_eq!(log_args(&["5"])["limit"], Value::from(5));
        assert_eq!(log_args(&["10", "main"])["limit"], Value::from(10));
        assert_eq!(log_args(&["10", "main"])["ref"], Value::from("main"));
    }

    #[test]
    fn git_log_negative_number_flag_is_parsed() {
        let (obj, _, _) =
            super::parse_tool_args(&["log".to_string(), "-5".to_string()], "git").unwrap();
        assert_eq!(obj.get("limit").and_then(Value::as_i64), Some(5));
    }

    #[test]
    fn git_show_defaults_to_head() {
        let mut obj = Map::new();
        let pos = vec!["show".to_string()];
        build_git_args(&mut obj, &pos).unwrap();
        assert_eq!(obj["ref"], Value::from("HEAD"));
    }

    #[test]
    fn search_dash_e_sets_pattern() {
        let (obj, pos, _) = super::parse_tool_args(
            &["-e".to_string(), "foo|bar".to_string(), "src".to_string()],
            "search",
        )
        .unwrap();
        assert_eq!(obj.get("pattern").and_then(Value::as_str), Some("foo|bar"));
        assert_eq!(pos, vec!["src".to_string()]);
    }

    #[test]
    fn search_regexp_flag_sets_pattern() {
        let (obj, pos, _) =
            super::parse_tool_args(&["--regexp".to_string(), "foo".to_string()], "search").unwrap();
        assert_eq!(obj.get("pattern").and_then(Value::as_str), Some("foo"));
        assert!(pos.is_empty());
    }

    #[test]
    fn unknown_short_flag_errors_for_search() {
        let err =
            super::parse_tool_args(&["-x".to_string(), "foo".to_string()], "search").unwrap_err();
        assert!(
            err.to_string().contains("unknown option") || err.render().contains("unknown option")
        );
    }

    #[test]
    fn short_flags_allowed_for_run_argv() {
        let (_, pos, _) = super::parse_tool_args(
            &["cargo".to_string(), "test".to_string(), "-q".to_string()],
            "run",
        )
        .unwrap();
        assert_eq!(pos, vec!["cargo", "test", "-q"]);
    }

    #[test]
    fn batch_strips_w_prefix() {
        let call = super::parse_subcommand_to_call("w r file.rs 1:5").unwrap();
        assert_eq!(call["tool"], "read");
        assert_eq!(call["args"]["path"], "file.rs");
        assert_eq!(call["args"]["lines"], "1:5");
    }

    #[test]
    fn batch_chained_delimiters() {
        let parts = super::split_cmd_chain("r file1.rs; s pattern crates && u cargo check");
        assert_eq!(
            parts,
            vec!["r file1.rs", "s pattern crates", "u cargo check"]
        );
    }

    #[test]
    fn edit_positionals_support_line_range() {
        let mut obj = Map::new();
        super::fill_positionals(
            "edit",
            &mut obj,
            &[
                "src/main.rs".to_string(),
                "10:20".to_string(),
                "new content".to_string(),
            ],
        )
        .unwrap();
        assert_eq!(obj["path"], "src/main.rs");
        assert_eq!(obj["lines"], "10:20");
        assert_eq!(obj["replace"], "new content");
    }

    #[test]
    fn edit_positionals_support_file_line_suffix() {
        let mut obj = Map::new();
        super::fill_positionals(
            "edit",
            &mut obj,
            &["src/main.rs:15:30".to_string(), "new content".to_string()],
        )
        .unwrap();
        assert_eq!(obj["path"], "src/main.rs");
        assert_eq!(obj["lines"], "15:30");
        assert_eq!(obj["replace"], "new content");
    }

    #[test]
    fn check_positionals_support_kind_and_path() {
        let mut obj = Map::new();
        super::fill_positionals("check", &mut obj, &["test".to_string()]).unwrap();
        assert_eq!(obj["kind"], "test");

        let mut obj2 = Map::new();
        super::fill_positionals(
            "check",
            &mut obj2,
            &["lint".to_string(), "crates".to_string()],
        )
        .unwrap();
        assert_eq!(obj2["kind"], "lint");
        assert_eq!(obj2["path"], "crates");
    }
}
