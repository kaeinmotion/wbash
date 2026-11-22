use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::error::{Result, WErr};
use crate::types::Format;

#[derive(Debug, Clone, Copy)]
pub struct CommonArgs {
    pub format: Format,
    pub budget: Option<u32>,
    pub budget_set_by_user: bool,
}

/// Per-tool default budgets (spec §5.2).
pub fn default_budget(tool: &str) -> u32 {
    match tool {
        "read" => 4000,
        "run" | "check" | "sh" => 1200,
        "batch" => 4000,
        _ => 1200,
    }
}

/// Extract and validate the cross-cutting `format` / `budget` args, returning
/// the remaining tool args.
pub fn strip_common(mut args: Value, tool: &str) -> Result<(CommonArgs, Value)> {
    let obj = args.as_object_mut().ok_or_else(|| {
        WErr::invalid_args(
            "arguments must be a JSON object",
            format!("see `w {tool} --help`"),
        )
    })?;

    let format = match obj.remove("format") {
        None => Format::Lean,
        Some(Value::String(s)) => Format::parse(&s)?,
        Some(_) => {
            return Err(WErr::invalid_args(
                "format must be a string",
                "use format=lean or format=json",
            ))
        }
    };

    let mut budget_set_by_user = false;
    let budget = match obj.remove("budget") {
        None => None,
        Some(v) => {
            let n = v.as_u64().ok_or_else(|| {
                WErr::invalid_args("budget must be a positive integer", "budget=600")
            })?;
            if !(64..=32_768).contains(&n) {
                return Err(WErr::invalid_args(
                    format!("budget {n} out of range"),
                    "budget must be between 64 and 32768",
                ));
            }
            budget_set_by_user = true;
            Some(n as u32)
        }
    };

    Ok((
        CommonArgs {
            format,
            budget,
            budget_set_by_user,
        },
        args,
    ))
}

/// Deserialize tool args with unknown fields rejected, producing teaching errors.
pub fn parse_args<T: DeserializeOwned>(args: Value, tool: &str, hint: &str) -> Result<T> {
    serde_json::from_value(args).map_err(|e| {
        let msg = e.to_string();
        let cleaned = msg.split(" at line ").next().unwrap_or(&msg).to_string();
        WErr::invalid_args(format!("{tool}: {cleaned}"), hint)
    })
}

/// Parse a `start:end` 1-based inclusive range. Either side may be omitted.
pub fn parse_line_range(spec: &str, len: usize) -> Result<(usize, usize)> {
    parse_line_range_inner(spec, len, false)
}

/// Parse a `start:end` 1-based inclusive range, clamping to file length.
pub fn parse_line_range_clamped(spec: &str, len: usize) -> Result<(usize, usize)> {
    parse_line_range_inner(spec, len, true)
}

fn parse_line_range_inner(spec: &str, len: usize, clamp: bool) -> Result<(usize, usize)> {
    let (a, b) = spec
        .split_once(':')
        .or_else(|| spec.split_once(".."))
        .or_else(|| spec.split_once('-'))
        .or_else(|| spec.split_once(','))
        .ok_or_else(|| {
            WErr::invalid_args(
                format!("bad lines range '{spec}'"),
                "use lines=10:40 or lines=10-40 (1-based inclusive)",
            )
        })?;
    if len == 0 {
        return Err(WErr::new("invalid_range", "file has no lines"));
    }
    let start = if a.trim().is_empty() {
        1
    } else {
        a.trim().parse::<usize>().map_err(|_| {
            WErr::invalid_args(format!("bad start in lines='{spec}'"), "use lines=10:40")
        })?
    };
    let end = if b.trim().is_empty() {
        len
    } else {
        b.trim().parse::<usize>().map_err(|_| {
            WErr::invalid_args(format!("bad end in lines='{spec}'"), "use lines=10:40")
        })?
    };
    if start == 0 || end == 0 {
        return Err(WErr::new(
            "invalid_range",
            format!("lines='{spec}' is not a valid 1-based inclusive range"),
        ));
    }
    if clamp {
        let mut s = start.min(len);
        let mut e = end.min(len);
        if s > e {
            std::mem::swap(&mut s, &mut e);
        }
        return Ok((s, e));
    }
    if start > end {
        return Err(WErr::new(
            "invalid_range",
            format!("lines='{spec}' is not a valid 1-based inclusive range"),
        ));
    }
    if start > len || end > len {
        return Err(WErr::new(
            "invalid_range",
            format!("lines='{spec}' exceeds file length {len}"),
        ));
    }
    Ok((start, end))
}

pub fn non_empty_str<'a>(args: &'a Value, key: &str, tool: &str) -> Result<&'a str> {
    args.get(key)
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| {
            WErr::invalid_args(
                format!("{tool}: missing required arg '{key}'"),
                format!("{key}=<value>"),
            )
        })
}
