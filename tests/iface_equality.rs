//! Interface equality — dynamic type before value.
//!
//! [`iface_eq`] is the whole of Go's rule for `==` on an interface operand. Most
//! of it is covered end to end by `parity-scripts/iface_equality.go`, which
//! byte-diffs against the reference; these tests take the cases a compiled
//! program cannot reach on the pinned fusevm, plus the ones where a
//! source-level test would still pass with the rule removed.
//!
//! `NaN` is only reachable here at all: `math.NaN` is one of the stdlib calls
//! go-rs rejects at compile time (BUGS.md), so no `.go` file can produce one.
//!
//! The reference answers are Go's, checked against `go1.26.5 darwin/arm64`.

use fusevm::Value;
use gors::host::iface_eq;

/// The headline: `any(1) == any(1.0)` is false in Go and `any(1) == any(1)` is
/// true, so an implementation that answered a constant fails one of them.
#[test]
fn an_int_never_equals_a_float_but_does_equal_an_int() {
    assert!(!iface_eq(&Value::Int(1), &Value::Float(1.0)));
    assert!(!iface_eq(&Value::Float(1.0), &Value::Int(1)));
    assert!(iface_eq(&Value::Int(1), &Value::Int(1)));
    assert!(iface_eq(&Value::Float(1.0), &Value::Float(1.0)));
    assert!(!iface_eq(&Value::Int(1), &Value::Int(2)));
    assert!(!iface_eq(&Value::Float(1.0), &Value::Float(2.0)));
}

/// Past 2^53 the promotion fusevm would apply lands on a neighbouring value, so
/// a promoting implementation is answering about a number that is not the
/// operand. The type rule settles it before the arithmetic can matter.
///
/// 3^34 = 16_677_181_699_666_569; its `f64` image is …568.
#[test]
fn a_large_int_beside_its_own_rounded_float_is_still_unequal() {
    const L: i64 = 16_677_181_699_666_569;
    assert!(!iface_eq(&Value::Int(L), &Value::Float(L as f64)));
    assert!(!iface_eq(&Value::Float(L as f64), &Value::Int(L)));
    // …and the neighbour it rounds to is not equal either, for the same reason.
    assert!(!iface_eq(&Value::Int(L - 1), &Value::Float(L as f64)));
}

/// A number beside its own text. Both render as `"1"`, so a rule that compared
/// the rendered strings — which is what the numeric hook's fallback does —
/// calls them equal. Go does not.
#[test]
fn a_value_never_equals_its_own_rendering() {
    assert!(!iface_eq(&Value::Int(1), &Value::str("1")));
    assert!(!iface_eq(&Value::Float(1.0), &Value::str("1")));
    assert!(!iface_eq(&Value::Bool(true), &Value::str("true")));
    // The same texts do compare equal to each other.
    assert!(iface_eq(&Value::str("1"), &Value::str("1")));
    assert!(iface_eq(&Value::Bool(true), &Value::Bool(true)));
    assert!(!iface_eq(&Value::Bool(true), &Value::Bool(false)));
    assert!(!iface_eq(&Value::str("1"), &Value::str("2")));
}

/// `NaN != NaN` in Go, as in IEEE 754 — and it is not reachable from a `.go`
/// file here, because `math.NaN` is rejected at compile time. Comparing the
/// rendered strings would make it equal (`"NaN" == "NaN"`), so this is the case
/// that forces the float arm to compare as `f64`.
#[test]
fn nan_does_not_equal_itself_but_the_other_floats_do() {
    assert!(!iface_eq(&Value::Float(f64::NAN), &Value::Float(f64::NAN)));
    assert!(iface_eq(
        &Value::Float(f64::INFINITY),
        &Value::Float(f64::INFINITY)
    ));
    assert!(!iface_eq(
        &Value::Float(f64::INFINITY),
        &Value::Float(f64::NEG_INFINITY)
    ));
    // Go's `==` on floats is IEEE, so the two zeros are equal.
    assert!(iface_eq(&Value::Float(0.0), &Value::Float(-0.0)));
}

/// A nil interface equals only another nil interface. This is the case the
/// compiler routes for `err == nil`, so it has to hold for every other kind on
/// the other side.
#[test]
fn nil_equals_only_nil() {
    assert!(iface_eq(&Value::Undef, &Value::Undef));
    for v in [
        Value::Int(0),
        Value::Float(0.0),
        Value::str(""),
        Value::Bool(false),
    ] {
        assert!(!iface_eq(&Value::Undef, &v), "nil == {v:?}");
        assert!(!iface_eq(&v, &Value::Undef), "{v:?} == nil");
    }
}

/// Every unequal answer above must be the *type* deciding, not the value —
/// otherwise the rule would collapse the moment two different types held the
/// same bits. Each pair here is numerically or textually identical and still
/// unequal.
#[test]
fn the_type_decides_before_the_value_does() {
    let same_number: [(Value, Value); 3] = [
        (Value::Int(97), Value::Float(97.0)),
        (Value::Int(0), Value::Float(0.0)),
        (Value::Bool(true), Value::str("true")),
    ];
    for (a, b) in same_number {
        assert!(!iface_eq(&a, &b), "{a:?} == {b:?}");
        assert!(!iface_eq(&b, &a), "{b:?} == {a:?}");
        // The value half is not broken: each still equals itself.
        assert!(iface_eq(&a, &a));
        assert!(iface_eq(&b, &b));
    }
}
