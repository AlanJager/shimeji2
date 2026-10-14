//! Step and allocation budgets for one evaluation.
//!
//! A fresh [`Budget`] is created for each `eval` or variable read. Nested
//! variable reads share that budget. Exceeding either limit returns an error
//! instead of looping.

use crate::ast::Span;

/// Limits for one evaluation. Defaults are large enough for every corpus
/// expression and small enough that a runaway expression stops.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_steps: u32,
    /// Bytes of string storage an evaluation may create. Concatenation checks
    /// this before it allocates the result.
    pub max_alloc: usize,
}

impl Default for Limits {
    fn default() -> Self {
        Self {
            max_steps: 10_000,
            max_alloc: 1024 * 1024,
        }
    }
}

impl Limits {
    pub fn new(max_steps: u32, max_alloc: usize) -> Self {
        Self {
            max_steps,
            max_alloc,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BudgetKind {
    Steps,
    Alloc,
}

#[derive(Debug, Clone)]
pub(crate) struct Budget {
    steps: u32,
    alloc: usize,
    limits: Limits,
}

impl Budget {
    pub(crate) fn new(limits: Limits) -> Self {
        Self {
            steps: 0,
            alloc: 0,
            limits,
        }
    }

    pub(crate) fn step(&mut self, span: Span) -> Result<(), BudgetError> {
        self.steps = self.steps.saturating_add(1);
        if self.steps > self.limits.max_steps {
            return Err(BudgetError {
                kind: BudgetKind::Steps,
                span,
            });
        }
        Ok(())
    }

    /// Reserve `bytes` before the caller allocates them.
    pub(crate) fn reserve(&mut self, bytes: usize, span: Span) -> Result<(), BudgetError> {
        self.alloc = self.alloc.saturating_add(bytes);
        if self.alloc > self.limits.max_alloc {
            return Err(BudgetError {
                kind: BudgetKind::Alloc,
                span,
            });
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct BudgetError {
    pub(crate) kind: BudgetKind,
    pub(crate) span: Span,
}
