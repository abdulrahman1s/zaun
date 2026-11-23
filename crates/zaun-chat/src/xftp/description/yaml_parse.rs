//! The small subset of YAML that file descriptions use: block maps, block lists, plain or quoted scalars, `[]` and `{}`.
use super::yaml_node::Node;

struct Line {
    indent: usize,
    text: String,
}

fn is_dash(text: &str) -> bool {
    text == "-" || text.starts_with("- ")
}

/// `key: rest` or `key:`; a colon inside a scalar (`1:abc`) does not start a value.
fn split_key(text: &str) -> Option<(&str, &str)> {
    if text.starts_with(['"', '\'']) {
        return None;
    }
    let at = text
        .find(": ")
        .or_else(|| text.ends_with(':').then(|| text.len() - 1))?;
    Some((text[..at].trim_end(), text[at + 1..].trim()))
}

fn unquote(s: &str) -> Result<String, String> {
    let Some(inner) = s.strip_prefix('"').or_else(|| s.strip_prefix('\'')) else {
        return Ok(s.to_owned());
    };
    let quote = &s[..1];
    let inner = inner
        .strip_suffix(quote)
        .ok_or("unterminated quoted string")?;
    if quote == "'" {
        return Ok(inner.replace("''", "'"));
    }
    let mut out = String::new();
    let mut chars = inner.chars();
    while let Some(c) = chars.next() {
        if c != '\\' {
            out.push(c);
            continue;
        }
        out.push(match chars.next() {
            Some('n') => '\n',
            Some('t') => '\t',
            Some(c @ ('"' | '\\' | '/')) => c,
            _ => return Err("unsupported escape in quoted string".into()),
        });
    }
    Ok(out)
}

fn value(rest: &str) -> Result<Node, String> {
    match rest {
        "[]" => Ok(Node::Seq(Vec::new())),
        "{}" => Ok(Node::Map(Vec::new())),
        r if r.starts_with(['[', '{', '&', '*', '|', '>', '!']) => Err("unsupported YAML".into()),
        r => Ok(Node::Scalar(unquote(r)?)),
    }
}

fn lines(text: &str) -> Result<Vec<Line>, String> {
    let mut out = Vec::new();
    for raw in text.lines() {
        if raw.contains('\t') {
            return Err("tab in YAML".into());
        }
        let trimmed = raw.trim_end();
        let body = trimmed.trim_start();
        if body.is_empty() || body.starts_with('#') || body == "---" {
            continue;
        }
        out.push(Line {
            indent: trimmed.len() - body.len(),
            text: body.to_owned(),
        });
    }
    Ok(out)
}

pub(super) fn parse(text: &str) -> Result<Node, String> {
    let mut lines = lines(text)?;
    let Some(first) = lines.first() else {
        return Ok(Node::Map(Vec::new()));
    };
    let (mut i, indent) = (0, first.indent);
    let node = block(&mut lines, &mut i, indent)?;
    if i < lines.len() {
        return Err("unexpected indentation".into());
    }
    Ok(node)
}

fn block(lines: &mut [Line], i: &mut usize, indent: usize) -> Result<Node, String> {
    if is_dash(&lines[*i].text) {
        return seq(lines, i, indent);
    }
    if split_key(&lines[*i].text).is_some() {
        return map(lines, i, indent);
    }
    let node = value(&lines[*i].text)?;
    *i += 1;
    Ok(node)
}

fn seq(lines: &mut [Line], i: &mut usize, indent: usize) -> Result<Node, String> {
    let mut items = Vec::new();
    while *i < lines.len() && lines[*i].indent == indent && is_dash(&lines[*i].text) {
        let rest = lines[*i].text[1..].trim_start().to_owned();
        if rest.is_empty() {
            *i += 1;
            items.push(nested(lines, i, indent)?);
        } else {
            // "- a: b" is a map whose first line starts after the dash.
            let used = lines[*i].text.len() - rest.len();
            lines[*i] = Line {
                indent: indent + used,
                text: rest,
            };
            let inner = lines[*i].indent;
            items.push(block(lines, i, inner)?);
        }
    }
    Ok(Node::Seq(items))
}

/// What follows a bare `key:` or `-`: a block indented deeper, else nothing.
fn nested(lines: &mut [Line], i: &mut usize, indent: usize) -> Result<Node, String> {
    match lines.get(*i).map(|l| l.indent) {
        Some(deeper) if deeper > indent => block(lines, i, deeper),
        _ => Ok(Node::Scalar(String::new())),
    }
}

fn map(lines: &mut [Line], i: &mut usize, indent: usize) -> Result<Node, String> {
    let mut entries = Vec::new();
    while *i < lines.len() && lines[*i].indent == indent && !is_dash(&lines[*i].text) {
        let text = lines[*i].text.clone();
        let (key, rest) = split_key(&text).ok_or("expected key: value")?;
        *i += 1;
        let node = if !rest.is_empty() {
            value(rest)?
        } else if lines
            .get(*i)
            .is_some_and(|l| l.indent == indent && is_dash(&l.text))
        {
            seq(lines, i, indent)?
        } else {
            nested(lines, i, indent)?
        };
        entries.push((unquote(key)?, node));
    }
    Ok(Node::Map(entries))
}
