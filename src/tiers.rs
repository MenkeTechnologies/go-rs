//! Which fusevm execution tier a program's bytecode actually reaches.
//!
//! Enabling the JIT is not the same as being compiled by it, and the only
//! honest way to tell the two apart is to ask the VM. This module runs a
//! program and then queries fusevm's own eligibility and cache predicates —
//! `is_block_eligible`, `block_jit_is_compiled`, `trace_is_compiled`,
//! `find_jit_region` — so the answer comes from the compiler that would have
//! done the work rather than from an assumption about it.
//!
//! `go --tiers file.go` prints the report.

use std::collections::BTreeMap;

use fusevm::{Chunk, ChunkBuilder, JitCompiler, Op};

/// A loop header — the target of a backward branch — and what became of it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Loop {
    /// Op index of the loop header the backward branch jumps to.
    pub anchor: usize,
    /// Whether fusevm would accept this loop's body as a trace. Asked of
    /// `is_trace_eligible` with the body's ops — the same predicate the
    /// recorder applies to what it recorded, which for a loop whose body has
    /// no early exit is the same op sequence.
    pub trace_eligible: bool,
    /// Whether a compiled trace is installed for this header after the run.
    pub traced: bool,
    /// Whether the tracing JIT gave up on this header.
    pub blacklisted: bool,
}

/// What the tiers did with one chunk.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ChunkTiers {
    /// Which chunk this is — `main` for a whole Go program.
    pub name: String,
    /// Ops in the compiled chunk.
    pub ops: usize,
    /// Whether every op in the chunk is block-JIT eligible, which is what the
    /// whole-chunk block tier requires.
    pub block_eligible: bool,
    /// Whether the block tier holds compiled native code for this chunk.
    pub block_compiled: bool,
    /// The largest contiguous block-eligible op range, if any is large enough
    /// for fusevm to consider it worth compiling.
    pub largest_eligible_region: Option<(usize, usize)>,
    /// Every loop header, and whether the tracing JIT compiled it.
    pub loops: Vec<Loop>,
    /// Op kinds the **block** tier refuses, by occurrence count — what keeps
    /// the whole chunk from being compiled in one piece.
    ///
    /// Not the same question as whether a loop is traced: the tracing tier
    /// takes `GetVar` / `SetVar` (fusevm promotes a referenced global to a
    /// register at trace entry and spills it at every exit), so a chunk can
    /// list those here and still reach native code through a trace.
    pub ineligible: BTreeMap<String, usize>,
}

impl ChunkTiers {
    /// Whether any tier holds compiled native code for this chunk.
    pub fn reaches_native(&self) -> bool {
        self.block_compiled || self.loops.iter().any(|l| l.traced)
    }
}

/// What the tiers did with one program.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Report {
    /// Every chunk the program compiled to, in the order they were lowered.
    pub chunks: Vec<ChunkTiers>,
}

impl Report {
    /// Whether any tier holds compiled native code for any of the program's
    /// chunks.
    pub fn reaches_native(&self) -> bool {
        self.chunks.iter().any(|c| c.reaches_native())
    }
}

impl std::fmt::Display for ChunkTiers {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "ops                     {}", self.ops)?;
        writeln!(f, "block-JIT eligible      {}", self.block_eligible)?;
        writeln!(f, "block-JIT compiled      {}", self.block_compiled)?;
        match self.largest_eligible_region {
            Some((s, e)) => writeln!(f, "largest eligible region {s}..{e} ({} ops)", e - s)?,
            None => writeln!(f, "largest eligible region none")?,
        }
        if self.loops.is_empty() {
            writeln!(f, "loops                   none")?;
        }
        for l in &self.loops {
            writeln!(
                f,
                "loop @{:<4}             trace-eligible={} traced={} blacklisted={}",
                l.anchor, l.trace_eligible, l.traced, l.blacklisted
            )?;
        }
        if self.ineligible.is_empty() {
            writeln!(f, "block-ineligible ops    none")?;
        } else {
            writeln!(f, "block-ineligible ops")?;
            for (name, count) in &self.ineligible {
                writeln!(f, "  {name:<22}{count}")?;
            }
        }
        Ok(())
    }
}

impl std::fmt::Display for Report {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        // A single-chunk program needs no section headers; the fleet's
        // multi-chunk frontends label each one.
        let label = self.chunks.len() > 1;
        for c in &self.chunks {
            if label {
                writeln!(f, "== {} ==", c.name)?;
            }
            write!(f, "{c}")?;
        }
        write!(f, "reaches native code     {}", self.reaches_native())
    }
}

/// Compile and run `src`, then report which tiers took it.
///
/// The program is run because tier membership is a runtime fact: the block tier
/// compiles after its warmup threshold and the tracing tier only after a loop
/// has gone round enough times to be recorded. go-rs writes program output
/// straight to the process stdout, so the program's own output precedes the
/// report — what is measured is what an ordinary run does.
pub fn report(src: &str) -> Result<Report, String> {
    // The chunk lowered here is the same bytecode the run below executes, and
    // fusevm keys its compiled code by the chunk's op hash, so asking this copy
    // is asking about the run that just happened.
    let chunk = crate::compile(src)?;
    crate::run_str(src)?;
    Ok(inspect(&chunk))
}

/// Report on an already-executed program chunk.
pub fn inspect(chunk: &Chunk) -> Report {
    Report {
        chunks: vec![inspect_chunk("main", chunk)],
    }
}

/// Report on one already-executed chunk.
pub fn inspect_chunk(name: &str, chunk: &Chunk) -> ChunkTiers {
    let jit = JitCompiler::new();
    let loops = loop_anchors(&chunk.ops)
        .into_iter()
        .map(|anchor| Loop {
            anchor,
            trace_eligible: body_of(&chunk.ops, anchor)
                .is_some_and(|body| jit.is_trace_eligible(body, anchor)),
            traced: jit.trace_is_compiled(chunk, anchor),
            blacklisted: jit.trace_is_blacklisted(chunk, anchor),
        })
        .collect();

    let mut ineligible: BTreeMap<String, usize> = BTreeMap::new();
    for op in &chunk.ops {
        if !op_is_eligible(&jit, op) {
            *ineligible.entry(op_name(op)).or_default() += 1;
        }
    }

    ChunkTiers {
        name: name.to_string(),
        ops: chunk.ops.len(),
        block_eligible: jit.is_block_eligible(chunk),
        block_compiled: jit.block_jit_is_compiled(chunk),
        largest_eligible_region: jit.find_jit_region(chunk),
        loops,
        ineligible,
    }
}

/// Every op index a backward branch jumps to — fusevm anchors a trace at each.
fn loop_anchors(ops: &[Op]) -> Vec<usize> {
    let mut anchors: Vec<usize> = ops
        .iter()
        .enumerate()
        .filter_map(|(ip, op)| match op {
            Op::Jump(t)
            | Op::JumpIfTrue(t)
            | Op::JumpIfFalse(t)
            | Op::JumpIfTrueKeep(t)
            | Op::JumpIfFalseKeep(t)
                if *t <= ip =>
            {
                Some(*t)
            }
            _ => None,
        })
        .collect();
    anchors.sort_unstable();
    anchors.dedup();
    anchors
}

/// The op sequence one iteration of the loop at `anchor` runs: from the header
/// through the backward branch that closes it. `None` when nothing closes it.
fn body_of(ops: &[Op], anchor: usize) -> Option<&[Op]> {
    let close = ops.iter().enumerate().position(|(ip, op)| {
        ip >= anchor
            && matches!(
                op,
                Op::Jump(t) | Op::JumpIfTrue(t) | Op::JumpIfFalse(t)
                    if *t == anchor
            )
    })?;
    Some(&ops[anchor..=close])
}

/// Whether fusevm's block tier accepts this op, asked by handing the JIT a
/// chunk holding just that op. Whole-chunk eligibility is the conjunction of
/// the per-op decision, so a one-op chunk isolates it.
fn op_is_eligible(jit: &JitCompiler, op: &Op) -> bool {
    let mut b = ChunkBuilder::new();
    b.emit(op.clone(), 1);
    jit.is_block_eligible(&b.build())
}

/// An op's variant name, without its operands, so occurrences group.
fn op_name(op: &Op) -> String {
    let text = format!("{op:?}");
    match text.split_once('(') {
        Some((name, _)) => name.to_string(),
        None => text,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The report can say yes. A counted loop built by hand in the rotated
    /// shape — entered at its body, closed by a conditional backward branch —
    /// is what fusevm's trace compiler accepts, and after a run the report
    /// finds the installed trace. Without this, every "not traced" below could
    /// be a report that only ever says no.
    #[test]
    fn a_rotated_slot_loop_reaches_a_compiled_trace() {
        let mut b = ChunkBuilder::new();
        b.emit(Op::LoadInt(0), 1);
        b.emit(Op::SetSlot(0), 1);
        let enter = b.emit(Op::Jump(usize::MAX), 1);
        let body = b.current_pos();
        b.emit(Op::GetSlot(0), 1);
        b.emit(Op::LoadInt(1), 1);
        b.emit(Op::Add, 1);
        b.emit(Op::SetSlot(0), 1);
        let cond = b.current_pos();
        b.patch_jump(enter, cond);
        b.emit(Op::GetSlot(0), 1);
        b.emit(Op::LoadInt(200_000), 1);
        b.emit(Op::NumLt, 1);
        b.emit(Op::JumpIfTrue(body), 1);
        b.emit(Op::GetSlot(0), 1);
        let chunk = b.build();

        let mut vm = fusevm::VM::new(chunk.clone());
        vm.enable_tracing_jit();
        vm.run();

        let report = inspect(&chunk);
        assert_eq!(report.chunks[0].loops.len(), 1, "{report}");
        assert!(report.chunks[0].loops[0].traced, "{report}");
        assert!(report.reaches_native(), "{report}");
    }

    /// A Go `for` inside a function counts in frame slots, so its body holds
    /// nothing the tiers refuse and fusevm's recorder accepts the sequence —
    /// and it now reaches a compiled trace.
    ///
    /// This used to assert the opposite, and said so: `for` was emitted in the
    /// unrotated shape — a forward `JumpIfFalse` exit closed by an
    /// unconditional backward `Jump` — which the trace compiler records and
    /// then declines, so the hottest shape a Go program has stayed in the
    /// interpreter however hot it got. `Compiler::compile_for` now emits every
    /// `for` rotated, which is the shape
    /// [`a_rotated_slot_loop_reaches_a_compiled_trace`] proves fusevm accepts.
    ///
    /// Keeping it as an assertion rather than deleting it is the point: rotate
    /// the lowering back, or emit a loop body holding an op the tiers refuse,
    /// and this fails.
    #[test]
    fn a_go_for_loop_reaches_a_compiled_trace() {
        let report = report(
            "package main\nfunc f(n int) int { t := 0; for i := 0; i < n; i++ { t += i }; return t }\nfunc main() { _ = f(200000) }",
        )
        .expect("runs");
        let counted = report.chunks[0]
            .loops
            .iter()
            .find(|l| l.trace_eligible)
            .unwrap_or_else(|| panic!("a trace-eligible loop: {report}"));
        assert!(counted.traced, "{report}");
        assert!(!counted.blacklisted, "{report}");
        assert!(report.reaches_native(), "{report}");
    }

    /// `for {}` has no condition to branch on, so its back edge stays an
    /// unconditional `Jump` and the tracing tier declines it — the one loop
    /// form rotation cannot reach. Separating it from the test above is what
    /// keeps "reaches native" honest about which shapes actually do.
    #[test]
    fn a_bare_for_keeps_its_unconditional_back_edge() {
        let report = report(
            "package main\nfunc f(n int) int { t, i := 0, 0; for { if i >= n { break }; t += i; i++ }; return t }\nfunc main() { _ = f(200000) }",
        )
        .expect("runs");
        assert!(
            report.chunks[0].loops.iter().all(|l| !l.traced),
            "{report}"
        );
        assert!(!report.reaches_native(), "{report}");
    }

    /// `for i := range n` walks `0 … n-1`, so the loop binds `$i` itself rather
    /// than reading it back out of a materialized key list. That removes the
    /// per-iteration `GINDEX_GET`, and with it the last `CallBuiltin` in the
    /// body — which is what the tracing tier refuses outright (`fusevm`'s
    /// `is_trace_op_allowed_at`: `Op::CallBuiltin(_, _) => false`). Rotation
    /// alone did not reach this loop form; dropping the key list did.
    #[test]
    fn a_range_over_an_integer_reaches_a_compiled_trace() {
        let report = report(
            "package main\nfunc f(n int) int { t := 0; for i := range n { t = (t + i) % 1000003 }; return t }\nfunc main() { _ = f(200000) }",
        )
        .expect("runs");
        let counted = report.chunks[0]
            .loops
            .iter()
            .find(|l| l.trace_eligible)
            .unwrap_or_else(|| panic!("a trace-eligible loop: {report}"));
        assert!(counted.traced, "{report}");
        assert!(report.reaches_native(), "{report}");
    }

    /// The same loop, in a function that also has a slice parameter, is
    /// trace-**eligible** and still never runs a trace.
    ///
    /// Nothing about the loop changed — the slice is not read inside it. The
    /// refusal is `fusevm`'s: `VM::refresh_slot_buffers` classifies a frame's
    /// slots and sets one `slots_all_numeric` flag for the whole frame, and
    /// `lookup_trace_for_backward` returns the anchor unentered whenever that
    /// flag is false and a numeric hook is installed — which go-rs always
    /// installs, because Go's fixed-width overflow is what it decides. A
    /// `Value::Obj` slice handle in any slot of the frame therefore keeps every
    /// loop in that function interpreted.
    ///
    /// fusevm already does the finer thing for globals (flagged per index, with
    /// the trace's entry guard refusing only on the indices it reads) and
    /// already knows which slots a trace touches (`collect_trace_slots`), so
    /// the per-slot version is available upstream — but it is not reachable
    /// from a frontend, which cannot keep a Go program's slices, maps, strings
    /// and structs out of its frames.
    ///
    /// This asserts the ceiling rather than working around it: when fusevm
    /// gains the per-slot gate, this test fails and says where to look.
    #[test]
    fn a_slice_in_the_frame_keeps_a_numeric_loop_interpreted() {
        let report = report(
            "package main\nfunc f(s []int, n int) int { t := 0; for i := range n { t = (t + i) % 1000003 }; return t }\nfunc main() { _ = f([]int{1}, 200000) }",
        )
        .expect("runs");
        let counted = report.chunks[0]
            .loops
            .iter()
            .find(|l| l.trace_eligible)
            .unwrap_or_else(|| panic!("a trace-eligible loop: {report}"));
        assert!(!counted.traced, "{report}");
        assert!(!report.reaches_native(), "{report}");
    }

    /// A program whose only work is a print reaches no tier: the builtin call
    /// is what keeps the whole chunk out of the block tier, and the loops the
    /// linked runtime helpers contribute are not even trace-eligible.
    #[test]
    fn a_printing_program_reaches_no_tier() {
        let report = report("package main\nimport \"fmt\"\nfunc main() { fmt.Println(2 + 3*4) }")
            .expect("runs");
        assert!(!report.chunks[0].block_eligible, "{report}");
        assert!(
            report.chunks[0].ineligible.contains_key("CallBuiltin"),
            "the print is what refuses: {report}"
        );
        assert!(
            report.chunks[0].loops.iter().all(|l| !l.trace_eligible),
            "{report}"
        );
        assert!(!report.reaches_native(), "{report}");
    }
}
