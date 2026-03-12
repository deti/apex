use serde::{Deserialize, Serialize};

/// A MIR function consisting of a name and a sequence of basic blocks.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MirFunction {
    pub name: String,
    pub blocks: Vec<BasicBlock>,
}

/// A basic block in MIR: an id, a list of statements, and a terminator.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BasicBlock {
    pub id: usize,
    pub statements: Vec<Statement>,
    pub terminator: Terminator,
}

/// MIR statement variants.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Statement {
    Assign { place: String, rvalue: String },
    StorageLive(String),
    StorageDead(String),
    Nop,
}

/// MIR terminator variants — each basic block ends with exactly one.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum Terminator {
    Goto {
        target: usize,
    },
    SwitchInt {
        discriminant: String,
        targets: Vec<(i128, usize)>,
        otherwise: usize,
    },
    Return,
    Unreachable,
    Call {
        func: String,
        destination: Option<usize>,
        cleanup: Option<usize>,
    },
    Drop {
        target: usize,
        unwind: Option<usize>,
    },
    Abort,
}

impl MirFunction {
    /// Create a new `MirFunction` with no blocks.
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            blocks: Vec::new(),
        }
    }

    /// Append a basic block and return its id.
    pub fn add_block(&mut self, statements: Vec<Statement>, terminator: Terminator) -> usize {
        let id = self.blocks.len();
        self.blocks.push(BasicBlock {
            id,
            statements,
            terminator,
        });
        id
    }

    /// Return successor block ids for the given block.
    pub fn successors(&self, block_id: usize) -> Vec<usize> {
        self.blocks
            .get(block_id)
            .map(|b| b.terminator.successors())
            .unwrap_or_default()
    }

    /// Number of basic blocks.
    pub fn block_count(&self) -> usize {
        self.blocks.len()
    }

    /// Total number of branch edges (sum of successor counts across all blocks).
    pub fn branch_count(&self) -> usize {
        self.blocks
            .iter()
            .map(|b| b.terminator.successors().len())
            .sum()
    }
}

impl Terminator {
    /// Return the set of successor block ids.
    pub fn successors(&self) -> Vec<usize> {
        match self {
            Terminator::Goto { target } => vec![*target],
            Terminator::SwitchInt {
                targets, otherwise, ..
            } => {
                let mut succs: Vec<usize> = targets.iter().map(|(_, t)| *t).collect();
                succs.push(*otherwise);
                succs
            }
            Terminator::Return | Terminator::Unreachable | Terminator::Abort => vec![],
            Terminator::Call {
                destination,
                cleanup,
                ..
            } => {
                let mut succs = Vec::new();
                if let Some(d) = destination {
                    succs.push(*d);
                }
                if let Some(c) = cleanup {
                    succs.push(*c);
                }
                succs
            }
            Terminator::Drop { target, unwind } => {
                let mut succs = vec![*target];
                if let Some(u) = unwind {
                    succs.push(*u);
                }
                succs
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn new_function_has_no_blocks() {
        let f = MirFunction::new("foo");
        assert_eq!(f.name, "foo");
        assert_eq!(f.block_count(), 0);
    }

    #[test]
    fn add_block_returns_sequential_ids() {
        let mut f = MirFunction::new("bar");
        let id0 = f.add_block(vec![], Terminator::Return);
        let id1 = f.add_block(vec![], Terminator::Return);
        assert_eq!(id0, 0);
        assert_eq!(id1, 1);
        assert_eq!(f.block_count(), 2);
    }

    #[test]
    fn goto_has_one_successor() {
        let t = Terminator::Goto { target: 3 };
        assert_eq!(t.successors(), vec![3]);
    }

    #[test]
    fn switch_int_successors_include_otherwise() {
        let t = Terminator::SwitchInt {
            discriminant: "_1".into(),
            targets: vec![(0, 1), (1, 2)],
            otherwise: 3,
        };
        assert_eq!(t.successors(), vec![1, 2, 3]);
    }

    #[test]
    fn return_has_no_successors() {
        assert!(Terminator::Return.successors().is_empty());
    }

    #[test]
    fn unreachable_has_no_successors() {
        assert!(Terminator::Unreachable.successors().is_empty());
    }

    #[test]
    fn abort_has_no_successors() {
        assert!(Terminator::Abort.successors().is_empty());
    }

    #[test]
    fn call_successors() {
        let t = Terminator::Call {
            func: "foo".into(),
            destination: Some(1),
            cleanup: Some(2),
        };
        assert_eq!(t.successors(), vec![1, 2]);

        let t2 = Terminator::Call {
            func: "bar".into(),
            destination: None,
            cleanup: None,
        };
        assert!(t2.successors().is_empty());
    }

    #[test]
    fn drop_successors() {
        let t = Terminator::Drop {
            target: 5,
            unwind: Some(6),
        };
        assert_eq!(t.successors(), vec![5, 6]);

        let t2 = Terminator::Drop {
            target: 5,
            unwind: None,
        };
        assert_eq!(t2.successors(), vec![5]);
    }

    #[test]
    fn branch_count_sums_all_edges() {
        let mut f = MirFunction::new("test");
        f.add_block(vec![], Terminator::Goto { target: 1 }); // 1 edge
        f.add_block(
            vec![],
            Terminator::SwitchInt {
                discriminant: "_1".into(),
                targets: vec![(0, 2)],
                otherwise: 3,
            },
        ); // 2 edges
        f.add_block(vec![], Terminator::Return); // 0 edges
        f.add_block(vec![], Terminator::Return); // 0 edges
        assert_eq!(f.branch_count(), 3);
    }

    // -----------------------------------------------------------------------
    // Additional cfg tests
    // -----------------------------------------------------------------------

    #[test]
    fn branch_count_empty_function() {
        let f = MirFunction::new("empty");
        assert_eq!(f.branch_count(), 0);
    }

    #[test]
    fn successors_out_of_bounds_returns_empty() {
        let f = MirFunction::new("empty");
        // block_id 99 does not exist -> get returns None -> unwrap_or_default = []
        assert!(f.successors(99).is_empty());
    }

    #[test]
    fn successors_valid_block_id() {
        let mut f = MirFunction::new("test");
        f.add_block(vec![], Terminator::Goto { target: 1 });
        assert_eq!(f.successors(0), vec![1]);
    }

    #[test]
    fn call_only_destination() {
        let t = Terminator::Call {
            func: "f".into(),
            destination: Some(3),
            cleanup: None,
        };
        assert_eq!(t.successors(), vec![3]);
    }

    #[test]
    fn call_only_cleanup() {
        let t = Terminator::Call {
            func: "f".into(),
            destination: None,
            cleanup: Some(7),
        };
        assert_eq!(t.successors(), vec![7]);
    }

    #[test]
    fn call_neither_destination_nor_cleanup() {
        let t = Terminator::Call {
            func: "f".into(),
            destination: None,
            cleanup: None,
        };
        assert!(t.successors().is_empty());
    }

    #[test]
    fn switch_int_empty_targets_just_otherwise() {
        let t = Terminator::SwitchInt {
            discriminant: "_x".into(),
            targets: vec![],
            otherwise: 5,
        };
        // Only the otherwise target should be included
        assert_eq!(t.successors(), vec![5]);
    }

    #[test]
    fn switch_int_single_target() {
        let t = Terminator::SwitchInt {
            discriminant: "_y".into(),
            targets: vec![(42, 2)],
            otherwise: 3,
        };
        assert_eq!(t.successors(), vec![2, 3]);
    }

    #[test]
    fn switch_int_multiple_targets() {
        let t = Terminator::SwitchInt {
            discriminant: "_z".into(),
            targets: vec![(0, 1), (1, 2), (2, 3)],
            otherwise: 4,
        };
        assert_eq!(t.successors(), vec![1, 2, 3, 4]);
    }

    #[test]
    fn drop_no_unwind() {
        let t = Terminator::Drop {
            target: 10,
            unwind: None,
        };
        assert_eq!(t.successors(), vec![10]);
    }

    #[test]
    fn drop_with_unwind() {
        let t = Terminator::Drop {
            target: 10,
            unwind: Some(11),
        };
        assert_eq!(t.successors(), vec![10, 11]);
    }

    #[test]
    fn statement_nop_in_block() {
        let mut f = MirFunction::new("nop_test");
        f.add_block(vec![Statement::Nop, Statement::Nop], Terminator::Return);
        assert_eq!(f.block_count(), 1);
        assert_eq!(f.blocks[0].statements.len(), 2);
        assert!(matches!(f.blocks[0].statements[0], Statement::Nop));
    }

    #[test]
    fn statement_assign_fields() {
        let s = Statement::Assign {
            place: "_0".into(),
            rvalue: "const 42".into(),
        };
        match s {
            Statement::Assign { place, rvalue } => {
                assert_eq!(place, "_0");
                assert_eq!(rvalue, "const 42");
            }
            _ => panic!("expected Assign"),
        }
    }

    #[test]
    fn statement_storage_live_dead_fields() {
        let live = Statement::StorageLive("_5".into());
        let dead = Statement::StorageDead("_5".into());
        assert!(matches!(live, Statement::StorageLive(ref v) if v == "_5"));
        assert!(matches!(dead, Statement::StorageDead(ref v) if v == "_5"));
    }

    #[test]
    fn mir_function_add_block_increments_id() {
        let mut f = MirFunction::new("sequential");
        for i in 0..5 {
            let id = f.add_block(vec![], Terminator::Return);
            assert_eq!(id, i);
        }
        assert_eq!(f.block_count(), 5);
    }

    #[test]
    fn branch_count_with_call_terminators() {
        let mut f = MirFunction::new("calls");
        // Call with both destination and cleanup = 2 edges
        f.add_block(vec![], Terminator::Call {
            func: "foo".into(),
            destination: Some(1),
            cleanup: Some(2),
        });
        // Call with only destination = 1 edge
        f.add_block(vec![], Terminator::Call {
            func: "bar".into(),
            destination: Some(3),
            cleanup: None,
        });
        // Call with neither = 0 edges
        f.add_block(vec![], Terminator::Call {
            func: "baz".into(),
            destination: None,
            cleanup: None,
        });
        assert_eq!(f.branch_count(), 3);
    }

    #[test]
    fn branch_count_with_drop_terminators() {
        let mut f = MirFunction::new("drops");
        f.add_block(vec![], Terminator::Drop { target: 1, unwind: Some(2) }); // 2
        f.add_block(vec![], Terminator::Drop { target: 3, unwind: None }); // 1
        assert_eq!(f.branch_count(), 3);
    }

    #[test]
    fn mir_function_name_stored_correctly() {
        let f = MirFunction::new("my_module::my_fn");
        assert_eq!(f.name, "my_module::my_fn");
    }

    #[test]
    fn switch_int_successors_order_is_targets_then_otherwise() {
        // Verify targets come before otherwise in the returned slice
        let t = Terminator::SwitchInt {
            discriminant: "_d".into(),
            targets: vec![(10, 100), (20, 200)],
            otherwise: 999,
        };
        let succs = t.successors();
        assert_eq!(&succs[..2], &[100, 200]);
        assert_eq!(succs[2], 999);
    }

    #[test]
    fn complex_function_block_count_and_successors() {
        // Build a complex function and verify block count and successor chains
        let mut f = MirFunction::new("round_trip");
        f.add_block(
            vec![
                Statement::StorageLive("_1".into()),
                Statement::Assign { place: "_0".into(), rvalue: "_1".into() },
                Statement::StorageDead("_1".into()),
                Statement::Nop,
            ],
            Terminator::Goto { target: 1 },
        );
        f.add_block(vec![], Terminator::SwitchInt {
            discriminant: "_0".into(),
            targets: vec![(0, 2), (1, 3)],
            otherwise: 4,
        });
        f.add_block(vec![], Terminator::Return);
        f.add_block(vec![], Terminator::Unreachable);
        f.add_block(vec![], Terminator::Abort);

        assert_eq!(f.name, "round_trip");
        assert_eq!(f.block_count(), 5);
        assert_eq!(f.blocks[0].statements.len(), 4);
        assert_eq!(f.successors(1), vec![2, 3, 4]);
        assert!(f.successors(2).is_empty()); // Return
        assert!(f.successors(3).is_empty()); // Unreachable
        assert!(f.successors(4).is_empty()); // Abort
    }
}
