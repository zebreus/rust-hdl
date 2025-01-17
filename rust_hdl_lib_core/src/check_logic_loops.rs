use crate::ast::{Verilog, VerilogExpression};
use crate::atom::{Atom, AtomKind};
use crate::block::Block;
use crate::check_error::{CheckError, PathedName, PathedNameList};
use crate::named_path::NamedPath;
use crate::probe::Probe;
use crate::verilog_visitor::VerilogVisitor;
use std::collections::HashSet;

#[derive(Copy, Clone, Debug, PartialEq)]
enum Mode {
    Ignore,
    Read,
    Write,
}

struct VerilogLogicLoopDetector {
    local_vars_written: HashSet<String>,
    mode: Mode,
    violations: Vec<String>,
}

impl Default for VerilogLogicLoopDetector {
    fn default() -> Self {
        Self {
            local_vars_written: Default::default(),
            mode: Mode::Ignore,
            violations: Default::default(),
        }
    }
}

impl VerilogVisitor for VerilogLogicLoopDetector {
    fn visit_slice_assignment(
        &mut self,
        base: &VerilogExpression,
        _width: &usize,
        offset: &VerilogExpression,
        replacement: &VerilogExpression,
    ) {
        let current_mode = self.mode;
        self.mode = Mode::Read;
        self.visit_expression(offset);
        self.visit_expression(replacement);
        self.mode = Mode::Write;
        self.visit_expression(base);
        self.mode = current_mode;
    }

    fn visit_signal(&mut self, c: &str) {
        let myname = c.replace("$next", "");
        match self.mode {
            Mode::Ignore => {}
            Mode::Write => {
                self.local_vars_written.insert(myname);
            }
            Mode::Read => {
                if !self.local_vars_written.contains(&myname) {
                    self.violations.push(myname);
                }
            }
        }
    }

    fn visit_assignment(&mut self, l: &VerilogExpression, r: &VerilogExpression) {
        let current_mode = self.mode;
        self.mode = Mode::Read;
        self.visit_expression(r);
        self.mode = Mode::Write;
        self.visit_expression(l);
        self.mode = current_mode;
    }
}

fn get_logic_loop_candidates(uut: &dyn Block) -> Vec<String> {
    match &uut.hdl() {
        Verilog::Combinatorial(code) => {
            let mut det = VerilogLogicLoopDetector::default();
            det.visit_block(code);
            if det.violations.is_empty() {
                vec![]
            } else {
                det.violations
            }
        }
        _ => vec![],
    }
}

#[derive(Default, Clone, Debug)]
struct LocalVars {
    path: NamedPath,
    names: Vec<HashSet<String>>,
    loops: PathedNameList,
}

impl LocalVars {
    fn update_loops(&mut self, candidates: &[String]) {
        for candidate in candidates {
            if self.names.last().unwrap().contains(candidate) {
                self.loops.push(PathedName {
                    path: self.path.to_string(),
                    name: candidate.to_string(),
                })
            }
        }
    }
}

impl Probe for LocalVars {
    fn visit_start_scope(&mut self, name: &str, _node: &dyn Block) {
        self.path.push(name);
        self.names.push(Default::default());
    }

    fn visit_start_namespace(&mut self, name: &str, _node: &dyn Block) {
        self.path.push(name);
        self.names.push(Default::default());
    }

    fn visit_atom(&mut self, name: &str, signal: &dyn Atom) {
        match signal.kind() {
            AtomKind::LocalSignal | AtomKind::OutputParameter => {
                self.names.last_mut().unwrap().insert(name.to_string());
            }
            _ => {}
        }
    }

    fn visit_end_namespace(&mut self, _name: &str, _node: &dyn Block) {
        self.names.pop();
        self.path.pop();
    }

    fn visit_end_scope(&mut self, _name: &str, node: &dyn Block) {
        self.update_loops(&get_logic_loop_candidates(node));
        self.path.pop();
        self.names.pop();
    }
}

/// Check a circuit for logical loops.  Logic loops are circular
/// dependencies in the logic that are neither simulateable nor
/// synthesizable.  For example
/// ```rust
/// use rust_hdl_lib_core::prelude::*;
/// use rust_hdl_lib_core::check_logic_loops::check_logic_loops;
///
/// #[derive(LogicBlock, Default)]
/// struct Circle {
///    in1: Signal<In, Bit>,
///    loc: Signal<Local, Bit>,
///    out: Signal<Out, Bit>,
/// }
///
/// impl Logic for Circle {
///     #[hdl_gen]
///     fn update(&mut self) {
///         self.loc.next = self.out.val();
///         self.out.next = self.loc.val();  // <-- head scratcher...
///     }
/// }
///
/// let mut uut = Circle::default(); uut.connect_all();
/// assert!(check_logic_loops(&uut).is_err());
/// ```
///
pub fn check_logic_loops(uut: &dyn Block) -> Result<(), CheckError> {
    let mut visitor = LocalVars::default();
    uut.accept("uut", &mut visitor);
    if visitor.loops.is_empty() {
        Ok(())
    } else {
        Err(CheckError::LogicLoops(visitor.loops))
    }
}
