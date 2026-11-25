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

/// Flexible line range deserializer for tool args.
/// Accepts:
/// - string: `"1-30"`, `"10:40"`, `"10"`
/// - array: `[1, 30]`, `[10]`
/// - integer: `10`
pub fn deserialize_line_range_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct LineRangeVisitor;

    impl<'de> serde::de::Visitor<'de> for LineRangeVisitor {
        type Value = Option<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("a line range string (\"1-30\"), array ([1, 30]), or integer (10)")
        }

        fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(LineRangeVisitor)
        }

        fn visit_str<E>(self, s: &str) -> std::result::Result<Self::Value, E> {
            Ok(Some(s.to_string()))
        }

        fn visit_string<E>(self, s: String) -> std::result::Result<Self::Value, E> {
            Ok(Some(s))
        }

        fn visit_i64<E>(self, n: i64) -> std::result::Result<Self::Value, E> {
            Ok(Some(format!("{n}-{n}")))
        }

        fn visit_u64<E>(self, n: u64) -> std::result::Result<Self::Value, E> {
            Ok(Some(format!("{n}-{n}")))
        }

        fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut items = Vec::new();
            while let Some(elem) = seq.next_element::<serde_json::Value>()? {
                match elem {
                    serde_json::Value::Number(n) => items.push(n.to_string()),
                    serde_json::Value::String(s) => items.push(s),
                    _ => {}
                }
            }
            if items.is_empty() {
                Ok(None)
            } else if items.len() == 1 {
                Ok(Some(format!("{}-{}", items[0], items[0])))
            } else {
                Ok(Some(format!("{}-{}", items[0], items[1])))
            }
        }
    }

    deserializer.deserialize_option(LineRangeVisitor)
}

/// Split a command line string into arguments respecting quotes.
pub fn split_shell_tokens(cmd: &str) -> Vec<String> {
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

/// Flexible u32 deserializer accepting an integer or string (e.g. "2" or 2).
pub fn deserialize_u32_lenient<'de, D>(deserializer: D) -> std::result::Result<u32, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct U32Visitor;

    impl<'de> serde::de::Visitor<'de> for U32Visitor {
        type Value = u32;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer or string representing an integer")
        }

        fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(v as u32)
        }

        fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            Ok(v.max(0) as u32)
        }

        fn visit_str<E>(self, s: &str) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            s.parse::<u32>().map_err(serde::de::Error::custom)
        }
    }

    deserializer.deserialize_any(U32Visitor)
}

/// Flexible Option<u32> deserializer accepting an integer, string, or null.
pub fn deserialize_u32_opt_lenient<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<u32>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptU32Visitor;

    impl<'de> serde::de::Visitor<'de> for OptU32Visitor {
        type Value = Option<u32>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an integer, string representing an integer, or null")
        }

        fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(OptU32Visitor)
        }

        fn visit_u64<E>(self, v: u64) -> std::result::Result<Self::Value, E> {
            Ok(Some(v as u32))
        }

        fn visit_i64<E>(self, v: i64) -> std::result::Result<Self::Value, E> {
            Ok(Some(v.max(0) as u32))
        }

        fn visit_str<E>(self, s: &str) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            s.parse::<u32>().map(Some).map_err(serde::de::Error::custom)
        }
    }

    deserializer.deserialize_option(OptU32Visitor)
}

/// Flexible argv deserializer for tool args.
/// Accepts either an array of strings (`["cargo", "test"]`) or a single command string (`"cargo test"`).
pub fn deserialize_argv<'de, D>(deserializer: D) -> std::result::Result<Vec<String>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct ArgvVisitor;

    impl<'de> serde::de::Visitor<'de> for ArgvVisitor {
        type Value = Vec<String>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an array of strings or a single command string")
        }

        fn visit_str<E>(self, s: &str) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if s.trim_start().starts_with('[') {
                if let Ok(items) = serde_json::from_str::<Vec<String>>(s) {
                    if !items.is_empty() {
                        return Ok(items);
                    }
                }
            }
            let parts = split_shell_tokens(s);
            if parts.is_empty() {
                Ok(vec![s.to_string()])
            } else {
                Ok(parts)
            }
        }

        fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                items.push(item);
            }
            Ok(items)
        }
    }

    deserializer.deserialize_any(ArgvVisitor)
}

/// Flexible Option<Vec<String>> deserializer for tool args.
/// Accepts either an array of strings (`["cargo", "test"]`), a single command string (`"cargo test"`), or null/empty.
pub fn deserialize_argv_opt<'de, D>(
    deserializer: D,
) -> std::result::Result<Option<Vec<String>>, D::Error>
where
    D: serde::Deserializer<'de>,
{
    struct OptArgvVisitor;

    impl<'de> serde::de::Visitor<'de> for OptArgvVisitor {
        type Value = Option<Vec<String>>;

        fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
            formatter.write_str("an array of strings, a single command string, or null")
        }

        fn visit_none<E>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_unit<E>(self) -> std::result::Result<Self::Value, E> {
            Ok(None)
        }

        fn visit_some<D>(self, deserializer: D) -> std::result::Result<Self::Value, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            deserializer.deserialize_any(OptArgvVisitor)
        }

        fn visit_str<E>(self, s: &str) -> std::result::Result<Self::Value, E>
        where
            E: serde::de::Error,
        {
            if s.trim_start().starts_with('[') {
                if let Ok(items) = serde_json::from_str::<Vec<String>>(s) {
                    return if items.is_empty() {
                        Ok(None)
                    } else {
                        Ok(Some(items))
                    };
                }
            }
            let parts = split_shell_tokens(s);
            if parts.is_empty() {
                if s.trim().is_empty() {
                    Ok(None)
                } else {
                    Ok(Some(vec![s.to_string()]))
                }
            } else {
                Ok(Some(parts))
            }
        }

        fn visit_seq<A>(self, mut seq: A) -> std::result::Result<Self::Value, A::Error>
        where
            A: serde::de::SeqAccess<'de>,
        {
            let mut items = Vec::new();
            while let Some(item) = seq.next_element::<String>()? {
                items.push(item);
            }
            if items.is_empty() {
                Ok(None)
            } else {
                Ok(Some(items))
            }
        }
    }

    deserializer.deserialize_option(OptArgvVisitor)
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
    let (a, b) = if let Some(pair) = spec
        .split_once(':')
        .or_else(|| spec.split_once("..="))
        .or_else(|| spec.split_once(".."))
        .or_else(|| spec.split_once('-'))
        .or_else(|| spec.split_once(','))
    {
        pair
    } else if spec.trim().trim_start_matches('L').parse::<usize>().is_ok() {
        let trimmed = spec.trim().trim_start_matches('L');
        (trimmed, trimmed)
    } else {
        return Err(WErr::invalid_args(
            format!("bad lines range '{spec}'"),
            "use lines=10:40 or lines=10-40 (1-based inclusive)",
        ));
    };
    if len == 0 {
        return Err(WErr::new("invalid_range", "file has no lines"));
    }
    let clean_a = a.trim().trim_start_matches('L');
    let clean_b = b.trim().trim_start_matches('L');
    let start = if clean_a.is_empty() {
        1
    } else {
        clean_a.parse::<usize>().map_err(|_| {
            WErr::invalid_args(format!("bad start in lines='{spec}'"), "use lines=10:40")
        })?
    };
    let end = if clean_b.is_empty() {
        len
    } else {
        clean_b.parse::<usize>().map_err(|_| {
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
