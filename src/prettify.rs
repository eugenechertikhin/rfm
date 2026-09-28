//! Встроенный best-effort prettify для просмотрщика (без внешних утилит и крейтов).
//! Тип определяется по расширению файла; форматтеры переформатируют отступы,
//! но не являются строгими валидаторами.

use std::path::Path;

/// Поддерживаемые типы для prettify.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    Json,
    Xml,
}

/// Определяет тип по расширению файла (регистронезависимо).
pub fn kind_by_ext(path: &Path) -> Option<Kind> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    match ext.as_str() {
        "json" => Some(Kind::Json),
        "xml" => Some(Kind::Xml),
        _ => None,
    }
}

/// Форматирует текст согласно типу.
pub fn prettify(kind: Kind, text: &str) -> String {
    match kind {
        Kind::Json => prettify_json(text),
        Kind::Xml => prettify_xml(text),
    }
}

const STEP: &str = "   "; // 3 пробела на уровень (как json_pp)

fn push_indent(out: &mut String, level: usize) {
    for _ in 0..level {
        out.push_str(STEP);
    }
}

/// Переформатирует JSON: посимвольный проход с учётом строк/escape.
/// Пустые `{}`/`[]` остаются на одной строке; `,` — перенос, `:` — `": "`.
fn prettify_json(src: &str) -> String {
    let b: Vec<char> = src.chars().collect();
    let n = b.len();
    let mut out = String::new();
    let mut indent = 0usize;
    let mut i = 0;
    while i < n {
        let c = b[i];
        match c {
            ' ' | '\t' | '\n' | '\r' => i += 1, // существующие пробелы игнорируем
            '"' => {
                out.push('"');
                i += 1;
                while i < n {
                    let ch = b[i];
                    out.push(ch);
                    i += 1;
                    if ch == '\\' {
                        if i < n {
                            out.push(b[i]);
                            i += 1;
                        }
                    } else if ch == '"' {
                        break;
                    }
                }
            }
            '{' | '[' => {
                let close = if c == '{' { '}' } else { ']' };
                let mut j = i + 1;
                while j < n && matches!(b[j], ' ' | '\t' | '\n' | '\r') {
                    j += 1;
                }
                if j < n && b[j] == close {
                    out.push(c);
                    out.push(close);
                    i = j + 1;
                } else {
                    out.push(c);
                    indent += 1;
                    out.push('\n');
                    push_indent(&mut out, indent);
                    i += 1;
                }
            }
            '}' | ']' => {
                indent = indent.saturating_sub(1);
                out.push('\n');
                push_indent(&mut out, indent);
                out.push(c);
                i += 1;
            }
            ',' => {
                out.push(',');
                out.push('\n');
                push_indent(&mut out, indent);
                i += 1;
            }
            ':' => {
                out.push_str(": ");
                i += 1;
            }
            other => {
                out.push(other);
                i += 1;
            }
        }
    }
    out
}

/// Класс XML-токена-тега.
#[derive(PartialEq)]
enum TagKind {
    Open,
    Close,
    /// Самозакрытие/декларация/комментарий/doctype/CDATA — без изменения уровня.
    Standalone,
}

enum Token {
    Tag(String),
    Text(String),
}

fn classify(tag: &str) -> TagKind {
    if tag.starts_with("</") {
        TagKind::Close
    } else if tag.starts_with("<?") || tag.starts_with("<!") || tag.ends_with("/>") {
        TagKind::Standalone
    } else {
        TagKind::Open
    }
}

/// Разбивает XML на теги и текст. Комментарии и CDATA берутся целиком (могут содержать `>`).
fn tokenize_xml(src: &str) -> Vec<Token> {
    let b: Vec<char> = src.chars().collect();
    let n = b.len();
    let mut toks = Vec::new();
    let mut i = 0;
    let starts = |b: &[char], i: usize, s: &str| -> bool {
        let sc: Vec<char> = s.chars().collect();
        i + sc.len() <= b.len() && b[i..i + sc.len()] == sc[..]
    };
    let find = |b: &[char], from: usize, s: &str| -> Option<usize> {
        let sc: Vec<char> = s.chars().collect();
        (from..=b.len().saturating_sub(sc.len())).find(|&k| b[k..k + sc.len()] == sc[..])
    };
    while i < n {
        if b[i] == '<' {
            let end = if starts(&b, i, "<!--") {
                find(&b, i, "-->").map(|k| k + 3)
            } else if starts(&b, i, "<![CDATA[") {
                find(&b, i, "]]>").map(|k| k + 3)
            } else {
                find(&b, i, ">").map(|k| k + 1)
            };
            match end {
                Some(e) => {
                    toks.push(Token::Tag(b[i..e].iter().collect()));
                    i = e;
                }
                None => {
                    // нет закрывающего — остаток как текст
                    toks.push(Token::Text(b[i..].iter().collect()));
                    break;
                }
            }
        } else {
            let mut j = i;
            while j < n && b[j] != '<' {
                j += 1;
            }
            toks.push(Token::Text(b[i..j].iter().collect()));
            i = j;
        }
    }
    toks
}

/// Переформатирует XML: каждый тег с отступом по глубине; короткое содержимое
/// `<a>text</a>` и пустое `<a></a>` — в одну строку. Пробельный текст пропускается.
fn prettify_xml(src: &str) -> String {
    let toks = tokenize_xml(src);
    let mut out = String::new();
    let mut indent = 0usize;
    let line = |out: &mut String, indent: usize, s: &str| {
        if !out.is_empty() {
            out.push('\n');
        }
        push_indent(out, indent);
        out.push_str(s);
    };
    let mut i = 0;
    while i < toks.len() {
        match &toks[i] {
            Token::Tag(t) => match classify(t) {
                TagKind::Close => {
                    indent = indent.saturating_sub(1);
                    line(&mut out, indent, t);
                }
                TagKind::Standalone => line(&mut out, indent, t),
                TagKind::Open => {
                    // <a>text</a> в одну строку
                    if let (Some(Token::Text(tx)), Some(Token::Tag(ct))) =
                        (toks.get(i + 1), toks.get(i + 2))
                    {
                        if classify(ct) == TagKind::Close && !tx.trim().is_empty() {
                            line(&mut out, indent, &format!("{t}{}{ct}", tx.trim()));
                            i += 3;
                            continue;
                        }
                    }
                    // <a></a> в одну строку
                    if let Some(Token::Tag(ct)) = toks.get(i + 1) {
                        if classify(ct) == TagKind::Close {
                            line(&mut out, indent, &format!("{t}{ct}"));
                            i += 2;
                            continue;
                        }
                    }
                    line(&mut out, indent, t);
                    indent += 1;
                }
            },
            Token::Text(s) => {
                let tr = s.trim();
                if !tr.is_empty() {
                    line(&mut out, indent, tr);
                }
            }
        }
        i += 1;
    }
    out
}

#[cfg(test)]
#[path = "prettify_test.rs"]
mod tests;
