//! A hand-written Go lexer with automatic semicolon insertion (ASI).
//!
//! Go's grammar is semicolon-terminated like C, but the language spec has the
//! lexer *insert* those semicolons so source rarely writes them: "a semicolon is
//! automatically inserted at the end of a non-blank line if the line's final
//! token is an identifier, a literal, one of the keywords `break`/`continue`/
//! `fallthrough`/`return`, or one of `++`, `--`, `)`, `]`, `}`" (Go spec,
//! *Semicolons*). go-rs implements exactly that rule here so the parser can
//! consume a uniform `stmt ;` stream, identical to how the real `go` scanner
//! feeds its parser.
//!
//! Covers the slice-1 surface: identifiers/keywords, integer/float/string/rune
//! literals, and the operators and punctuation used by declarations,
//! expressions, and control statements. Line (`//`) and block (`/* */`)
//! comments are skipped.

use std::fmt;

/// A lexical token with its 1-based source line (for error reporting).
#[derive(Debug, Clone, PartialEq)]
pub struct Token {
    pub kind: Tok,
    pub line: u32,
}

/// Token kinds.
#[derive(Debug, Clone, PartialEq)]
pub enum Tok {
    // literals & names
    Int(i64),
    Float(f64),
    Str(String),
    Ident(String),
    // keywords
    Package,
    Import,
    Func,
    Var,
    Const,
    Type,
    Struct,
    If,
    Else,
    For,
    Range,
    Return,
    Break,
    Continue,
    True,
    False,
    // punctuation
    LBrace,
    RBrace,
    LParen,
    RParen,
    LBracket,
    RBracket,
    Semi,
    Comma,
    Dot,
    Colon,
    // operators
    Assign,
    Define, // :=
    PlusAssign,
    MinusAssign,
    StarAssign,
    SlashAssign,
    PercentAssign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PlusPlus,
    MinusMinus,
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
    Not,
    Eof,
}

impl fmt::Display for Tok {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{self:?}")
    }
}

impl Tok {
    /// True when this token, as the last on a line, triggers Go's automatic
    /// semicolon insertion.
    fn ends_statement(&self) -> bool {
        matches!(
            self,
            Tok::Ident(_)
                | Tok::Int(_)
                | Tok::Float(_)
                | Tok::Str(_)
                | Tok::True
                | Tok::False
                | Tok::Return
                | Tok::Break
                | Tok::Continue
                | Tok::RParen
                | Tok::RBracket
                | Tok::RBrace
                | Tok::PlusPlus
                | Tok::MinusMinus
        )
    }
}

/// Append an automatic semicolon if the previous token ends a statement and the
/// last emitted token is not already a semicolon.
fn maybe_asi(out: &mut Vec<Token>, line: u32) {
    if let Some(last) = out.last() {
        if last.kind.ends_statement() {
            out.push(Token {
                kind: Tok::Semi,
                line,
            });
        }
    }
}

/// Lex `src` into a token vector terminated by `Tok::Eof`, with Go automatic
/// semicolons inserted.
pub fn lex(src: &str) -> Result<Vec<Token>, String> {
    let bytes = src.as_bytes();
    let mut i = 0usize;
    let mut line = 1u32;
    let mut out: Vec<Token> = Vec::new();

    while i < bytes.len() {
        let c = bytes[i] as char;

        // newline — the ASI decision point
        if c == '\n' {
            maybe_asi(&mut out, line);
            line += 1;
            i += 1;
            continue;
        }
        if c.is_whitespace() {
            i += 1;
            continue;
        }

        // comments
        if c == '/' && i + 1 < bytes.len() {
            match bytes[i + 1] as char {
                // line comment: stop before the newline so the '\n' branch still
                // runs ASI (Go treats `x := 1 // c` as terminated).
                '/' => {
                    i += 2;
                    while i < bytes.len() && bytes[i] != b'\n' {
                        i += 1;
                    }
                    continue;
                }
                // block comment: a comment spanning a newline acts like a
                // newline for ASI.
                '*' => {
                    i += 2;
                    let mut saw_newline = false;
                    while i + 1 < bytes.len() && !(bytes[i] == b'*' && bytes[i + 1] == b'/') {
                        if bytes[i] == b'\n' {
                            saw_newline = true;
                            line += 1;
                        }
                        i += 1;
                    }
                    i += 2; // consume closing */
                    if saw_newline {
                        maybe_asi(&mut out, line);
                    }
                    continue;
                }
                _ => {}
            }
        }

        // identifiers & keywords
        if c.is_ascii_alphabetic() || c == '_' {
            let start = i;
            while i < bytes.len() {
                let ch = bytes[i] as char;
                if ch.is_ascii_alphanumeric() || ch == '_' {
                    i += 1;
                } else {
                    break;
                }
            }
            out.push(Token {
                kind: keyword_or_ident(&src[start..i]),
                line,
            });
            continue;
        }

        // numbers (int or float)
        if c.is_ascii_digit() {
            let start = i;
            let mut is_float = false;
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' && !(i + 1 < bytes.len() && bytes[i + 1] == b'.')
            {
                is_float = true;
                i += 1;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
            }
            // exponent
            if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
                is_float = true;
                i += 1;
                if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
                    i += 1;
                }
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
            }
            let text = &src[start..i];
            if is_float {
                let v: f64 = text
                    .parse()
                    .map_err(|_| format!("go-rs: bad float literal `{text}` on line {line}"))?;
                out.push(Token {
                    kind: Tok::Float(v),
                    line,
                });
            } else {
                let v: i64 = text
                    .parse()
                    .map_err(|_| format!("go-rs: bad integer literal `{text}` on line {line}"))?;
                out.push(Token {
                    kind: Tok::Int(v),
                    line,
                });
            }
            continue;
        }

        // interpreted string literal
        if c == '"' {
            i += 1;
            let mut s = String::new();
            while i < bytes.len() && bytes[i] != b'"' {
                if bytes[i] == b'\\' && i + 1 < bytes.len() {
                    i += 1;
                    s.push(unescape(bytes[i] as char));
                    i += 1;
                } else {
                    let ch = src[i..].chars().next().unwrap();
                    if ch == '\n' {
                        line += 1;
                    }
                    s.push(ch);
                    i += ch.len_utf8();
                }
            }
            if i >= bytes.len() {
                return Err(format!("go-rs: unterminated string literal on line {line}"));
            }
            i += 1; // closing quote
            out.push(Token {
                kind: Tok::Str(s),
                line,
            });
            continue;
        }

        // raw string literal (backticks): no escapes, may span lines
        if c == '`' {
            i += 1;
            let mut s = String::new();
            while i < bytes.len() && bytes[i] != b'`' {
                let ch = src[i..].chars().next().unwrap();
                if ch == '\n' {
                    line += 1;
                }
                s.push(ch);
                i += ch.len_utf8();
            }
            if i >= bytes.len() {
                return Err(format!(
                    "go-rs: unterminated raw string literal on line {line}"
                ));
            }
            i += 1;
            out.push(Token {
                kind: Tok::Str(s),
                line,
            });
            continue;
        }

        // rune literal — modeled as a one-char string (slice 1)
        if c == '\'' {
            i += 1;
            let ch = if bytes[i] == b'\\' {
                i += 1;
                let c = unescape(bytes[i] as char);
                i += 1;
                c
            } else {
                let c = src[i..].chars().next().unwrap();
                i += c.len_utf8();
                c
            };
            if i >= bytes.len() || bytes[i] != b'\'' {
                return Err(format!("go-rs: unterminated rune literal on line {line}"));
            }
            i += 1;
            out.push(Token {
                kind: Tok::Str(ch.to_string()),
                line,
            });
            continue;
        }

        // operators & punctuation (longest match first)
        let two = if i + 1 < bytes.len() {
            &src[i..i + 2]
        } else {
            ""
        };
        let (kind, adv) = match two {
            ":=" => (Tok::Define, 2),
            "+=" => (Tok::PlusAssign, 2),
            "-=" => (Tok::MinusAssign, 2),
            "*=" => (Tok::StarAssign, 2),
            "/=" => (Tok::SlashAssign, 2),
            "%=" => (Tok::PercentAssign, 2),
            "++" => (Tok::PlusPlus, 2),
            "--" => (Tok::MinusMinus, 2),
            "==" => (Tok::EqEq, 2),
            "!=" => (Tok::NotEq, 2),
            "<=" => (Tok::Le, 2),
            ">=" => (Tok::Ge, 2),
            "&&" => (Tok::AndAnd, 2),
            "||" => (Tok::OrOr, 2),
            _ => match c {
                '{' => (Tok::LBrace, 1),
                '}' => (Tok::RBrace, 1),
                '(' => (Tok::LParen, 1),
                ')' => (Tok::RParen, 1),
                '[' => (Tok::LBracket, 1),
                ']' => (Tok::RBracket, 1),
                ';' => (Tok::Semi, 1),
                ',' => (Tok::Comma, 1),
                '.' => (Tok::Dot, 1),
                ':' => (Tok::Colon, 1),
                '=' => (Tok::Assign, 1),
                '+' => (Tok::Plus, 1),
                '-' => (Tok::Minus, 1),
                '*' => (Tok::Star, 1),
                '/' => (Tok::Slash, 1),
                '%' => (Tok::Percent, 1),
                '<' => (Tok::Lt, 1),
                '>' => (Tok::Gt, 1),
                '!' => (Tok::Not, 1),
                other => {
                    return Err(format!(
                        "go-rs: unexpected character `{other}` on line {line}"
                    ))
                }
            },
        };
        out.push(Token { kind, line });
        i += adv;
    }

    // Final line's ASI (a program ending without a trailing newline still
    // terminates its last statement), then EOF.
    maybe_asi(&mut out, line);
    out.push(Token {
        kind: Tok::Eof,
        line,
    });
    Ok(out)
}

fn keyword_or_ident(word: &str) -> Tok {
    match word {
        "package" => Tok::Package,
        "import" => Tok::Import,
        "func" => Tok::Func,
        "var" => Tok::Var,
        "const" => Tok::Const,
        "type" => Tok::Type,
        "struct" => Tok::Struct,
        "if" => Tok::If,
        "else" => Tok::Else,
        "for" => Tok::For,
        "range" => Tok::Range,
        "return" => Tok::Return,
        "break" => Tok::Break,
        "continue" => Tok::Continue,
        "true" => Tok::True,
        "false" => Tok::False,
        _ => Tok::Ident(word.to_string()),
    }
}

fn unescape(c: char) -> char {
    match c {
        'n' => '\n',
        't' => '\t',
        'r' => '\r',
        '0' => '\0',
        '\\' => '\\',
        '"' => '"',
        '`' => '`',
        '\'' => '\'',
        other => other,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<Tok> {
        lex(src).unwrap().into_iter().map(|t| t.kind).collect()
    }

    #[test]
    fn inserts_semicolon_after_statement_line() {
        let k = kinds("x := 1\ny := 2\n");
        // ... 1 ; ... 2 ;
        assert_eq!(k.iter().filter(|t| **t == Tok::Semi).count(), 2);
    }

    #[test]
    fn no_semicolon_after_open_brace_or_operator() {
        // A line ending in `{` or a binary operator must NOT get a semicolon.
        let k = kinds("func f() {\n  1 +\n  2\n}\n");
        // Only the `2` line and the closing `}` trigger ASI.
        assert_eq!(k.iter().filter(|t| **t == Tok::Semi).count(), 2);
    }

    #[test]
    fn line_comment_still_terminates() {
        let k = kinds("x := 1 // comment\n");
        assert!(k.contains(&Tok::Semi));
    }
}
