use serde::Serialize;

use crate::error::{Result, WErr};
use crate::estimator;
use crate::handles::HandleStore;
use crate::redact::Redactor;
use crate::types::{Format, ItemClass, Meta, ToolResult};

pub struct RenderOptions<'a> {
    pub budget: u32,
    pub format: Format,
    pub handles: &'a mut HandleStore,
    pub redactor: &'a Redactor,
    pub passthrough_tokens: Option<u32>,
    pub min_gain_pct: Option<u32>,
    pub verbosity: Option<String>,
}

#[derive(Debug, Clone)]
pub struct Rendered {
    pub text: String,
    pub ok: bool,
    pub cli_code: i32,
    pub is_error: bool,
    pub out_tokens: u32,
    pub redactions: u32,
}

impl Rendered {
    pub fn error(err: &WErr, format: Format) -> Self {
        let text = match format {
            Format::Lean => format!("{}\n", err.render()),
            Format::Json => render_error_json(err),
        };
        let out_tokens = estimator::estimate_text(&text);
        Self {
            text,
            ok: false,
            cli_code: err.cli_exit(),
            is_error: true,
            out_tokens,
            redactions: 0,
        }
    }
}

/// Lean wire-format rendering with value-ranked budget fitting (spec §6, §7.1).
pub fn render_lean(result: &ToolResult, opts: RenderOptions<'_>) -> Result<Rendered> {
    let mut redactions = 0u32;
    let (header, n) = opts.redactor.redact(&result.header);
    redactions += n;

    let must_tokens: u32 = result
        .items
        .iter()
        .filter(|i| i.class == ItemClass::MustKeep)
        .map(|i| i.est_tokens)
        .sum();
    let header_tokens = estimator::estimate_text(&header);
    let budget_exceeded = must_tokens + header_tokens > opts.budget;

    let mut remaining = opts
        .budget
        .saturating_sub(header_tokens)
        .saturating_sub(must_tokens);
    let mut included = vec![false; result.items.len()];

    // Greedy fill by value per token, stable by original order.
    let mut order: Vec<usize> = result
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.class != ItemClass::MustKeep)
        .map(|(idx, _)| idx)
        .collect();
    order.sort_by(|&a, &b| {
        let ia = &result.items[a];
        let ib = &result.items[b];
        let ra = ia.value / (ia.est_tokens.max(1) as f32);
        let rb = ib.value / (ib.est_tokens.max(1) as f32);
        rb.partial_cmp(&ra)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    for idx in order {
        let item = &result.items[idx];
        if item.est_tokens <= remaining {
            remaining -= item.est_tokens;
            included[idx] = true;
        }
    }

    let mut out: Vec<String> = Vec::new();
    out.push(header);
    for note in &result.meta.notes {
        let (line, n) = opts.redactor.redact(&format!("# {note}"));
        redactions += n;
        out.push(line);
    }
    if budget_exceeded {
        out.push("# budget_exceeded_must_keep".to_string());
    }

    let mut dropped_lines_count = 0usize;
    let mut gaps: Vec<(usize, usize)> = Vec::new(); // (index in out, count)

    let mut body_from_items = false;
    for (idx, item) in result.items.iter().enumerate() {
        if included[idx] || item.class == ItemClass::MustKeep {
            if dropped_lines_count > 0 {
                gaps.push((out.len(), dropped_lines_count));
                out.push(String::new());
                dropped_lines_count = 0;
            }
            for line in &item.lines {
                let (red, n) = opts.redactor.redact(line);
                redactions += n;
                out.push(red);
            }
            body_from_items = true;
        } else {
            dropped_lines_count += item.lines.len();
        }
    }
    if dropped_lines_count > 0 {
        gaps.push((out.len(), dropped_lines_count));
        out.push(String::new());
    }

    if !gaps.is_empty() {
        let mut all_dropped: Vec<&str> = Vec::new();
        for (idx, item) in result.items.iter().enumerate() {
            if !(included[idx] || item.class == ItemClass::MustKeep) {
                for l in &item.lines {
                    all_dropped.push(l.as_str());
                }
            }
        }
        let joined = all_dropped.join("\n");
        let href = opts.handles.put("elide", joined.as_bytes(), "text")?;
        for (i, n) in gaps {
            out[i] = format!("...+{n} lines ({})", href.id);
        }
    }

    let build_text = |out: &Vec<String>| -> String {
        let mut text = out.join("\n");
        text.push('\n');
        text
    };

    // Passthrough/min-gain: for small unstructured outputs (or fully emitted small bodies),
    // prefer raw body when structured formatting is not a win. Never suppress diagnostics/errors.
    let unstructured = result.diagnostics.is_empty()
        && !result
            .meta
            .counts
            .keys()
            .any(|k| !matches!(k.as_str(), "raw_bytes" | "duration_ms"))
        && !result.meta.notes.iter().any(|n| n.contains("parse_miss"));
    let no_omissions = result
        .items
        .iter()
        .enumerate()
        .all(|(idx, item)| included[idx] || item.class == ItemClass::MustKeep);
    let has_must_body = result.items.iter().any(|i| i.class == ItemClass::MustKeep);
    if unstructured && no_omissions {
        if let Some(raw) = result.raw_text.as_deref() {
            let raw_trim = raw.trim_end_matches(['\n', '\r']);
            let raw_body_t = estimator::estimate_text(raw_trim);
            let structured_text = build_text(&out);
            let structured_t = estimator::estimate_text(&structured_text);
            let mut pfx = vec![out[0].clone()];
            pfx.extend(
                out.iter()
                    .skip(1)
                    .take_while(|l| l.starts_with('#'))
                    .cloned(),
            );
            let (rraw, rn) = opts.redactor.redact(raw_trim);
            let mut ptext = pfx.join("\n");
            ptext.push('\n');
            if !rraw.is_empty() {
                ptext.push_str(&rraw);
                ptext.push('\n');
            }
            let pass_t = estimator::estimate_text(&ptext);
            let small_enough = opts
                .passthrough_tokens
                .map(|pt| raw_body_t <= pt)
                .unwrap_or(false);
            let insufficient_gain = opts
                .min_gain_pct
                .map(|pct| {
                    if raw_body_t == 0 {
                        return false;
                    }
                    let saved = structured_t.saturating_sub(pass_t);
                    saved * 100 < raw_body_t.saturating_mul(pct)
                })
                .unwrap_or(false);
            let minimal_tool_success = result.meta.ok
                && matches!(opts.verbosity.as_deref(), Some("minimal"))
                && matches!(result.tool, "run" | "sh" | "check");
            let can_passthrough = !minimal_tool_success
                && !has_must_body
                && (small_enough || insufficient_gain)
                && (result.handles.is_empty() || !body_from_items || pass_t <= structured_t);
            if can_passthrough && (small_enough || pass_t <= structured_t) {
                redactions += rn;
                return Ok(Rendered {
                    out_tokens: pass_t,
                    text: ptext,
                    ok: result.meta.ok,
                    cli_code: cli_code_for(result),
                    is_error: false,
                    redactions,
                });
            }
            // fallback: if nothing structured emitted, include raw body
            if !body_from_items && !minimal_tool_success {
                redactions += rn;
                if !rraw.is_empty() {
                    out.push(rraw);
                }
            }
        }
    }

    let text = build_text(&out);
    Ok(Rendered {
        out_tokens: estimator::estimate_text(&text),
        text,
        ok: result.meta.ok,
        cli_code: cli_code_for(result),
        is_error: false,
        redactions,
    })
}

#[derive(Serialize)]
struct JsonError<'a> {
    code: &'a str,
    message: &'a str,
    #[serde(skip_serializing_if = "Option::is_none")]
    hint: Option<&'a str>,
}

#[derive(Serialize)]
struct JsonOut<'a> {
    ok: bool,
    tool: &'a str,
    meta: Meta,
    data: JsonData,
    diagnostics: Vec<crate::types::Diagnostic>,
    handles: Vec<crate::types::HandleRef>,
    errors: Vec<JsonError<'a>>,
}

#[derive(Serialize)]
struct JsonData {
    header: String,
    lines: Vec<String>,
    elided_lines: usize,
}

/// JSON wire-format rendering (spec §6.3).
pub fn render_json(result: &ToolResult, opts: RenderOptions<'_>) -> Result<Rendered> {
    let mut redactions = 0u32;
    let (header, n) = opts.redactor.redact(&result.header);
    redactions += n;

    let must_tokens: u32 = result
        .items
        .iter()
        .filter(|i| i.class == ItemClass::MustKeep)
        .map(|i| i.est_tokens)
        .sum();
    let header_tokens = estimator::estimate_text(&header);
    let budget_exceeded = must_tokens + header_tokens > opts.budget;
    let mut remaining = opts
        .budget
        .saturating_sub(header_tokens)
        .saturating_sub(must_tokens);

    let mut order: Vec<usize> = result
        .items
        .iter()
        .enumerate()
        .filter(|(_, i)| i.class != ItemClass::MustKeep)
        .map(|(idx, _)| idx)
        .collect();
    order.sort_by(|&a, &b| {
        let ia = &result.items[a];
        let ib = &result.items[b];
        let ra = ia.value / (ia.est_tokens as f32).max(1.0);
        let rb = ib.value / (ib.est_tokens as f32).max(1.0);
        rb.partial_cmp(&ra)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then(a.cmp(&b))
    });
    let mut included = vec![false; result.items.len()];
    for (idx, item) in result.items.iter().enumerate() {
        if item.class == ItemClass::MustKeep {
            included[idx] = true;
        }
    }
    for idx in order {
        let item = &result.items[idx];
        if item.est_tokens <= remaining {
            remaining -= item.est_tokens;
            included[idx] = true;
        }
    }

    let mut lines: Vec<String> = Vec::new();
    if budget_exceeded {
        lines.push("# budget_exceeded_must_keep".to_string());
    }

    let mut dropped_lines: Vec<String> = Vec::new();
    let mut gaps: Vec<(usize, usize)> = Vec::new();
    for (idx, item) in result.items.iter().enumerate() {
        if included[idx] {
            if !dropped_lines.is_empty() {
                let n = dropped_lines.len();
                gaps.push((lines.len(), n));
                lines.push(String::new());
                dropped_lines.clear();
            }
            for line in &item.lines {
                let (red, n) = opts.redactor.redact(line);
                redactions += n;
                lines.push(red);
            }
        } else {
            dropped_lines.extend(item.lines.iter().cloned());
        }
    }
    if !dropped_lines.is_empty() {
        let n = dropped_lines.len();
        gaps.push((lines.len(), n));
        lines.push(String::new());
    }

    let mut handles = result.handles.clone();
    let mut elided = 0usize;
    if !gaps.is_empty() {
        let mut all_dropped: Vec<String> = Vec::new();
        for (idx, item) in result.items.iter().enumerate() {
            if !included[idx] {
                all_dropped.extend(item.lines.iter().cloned());
            }
        }
        elided = all_dropped.len();
        let joined = all_dropped.join(
            "
",
        );
        let href = opts.handles.put("elide", joined.as_bytes(), "text")?;
        handles.push(crate::types::HandleRef {
            id: href.id.clone(),
            kind: href.kind.clone(),
            bytes: href.bytes,
            hash: href.hash.clone(),
        });
        for (i, n) in gaps {
            lines[i] = format!("...+{n} lines ({})", href.id);
        }
    }

    let mut meta = result.meta.clone();
    for note in &mut meta.notes {
        let (r, n) = opts.redactor.redact(note);
        redactions += n;
        *note = r;
    }
    meta.redactions = result.meta.redactions.saturating_add(redactions);

    let mut diagnostics = result.diagnostics.clone();
    for d in &mut diagnostics {
        let (r, n) = opts.redactor.redact(&d.file);
        redactions += n;
        d.file = r;
        let (r, n) = opts.redactor.redact(&d.message);
        redactions += n;
        d.message = r;
        if let Some(code) = &d.code {
            let (r, n) = opts.redactor.redact(code);
            redactions += n;
            d.code = Some(r);
        }
        if let Some(tool) = &d.tool {
            let (r, n) = opts.redactor.redact(tool);
            redactions += n;
            d.tool = Some(r);
        }
    }

    let out = JsonOut {
        ok: result.meta.ok,
        tool: result.tool,
        meta,
        data: JsonData {
            header,
            lines,
            elided_lines: elided,
        },
        diagnostics,
        handles,
        errors: Vec::new(),
    };
    let text = serde_json::to_string(&out)?
        + "
";
    Ok(Rendered {
        out_tokens: estimator::estimate_text(&text),
        text,
        ok: result.meta.ok,
        cli_code: cli_code_for(result),
        is_error: false,
        redactions,
    })
}

fn cli_code_for(result: &ToolResult) -> i32 {
    if result.meta.ok {
        return 0;
    }
    if matches!(result.tool, "run" | "sh") {
        return match result.meta.exit {
            Some(crate::types::Exit::Code(c)) if c != 0 => c.clamp(1, 255),
            Some(crate::types::Exit::Timeout) => 124,
            _ => 1,
        };
    }
    1
}

pub fn render(result: &ToolResult, opts: RenderOptions<'_>) -> Result<Rendered> {
    match opts.format {
        Format::Lean => render_lean(result, opts),
        Format::Json => render_json(result, opts),
    }
}

fn render_error_json(err: &WErr) -> String {
    let v = serde_json::json!({
        "ok": false,
        "tool": "",
        "meta": serde_json::Value::Null,
        "data": serde_json::Value::Null,
        "diagnostics": [],
        "handles": [],
        "errors": [{
            "code": err.code,
            "message": err.message,
            "hint": err.hint,
        }],
    });
    v.to_string() + "\n"
}
