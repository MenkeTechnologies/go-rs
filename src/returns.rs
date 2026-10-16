//! The return-statement rules `go` enforces at build time: a function with
//! results must end in a terminating statement ("missing return"), and each
//! `return` must supply as many values as the function has results.
//!
//! Both are syntactic, so the parser applies them to each function body as it
//! finishes parsing it (declared functions and function literals alike).

use crate::ast::{Expr, Stmt};

/// Check a function body against its result list.
pub fn check(results: &[String], result_names: &[String], body: &[Stmt]) -> Result<(), String> {
    let named = result_names.iter().any(|n| !n.is_empty());
    check_returns(body, results.len(), named)?;
    if !results.is_empty() && !ends_terminating(body) {
        return Err("missing return".to_string());
    }
    Ok(())
}

/// Every `return` in `body` (not descending into function literals, which have
/// their own result list) against `want` results.
fn check_returns(body: &[Stmt], want: usize, named: bool) -> Result<(), String> {
    for s in body {
        match s {
            Stmt::Return(vals, line) => check_return(vals, *line, want, named)?,
            Stmt::If {
                then, els, init, ..
            } => {
                check_returns(then, want, named)?;
                check_returns(els, want, named)?;
                if let Some(i) = init {
                    check_returns(std::slice::from_ref(i), want, named)?;
                }
            }
            Stmt::For { body, .. } | Stmt::ForRange { body, .. } | Stmt::Block(body) => {
                check_returns(body, want, named)?
            }
            Stmt::Switch { cases, default, .. } => {
                for c in cases {
                    check_returns(&c.body, want, named)?;
                }
                check_returns(default.as_deref().unwrap_or(&[]), want, named)?;
            }
            Stmt::TypeSwitch { cases, default, .. } => {
                for c in cases {
                    check_returns(&c.body, want, named)?;
                }
                check_returns(default.as_deref().unwrap_or(&[]), want, named)?;
            }
            Stmt::Select { cases, default, .. } => {
                for c in cases {
                    check_returns(&c.body, want, named)?;
                }
                check_returns(default.as_deref().unwrap_or(&[]), want, named)?;
            }
            _ => {}
        }
    }
    Ok(())
}

fn check_return(vals: &[Expr], line: u32, want: usize, named: bool) -> Result<(), String> {
    let have = vals.len();
    // `return f()` may supply every result from one multi-value call.
    let spread = have == 1 && want > 1 && matches!(vals[0], Expr::Call { .. });
    if have == want || spread || (have == 0 && named) {
        return Ok(());
    }
    let what = if have > want {
        "too many return values"
    } else {
        "not enough return values"
    };
    Err(format!("{what} (line {line})"))
}

fn ends_terminating(body: &[Stmt]) -> bool {
    body.last().is_some_and(terminating)
}

/// Go's "terminating statement" rule (spec, Terminating statements).
fn terminating(s: &Stmt) -> bool {
    match s {
        Stmt::Return(..) | Stmt::Goto(..) => true,
        Stmt::ExprStmt(Expr::Call { func, .. }) => matches!(&**func, Expr::Ident(n) if n == "panic"),
        Stmt::Block(b) => ends_terminating(b),
        Stmt::If { then, els, .. } => ends_terminating(then) && ends_terminating(els),
        Stmt::For {
            cond: None,
            body,
            label,
            ..
        } => !breaks_out(body, label.as_deref()),
        Stmt::Switch {
            label,
            cases,
            default,
            ..
        } => {
            default.is_some()
                && cases.iter().all(|c| ends_terminating_or_falls(&c.body))
                && default.as_deref().is_some_and(ends_terminating_or_falls)
                && !cases.iter().any(|c| breaks_out(&c.body, label.as_deref()))
                && !default.as_deref().is_some_and(|d| breaks_out(d, label.as_deref()))
        }
        Stmt::TypeSwitch {
            cases, default, ..
        } => {
            default.is_some()
                && cases.iter().all(|c| ends_terminating(&c.body))
                && default.as_deref().is_some_and(ends_terminating)
                && !cases.iter().any(|c| breaks_out(&c.body, None))
                && !default.as_deref().is_some_and(|d| breaks_out(d, None))
        }
        Stmt::Select { cases, default, .. } => {
            cases.iter().all(|c| ends_terminating(&c.body))
                && default.as_deref().is_none_or(ends_terminating)
                && !cases.iter().any(|c| breaks_out(&c.body, None))
                && !default.as_deref().is_some_and(|d| breaks_out(d, None))
        }
        _ => false,
    }
}

fn ends_terminating_or_falls(body: &[Stmt]) -> bool {
    matches!(body.last(), Some(Stmt::Fallthrough(_))) || ends_terminating(body)
}

/// Whether `body` holds a `break` that leaves the enclosing statement: an
/// unlabeled one not nested in an inner `for` / `switch` / `select`, or one
/// naming `label` from anywhere inside.
fn breaks_out(body: &[Stmt], label: Option<&str>) -> bool {
    body.iter().any(|s| stmt_breaks_out(s, label, true))
}

fn stmt_breaks_out(s: &Stmt, label: Option<&str>, direct: bool) -> bool {
    let any = |b: &[Stmt], direct: bool| b.iter().any(|s| stmt_breaks_out(s, label, direct));
    match s {
        Stmt::Break(_, None) => direct,
        Stmt::Break(_, Some(l)) => Some(l.as_str()) == label,
        Stmt::If { then, els, .. } => any(then, direct) || any(els, direct),
        Stmt::Block(b) => any(b, direct),
        Stmt::For { body, .. } | Stmt::ForRange { body, .. } => any(body, false),
        Stmt::Switch { cases, default, .. } => {
            cases.iter().any(|c| any(&c.body, false)) || any(default.as_deref().unwrap_or(&[]), false)
        }
        Stmt::TypeSwitch { cases, default, .. } => {
            cases.iter().any(|c| any(&c.body, false)) || any(default.as_deref().unwrap_or(&[]), false)
        }
        Stmt::Select { cases, default, .. } => {
            cases.iter().any(|c| any(&c.body, false)) || any(default.as_deref().unwrap_or(&[]), false)
        }
        _ => false,
    }
}
