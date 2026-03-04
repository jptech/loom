use super::fileset::{AssembledConstraint, ConstraintScope};

/// Sort constraints: component-scoped before global.
/// Within each group, original insertion order is preserved.
pub fn sort_constraints(mut constraints: Vec<AssembledConstraint>) -> Vec<AssembledConstraint> {
    constraints.sort_by_key(|c| match &c.scope {
        ConstraintScope::Component { .. } => 0,
        ConstraintScope::Global => 1,
    });
    constraints
}
