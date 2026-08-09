//! The numeric hook on a mixed `Int`/`Float` pair.
//!
//! fusevm answers a mixed pair natively by promoting the integer to `f64`, but
//! past 2^53 that promotion rounds to a neighbouring value, so it delegates the
//! pair here instead. These tests call [`numeric_hook`] directly — the pair does
//! not reach it through a compiled program on the pinned fusevm, so a
//! source-level test would silently prove nothing.
//!
//! The reference answers are Go's, checked against `go1.26.5 darwin/arm64`:
//! a mixed pair only exists in valid Go as an interface comparison, where
//! equality is decided by dynamic type before value, and every other operator
//! on one is a compile error.

use fusevm::{NumOp, Value};
use gors::host::numeric_hook;

/// 3^34 = 16_677_181_699_666_569, past 2^53. Its `f64` image is …568, so a
/// promoting implementation answers about a value that is not this one.
const L: i64 = 16_677_181_699_666_569;

/// Every op the hook accepts, so a new `NumOp` variant cannot slip past these
/// tests unclassified.
const ARITH: [NumOp; 6] = [
    NumOp::Add,
    NumOp::Sub,
    NumOp::Mul,
    NumOp::Div,
    NumOp::Mod,
    NumOp::Pow,
];
const ORDER: [NumOp; 4] = [NumOp::Lt, NumOp::Gt, NumOp::Le, NumOp::Ge];

/// The literal the promotion would round to — proof the two are distinct, so a
/// promoting implementation is answering about the wrong integer.
#[test]
fn promotion_of_l_rounds_to_a_neighbour() {
    assert_ne!(L as f64 as i64, L, "2^53 premise is wrong: L survives f64");
    assert_eq!(L as f64 as i64, 16_677_181_699_666_568);
}

/// Interface equality is decided by dynamic type, so a mixed pair is never
/// equal — independent of the values, and so independent of the rounding.
#[test]
fn eq_on_a_mixed_pair_is_always_false() {
    for y in [L as f64, 0.5, 2.0, 0.0, -1.0, f64::NAN, f64::INFINITY] {
        for (a, b) in [
            (Value::Int(L), Value::Float(y)),
            (Value::Float(y), Value::Int(L)),
            (Value::Int(1), Value::Float(1.0)),
            (Value::Float(1.0), Value::Int(1)),
        ] {
            assert_eq!(
                numeric_hook(NumOp::Eq, &a, &b),
                Ok(Value::bool(false)),
                "Eq {a:?} {b:?}"
            );
            assert_eq!(
                numeric_hook(NumOp::Ne, &a, &b),
                Ok(Value::bool(true)),
                "Ne {a:?} {b:?}"
            );
        }
    }
}

/// `Int(1)` vs `Float(1.0)` is the case a string-equality or a promoting hook
/// both get wrong in the *same* direction: they answer `true`, but Go's
/// `any(1) == any(1.0)` is `false`.
#[test]
fn numerically_equal_mixed_pair_is_still_not_equal() {
    assert_eq!(
        numeric_hook(NumOp::Eq, &Value::Int(1), &Value::Float(1.0)),
        Ok(Value::bool(false))
    );
    // The rounded pair: L's f64 image compares equal to L only after the
    // rounding Go never performs.
    assert_eq!(
        numeric_hook(NumOp::Eq, &Value::Int(L), &Value::Float(L as f64)),
        Ok(Value::bool(false))
    );
}

/// Arithmetic and ordering on a mixed pair are compile errors in Go, so the
/// hook reports rather than inventing an answer. Before this was classified,
/// `Add` concatenated into a *string* and `Lt` ordered lexicographically.
#[test]
fn arithmetic_and_ordering_on_a_mixed_pair_are_rejected() {
    for op in ARITH.iter().chain(ORDER.iter()).copied() {
        for (a, b, types) in [
            (Value::Int(L), Value::Float(0.5), "int and float64"),
            (Value::Float(0.5), Value::Int(L), "float64 and int"),
        ] {
            let got = numeric_hook(op, &a, &b);
            let want = Err(format!(
                "go-rs: invalid operation: operator {op:?} not defined on mismatched types {types}"
            ));
            assert_eq!(got, want, "{op:?} on {a:?} {b:?}");
        }
    }
}

/// The specific silent-wrong-answer shapes this replaces: `+` must never yield
/// a string from two numbers, and `<` must never be lexicographic.
#[test]
fn mixed_add_never_concatenates_and_lt_is_never_lexicographic() {
    let add = numeric_hook(NumOp::Add, &Value::Int(L), &Value::Float(0.5));
    assert!(add.is_err(), "mixed + produced {add:?}");
    if let Ok(v) = &add {
        assert!(!matches!(v, Value::Str(_)), "mixed + concatenated: {v:?}");
    }

    // Lexicographically "16677181699666569" < "2", so a string-ordering hook
    // answers Lt=true here while L is numerically the far larger operand.
    assert!(numeric_hook(NumOp::Lt, &Value::Int(L), &Value::Float(2.0)).is_err());
}

/// The paths that share this hook must keep working: an integer pair still
/// wraps (Go's fixed-width overflow) and nil is still the additive identity.
#[test]
fn int_pair_and_nil_paths_are_untouched() {
    assert_eq!(
        numeric_hook(NumOp::Add, &Value::Int(i64::MAX), &Value::Int(1)),
        Ok(Value::Int(i64::MIN))
    );
    assert_eq!(
        numeric_hook(NumOp::Add, &Value::Undef, &Value::Float(2.5)),
        Ok(Value::Float(2.5))
    );
    assert_eq!(
        numeric_hook(NumOp::Add, &Value::Int(7), &Value::Undef),
        Ok(Value::Int(7))
    );
    // A string operand still concatenates and still orders as a string.
    assert_eq!(
        numeric_hook(NumOp::Add, &Value::str("a"), &Value::Int(1)),
        Ok(Value::str("a1"))
    );
    assert_eq!(
        numeric_hook(NumOp::Lt, &Value::str("a"), &Value::str("b")),
        Ok(Value::bool(true))
    );
}
