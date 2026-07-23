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
    /// A float literal: its `f64` value and, when it fits, an exact decimal
    /// `(mantissa, scale)` meaning `mantissa · 10⁻ˢᶜᵃˡᵉ` — used to constant-fold
    /// float expressions with Go's arbitrary-precision rounding.
    Float(f64, Option<(i128, i32)>),
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
    Interface,
    If,
    Else,
    For,
    Range,
    Return,
    Break,
    Continue,
    True,
    False,
    Go,
    Chan,
    Select,
    Switch,
    Fallthrough,
    Defer,
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
    AmpAssign,
    PipeAssign,
    CaretAssign,
    ShlAssign,
    ShrAssign,
    AndNotAssign,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    PlusPlus,
    MinusMinus,
    Arrow, // <- (channel send/receive)
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    AndAnd,
    OrOr,
    Not,
    /// `|` — only appears in a generic type-constraint union (`~int | ~float64`),
    /// which go-rs erases; not an expression operator.
    Pipe,
    /// `~` — the generic underlying-type constraint marker (`~int`); erased.
    Tilde,
    /// `&` — the address-of operator (`&T{…}`, `&x`) or bitwise AND (`a & b`).
    Amp,
    /// `^` — bitwise XOR (`a ^ b`) or complement (`^x`).
    Caret,
    /// `<<` — left shift.
    Shl,
    /// `>>` — right shift.
    Shr,
    /// `&^` — bit clear (AND NOT).
    AndNot,
    /// `...` — variadic parameter marker / argument spread.
    Ellipsis,
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
                | Tok::Float(..)
                | Tok::Str(_)
                | Tok::True
                | Tok::False
                | Tok::Return
                | Tok::Break
                | Tok::Continue
                | Tok::Fallthrough
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
            // Base-prefixed integer literals: `0x1F`, `0o17`, `0b1010` (with `_`
            // digit separators). Parsed here; decimal/float handled below.
            if c == '0' && i + 1 < bytes.len() {
                let (base, allowed): (Option<u32>, &str) = match bytes[i + 1] {
                    b'x' | b'X' => (Some(16), "0123456789abcdefABCDEF_"),
                    b'o' | b'O' => (Some(8), "01234567_"),
                    b'b' | b'B' => (Some(2), "01_"),
                    _ => (None, ""),
                };
                if let Some(radix) = base {
                    i += 2;
                    let ds = i;
                    while i < bytes.len() && allowed.contains(bytes[i] as char) {
                        i += 1;
                    }
                    let digits = src[ds..i].replace('_', "");
                    // A value above i64::MAX (a uint64 constant like
                    // 0x8080808080808080) is reinterpreted as the i64 with the
                    // same bit pattern — go-rs stores integers in an i64.
                    let v = i64::from_str_radix(&digits, radix)
                        .or_else(|_| u64::from_str_radix(&digits, radix).map(|u| u as i64))
                        .map_err(|_| {
                            format!(
                                "go-rs: bad integer literal `{}` on line {line}",
                                &src[start..i]
                            )
                        })?;
                    out.push(Token {
                        kind: Tok::Int(v),
                        line,
                    });
                    continue;
                }
            }
            let mut is_float = false;
            while i < bytes.len() && ((bytes[i] as char).is_ascii_digit() || bytes[i] == b'_') {
                i += 1;
            }
            if i < bytes.len() && bytes[i] == b'.' && !(i + 1 < bytes.len() && bytes[i + 1] == b'.')
            {
                is_float = true;
                i += 1;
                while i < bytes.len() && ((bytes[i] as char).is_ascii_digit() || bytes[i] == b'_') {
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
            // `_` digit separators (`1_000`, `3.141_592`) are stripped for parsing.
            let clean = text.replace('_', "");
            if is_float {
                let v: f64 = clean
                    .parse()
                    .map_err(|_| format!("go-rs: bad float literal `{text}` on line {line}"))?;
                out.push(Token {
                    kind: Tok::Float(v, exact_decimal(&clean)),
                    line,
                });
            } else {
                let v: i64 = clean
                    .parse()
                    .or_else(|_| clean.parse::<u64>().map(|u| u as i64))
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
                    let (cp, next) = scan_escape(src, bytes, i + 1);
                    s.push(char::from_u32(cp).unwrap_or('\u{FFFD}'));
                    i = next;
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

        // rune literal — a Go rune is an int32 code point, so it lexes to an
        // integer token (matching string indexing and `range`, which also yield
        // int code points). Handles the full Go escape set: `\n \t \r \0 \\ \' \"`,
        // `\xHH` (hex byte), `\uHHHH` / `\UHHHHHHHH` (Unicode), and `\ooo` (octal).
        if c == '\'' {
            i += 1;
            let cp: u32 = if bytes[i] == b'\\' {
                let (cp, next) = scan_escape(src, bytes, i + 1);
                i = next;
                cp
            } else {
                let ch = src[i..].chars().next().unwrap();
                i += ch.len_utf8();
                ch as u32
            };
            if i >= bytes.len() || bytes[i] != b'\'' {
                return Err(format!("go-rs: unterminated rune literal on line {line}"));
            }
            i += 1;
            out.push(Token {
                kind: Tok::Int(cp as i64),
                line,
            });
            continue;
        }

        // operators & punctuation (longest match first)
        let three = if i + 2 < bytes.len() {
            &src[i..i + 3]
        } else {
            ""
        };
        let two = if i + 1 < bytes.len() {
            &src[i..i + 2]
        } else {
            ""
        };
        let (kind, adv) = match three {
            "<<=" => (Tok::ShlAssign, 3),
            ">>=" => (Tok::ShrAssign, 3),
            "&^=" => (Tok::AndNotAssign, 3),
            "..." => (Tok::Ellipsis, 3),
            _ => match two {
                ":=" => (Tok::Define, 2),
                "+=" => (Tok::PlusAssign, 2),
                "-=" => (Tok::MinusAssign, 2),
                "*=" => (Tok::StarAssign, 2),
                "/=" => (Tok::SlashAssign, 2),
                "%=" => (Tok::PercentAssign, 2),
                "++" => (Tok::PlusPlus, 2),
                "--" => (Tok::MinusMinus, 2),
                "<-" => (Tok::Arrow, 2),
                "==" => (Tok::EqEq, 2),
                "!=" => (Tok::NotEq, 2),
                "<=" => (Tok::Le, 2),
                ">=" => (Tok::Ge, 2),
                "&&" => (Tok::AndAnd, 2),
                "||" => (Tok::OrOr, 2),
                "<<" => (Tok::Shl, 2),
                ">>" => (Tok::Shr, 2),
                "&^" => (Tok::AndNot, 2),
                "&=" => (Tok::AmpAssign, 2),
                "|=" => (Tok::PipeAssign, 2),
                "^=" => (Tok::CaretAssign, 2),
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
                    '|' => (Tok::Pipe, 1),
                    '~' => (Tok::Tilde, 1),
                    // `&` — address-of (a single `&`; `&&` matched above).
                    '&' => (Tok::Amp, 1),
                    // `^` — bitwise XOR (binary) / complement (unary).
                    '^' => (Tok::Caret, 1),
                    other => {
                        return Err(format!(
                            "go-rs: unexpected character `{other}` on line {line}"
                        ))
                    }
                },
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

/// Parse a decimal float literal into an exact `(mantissa, scale)` such that its
/// value is `mantissa · 10⁻ˢᶜᵃˡᵉ`. Returns `None` for exponent forms or when the
/// digits overflow `i128` (the constant folder then falls back to `f64`).
fn exact_decimal(text: &str) -> Option<(i128, i32)> {
    // Exponent literals (`1e9`, `2.5e-3`) are left to the f64 path.
    if text.contains(['e', 'E']) {
        return None;
    }
    let (sign, body) = match text.strip_prefix('-') {
        Some(rest) => (-1i128, rest),
        None => (1i128, text),
    };
    let (int_part, frac_part) = match body.split_once('.') {
        Some((a, b)) => (a, b),
        None => (body, ""),
    };
    let digits = format!("{int_part}{frac_part}");
    let mantissa: i128 = digits.parse().ok()?;
    Some((sign * mantissa, frac_part.len() as i32))
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
        "interface" => Tok::Interface,
        "if" => Tok::If,
        "else" => Tok::Else,
        "for" => Tok::For,
        "range" => Tok::Range,
        "return" => Tok::Return,
        "break" => Tok::Break,
        "continue" => Tok::Continue,
        "true" => Tok::True,
        "false" => Tok::False,
        "go" => Tok::Go,
        "chan" => Tok::Chan,
        "select" => Tok::Select,
        "switch" => Tok::Switch,
        "fallthrough" => Tok::Fallthrough,
        "defer" => Tok::Defer,
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

/// Scan a Go escape sequence. `i` indexes the character just after the `\`.
/// Returns the resulting Unicode code point and the index past the sequence.
/// Handles `\xHH`, `\uHHHH`, `\UHHHHHHHH`, `\ooo` (octal), and the simple
/// single-char escapes (`\n \t \r \0 \\ \' \"` …).
fn scan_escape(src: &str, bytes: &[u8], i: usize) -> (u32, usize) {
    match bytes[i] {
        b'x' => {
            let hex = &src[i + 1..i + 3];
            (u32::from_str_radix(hex, 16).unwrap_or(0xFFFD), i + 3)
        }
        b'u' => {
            let hex = &src[i + 1..i + 5];
            (u32::from_str_radix(hex, 16).unwrap_or(0xFFFD), i + 5)
        }
        b'U' => {
            let hex = &src[i + 1..i + 9];
            (u32::from_str_radix(hex, 16).unwrap_or(0xFFFD), i + 9)
        }
        b'0'..=b'7' => {
            let oct = &src[i..i + 3];
            (u32::from_str_radix(oct, 8).unwrap_or(0xFFFD), i + 3)
        }
        other => (unescape(other as char) as u32, i + 1),
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
