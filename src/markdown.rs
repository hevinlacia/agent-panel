use std::collections::HashMap;

use regex::Regex;
use serde_json::{json, Value};

pub(crate) fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF | 0x20000..=0x2A6DF | 0x2A700..=0x2B73F | 0x2B740..=0x2B81F | 0x2B820..=0x2CEAF | 0xF900..=0xFAFF)
}

pub(crate) fn markdown_outline(body: &str) -> Vec<Value> {
    body.lines()
        .filter_map(markdown_heading)
        .take(80)
        .map(|(level, title)| {
            json!({
                "level": level,
                "title": title,
                "anchor": markdown_anchor(&title),
            })
        })
        .collect()
}

pub(crate) fn markdown_section(body: &str, heading: &str) -> Option<String> {
    let target = heading.trim().to_ascii_lowercase();
    if target.is_empty() {
        return None;
    }
    let lines: Vec<&str> = body.lines().collect();
    let mut start = None;
    let mut start_level = 0usize;
    for (idx, line) in lines.iter().enumerate() {
        let Some((level, title)) = markdown_heading(line) else {
            continue;
        };
        let title_lower = title.to_ascii_lowercase();
        if title_lower == target
            || title_lower.contains(&target)
            || markdown_anchor(&title) == target
        {
            start = Some(idx);
            start_level = level;
            break;
        }
    }
    let start = start?;
    let mut end = lines.len();
    for (idx, line) in lines.iter().enumerate().skip(start + 1) {
        if let Some((level, _)) = markdown_heading(line) {
            if level <= start_level {
                end = idx;
                break;
            }
        }
    }
    Some(lines[start..end].join("\n"))
}

pub(crate) fn markdown_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let hashes = trimmed.chars().take_while(|c| *c == '#').count();
    if !(1..=6).contains(&hashes) {
        return None;
    }
    let rest = trimmed[hashes..].trim();
    if rest.is_empty() {
        return None;
    }
    Some((hashes, rest.trim_matches('#').trim().to_string()))
}

pub(crate) fn markdown_anchor(title: &str) -> String {
    title
        .trim()
        .to_ascii_lowercase()
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || is_cjk(c) {
                c
            } else {
                '-'
            }
        })
        .collect::<String>()
        .split('-')
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("-")
}

pub(crate) fn html_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            _ => out.push(c),
        }
    }
    out
}

pub(crate) fn inline_html(s: &str) -> String {
    let esc = html_escape(s);
    let code_re = Regex::new(r"`([^`]+)`").unwrap();
    let mut codes: Vec<String> = Vec::new();
    let step1 = code_re
        .replace_all(&esc, |caps: &regex::Captures<'_>| {
            let idx = codes.len();
            codes.push(format!(
                "<code>{}</code>",
                caps.get(1).map(|m| m.as_str()).unwrap_or("")
            ));
            format!("\u{0}{idx}\u{0}")
        })
        .into_owned();
    let bold_re = Regex::new(r"\*\*([^*]+)\*\*").unwrap();
    let step2 = bold_re
        .replace_all(&step1, "<strong>$1</strong>")
        .into_owned();
    let link_re = Regex::new(r"\[([^\]\n]+)\]\(([^)\s]+)\)").unwrap();
    let step3 = link_re
        .replace_all(
            &step2,
            r#"<a href="$2" target="_blank" rel="noreferrer">$1</a>"#,
        )
        .into_owned();
    let mut out = step3;
    for (idx, code) in codes.iter().enumerate() {
        out = out.replace(&format!("\u{0}{idx}\u{0}"), code);
    }
    out
}

pub(crate) fn render_table_html(rows: &[String]) -> String {
    if rows.is_empty() {
        return String::new();
    }
    let cells = |r: &str| -> Vec<String> {
        let t = r.trim().trim_start_matches('|').trim_end_matches('|');
        t.split('|').map(|c| inline_html(c.trim())).collect()
    };
    let header = cells(&rows[0]);
    let is_separator =
        |r: &str| !r.is_empty() && r.chars().all(|c| matches!(c, '|' | '-' | ':' | ' '));
    let body_start = if rows.len() >= 2 && is_separator(&rows[1]) {
        2
    } else {
        1
    };
    let mut out = String::from("<table>");
    if !header.is_empty() {
        out.push_str("<thead><tr>");
        for h in &header {
            out.push_str(&format!("<th>{h}</th>"));
        }
        out.push_str("</tr></thead>");
    }
    out.push_str("<tbody>");
    for row in &rows[body_start..] {
        let cols = cells(row);
        out.push_str("<tr>");
        for c in cols {
            out.push_str(&format!("<td>{c}</td>"));
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
    out
}

pub(crate) fn render_markdown_html(src: &str) -> String {
    let ul_re = Regex::new(r"^[-*]\s+(.*)$").unwrap();
    let ol_re = Regex::new(r"^\d+[.)]\s+(.*)$").unwrap();
    let checkbox_re = Regex::new(r"^[-*]\s+\[([ xX])\]\s+(.*)$").unwrap();
    let mut out = String::new();
    let lines: Vec<&str> = src.lines().collect();
    let mut i = 0usize;
    let mut in_code = false;
    let mut list_tag: Option<&'static str> = None;
    let mut table: Vec<String> = Vec::new();

    while i < lines.len() {
        let line = lines[i].trim_end();
        if in_code {
            if line.trim_start().starts_with("```") {
                out.push_str("</code></pre>");
                in_code = false;
            } else {
                out.push_str(&html_escape(line));
                out.push('\n');
            }
            i += 1;
            continue;
        }
        let trimmed = line.trim();
        if trimmed.starts_with("```") {
            if !table.is_empty() {
                out.push_str(&render_table_html(&table));
                table.clear();
            }
            if let Some(tag) = list_tag.take() {
                out.push_str(&format!("</{tag}>"));
            }
            let lang = trimmed.trim_start_matches("```").trim();
            out.push_str("<pre><code");
            if !lang.is_empty() {
                out.push_str(&format!(" class=\"lang-{}\"", html_escape(lang)));
            }
            out.push('>');
            in_code = true;
            i += 1;
            continue;
        }
        if trimmed.is_empty() {
            if !table.is_empty() {
                out.push_str(&render_table_html(&table));
                table.clear();
            }
            if let Some(tag) = list_tag.take() {
                out.push_str(&format!("</{tag}>"));
            }
            i += 1;
            continue;
        }
        // Table row: a |...| line (also catches `| --- |` separator rows).
        if trimmed.starts_with('|') && trimmed.ends_with('|') {
            if let Some(tag) = list_tag.take() {
                out.push_str(&format!("</{tag}>"));
            }
            table.push(trimmed.to_string());
            i += 1;
            continue;
        }
        if !table.is_empty() {
            out.push_str(&render_table_html(&table));
            table.clear();
        }
        // Headings.
        if trimmed.starts_with('#') {
            if let Some(tag) = list_tag.take() {
                out.push_str(&format!("</{tag}>"));
            }
            let mut level = 0usize;
            let mut rest = trimmed;
            while rest.starts_with('#') && level < 6 {
                level += 1;
                rest = &rest[1..];
            }
            out.push_str(&format!(
                "<h{level}>{}</h{level}>",
                inline_html(rest.trim())
            ));
            i += 1;
            continue;
        }
        // Blockquote.
        if let Some(rest) = trimmed.strip_prefix('>') {
            if let Some(tag) = list_tag.take() {
                out.push_str(&format!("</{tag}>"));
            }
            out.push_str(&format!(
                "<blockquote>{}</blockquote>",
                inline_html(rest.trim())
            ));
            i += 1;
            continue;
        }
        // Horizontal rule.
        if trimmed.len() >= 3 && trimmed.chars().all(|c| matches!(c, '-' | '*' | '_' | ' ')) {
            if let Some(tag) = list_tag.take() {
                out.push_str(&format!("</{tag}>"));
            }
            out.push_str("<hr/>");
            i += 1;
            continue;
        }
        // Checkbox list (before plain ul so `- [ ] x` is not swallowed).
        if let Some(caps) = checkbox_re.captures(trimmed) {
            if list_tag != Some("ul") {
                if let Some(tag) = list_tag.take() {
                    out.push_str(&format!("</{tag}>"));
                }
                out.push_str("<ul class=\"task-list\">");
                list_tag = Some("ul");
            }
            let checked = matches!(&caps[1], "x" | "X");
            let text = inline_html(caps.get(2).map(|m| m.as_str()).unwrap_or(""));
            out.push_str(&format!(
                "<li class=\"task-item\"><input type=\"checkbox\" disabled{} /><span>{}</span></li>",
                if checked { " checked" } else { "" },
                text
            ));
            i += 1;
            continue;
        }
        // Unordered list.
        if let Some(caps) = ul_re.captures(trimmed) {
            if list_tag != Some("ul") {
                if let Some(tag) = list_tag.take() {
                    out.push_str(&format!("</{tag}>"));
                }
                out.push_str("<ul>");
                list_tag = Some("ul");
            }
            out.push_str(&format!(
                "<li>{}</li>",
                inline_html(caps.get(1).map(|m| m.as_str()).unwrap_or(""))
            ));
            i += 1;
            continue;
        }
        // Ordered list.
        if let Some(caps) = ol_re.captures(trimmed) {
            if list_tag != Some("ol") {
                if let Some(tag) = list_tag.take() {
                    out.push_str(&format!("</{tag}>"));
                }
                out.push_str("<ol>");
                list_tag = Some("ol");
            }
            out.push_str(&format!(
                "<li>{}</li>",
                inline_html(caps.get(1).map(|m| m.as_str()).unwrap_or(""))
            ));
            i += 1;
            continue;
        }
        // Plain paragraph.
        if let Some(tag) = list_tag.take() {
            out.push_str(&format!("</{tag}>"));
        }
        out.push_str(&format!("<p>{}</p>", inline_html(trimmed)));
        i += 1;
    }
    if !table.is_empty() {
        out.push_str(&render_table_html(&table));
    }
    if let Some(tag) = list_tag.take() {
        out.push_str(&format!("</{tag}>"));
    }
    if in_code {
        out.push_str("</code></pre>");
    }
    out
}

pub(crate) fn upsert_markdown_section(raw: &str, heading: &str, content: &str) -> String {
    let target = normalize_heading_text(heading);
    let heading_line = if heading.trim_start().starts_with('#') {
        heading.trim().to_string()
    } else {
        format!("## {}", heading.trim())
    };
    let lines: Vec<&str> = raw.lines().collect();
    let mut start: Option<(usize, usize)> = None;
    for (idx, line) in lines.iter().enumerate() {
        if let Some((level, text)) = parse_markdown_heading(line) {
            if normalize_heading_text(&text) == target {
                start = Some((idx, level));
                break;
            }
        }
    }
    let new_block = format!("{}\n{}", heading_line, content.trim());
    if let Some((start_idx, level)) = start {
        let mut end_idx = lines.len();
        for (idx, line) in lines.iter().enumerate().skip(start_idx + 1) {
            if let Some((next_level, _)) = parse_markdown_heading(line) {
                if next_level <= level {
                    end_idx = idx;
                    break;
                }
            }
        }
        let mut out = Vec::new();
        out.extend(lines[..start_idx].iter().map(|s| s.to_string()));
        out.push(new_block);
        out.extend(lines[end_idx..].iter().map(|s| s.to_string()));
        format!("{}\n", out.join("\n").trim_end())
    } else {
        format!("{}\n\n{}\n", raw.trim_end(), new_block)
    }
}

pub(crate) fn parse_markdown_heading(line: &str) -> Option<(usize, String)> {
    let trimmed = line.trim_start();
    let level = trimmed.chars().take_while(|c| *c == '#').count();
    if level == 0
        || level > 6
        || !trimmed
            .chars()
            .nth(level)
            .map(|c| c.is_whitespace())
            .unwrap_or(false)
    {
        return None;
    }
    Some((
        level,
        trimmed[level..].trim().trim_matches('#').trim().to_string(),
    ))
}

pub(crate) fn normalize_heading_text(value: &str) -> String {
    value
        .trim()
        .trim_start_matches('#')
        .trim()
        .trim_matches('#')
        .trim()
        .to_lowercase()
}

pub(crate) fn set_frontmatter_field(raw: &str, key: &str, value: &str) -> String {
    let mut lines: Vec<String> = raw.split('\n').map(|s| s.to_string()).collect();
    if lines.first().map(|s| s.as_str()) != Some("---") {
        let body = raw.trim_start_matches('\n');
        if value.is_empty() {
            return body.to_string();
        }
        return format!("---\n{}: {}\n---\n{}", key, yaml_quote(value), body);
    }
    let end = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| l.as_str() == "---")
        .map(|(i, _)| i);
    let Some(end) = end else {
        return raw.to_string();
    };
    let mut found = None;
    for i in 1..end {
        if lines[i]
            .split_once(':')
            .map(|(k, _)| k.trim() == key)
            .unwrap_or(false)
        {
            found = Some(i);
            break;
        }
    }
    if let Some(i) = found {
        if value.is_empty() {
            lines.remove(i);
        } else {
            lines[i] = format!("{}: {}", key, yaml_quote(value));
        }
    } else if !value.is_empty() {
        lines.insert(end, format!("{}: {}", key, yaml_quote(value)));
    }
    lines.join("\n")
}

pub(crate) fn yaml_quote(value: &str) -> String {
    if value
        .chars()
        .all(|c| c.is_alphanumeric() || "-_./:#?=&%".contains(c))
    {
        value.to_string()
    } else {
        serde_json::to_string(value).unwrap_or_else(|_| value.to_string())
    }
}

#[derive(Debug, Clone)]
pub(crate) struct Frontmatter {
    pub(crate) fields: HashMap<String, String>,
    pub(crate) body: String,
}

pub(crate) fn parse_frontmatter(text: &str) -> Frontmatter {
    let normalized = text.replace("\r\n", "\n");
    if !normalized.starts_with("---\n") && normalized.trim() != "---" {
        return Frontmatter {
            fields: HashMap::new(),
            body: normalized,
        };
    }
    let lines: Vec<&str> = normalized.split('\n').collect();
    let Some(end) = lines
        .iter()
        .enumerate()
        .skip(1)
        .find(|(_, l)| **l == "---")
        .map(|(i, _)| i)
    else {
        return Frontmatter {
            fields: HashMap::new(),
            body: normalized,
        };
    };
    let mut fields = HashMap::new();
    for line in &lines[1..end] {
        if line.trim().is_empty() || line.trim_start().starts_with('#') {
            continue;
        }
        if let Some((k, v)) = line.split_once(':') {
            let value = v.trim().trim_matches('"').trim_matches('\'').to_string();
            fields.insert(k.trim().to_string(), value);
        }
    }
    Frontmatter {
        fields,
        body: lines[end + 1..].join("\n"),
    }
}
