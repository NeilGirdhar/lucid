//! Validated, backend-independent control-flow IR for Lucid.

use std::fmt::Write;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct ValueId(pub u32);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct BlockId(pub u32);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Instruction {
    /// Read a positional function argument. The function signature owns the
    /// argument count; CIR only records the stable zero-based index.
    Param {
        result: ValueId,
        index: u32,
    },
    ConstInt {
        result: ValueId,
        value: i64,
    },
    ConstBool {
        result: ValueId,
        value: bool,
    },
    Add {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Sub {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Mul {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Pow {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Div {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    FloorDiv {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Mod {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    BitAnd {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    BitOr {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    BitXor {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Shl {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Shr {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    CmpEq {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    CmpNe {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    CmpLe {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    CmpGt {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    CmpGe {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    CmpLt {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Neg {
        result: ValueId,
        operand: ValueId,
    },
    BitNot {
        result: ValueId,
        operand: ValueId,
    },
    Not {
        result: ValueId,
        operand: ValueId,
    },
    And {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Or {
        result: ValueId,
        left: ValueId,
        right: ValueId,
    },
    Phi {
        result: ValueId,
        incomings: Vec<(BlockId, ValueId)>,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Terminator {
    Jump(BlockId),
    Branch {
        condition: ValueId,
        then_block: BlockId,
        else_block: BlockId,
    },
    Return(Option<ValueId>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: BlockId,
    pub instructions: Vec<Instruction>,
    pub terminator: Terminator,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Function {
    pub entry: BlockId,
    pub blocks: Vec<Block>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum VerifyError {
    EmptyFunction,
    MissingEntry(BlockId),
    DuplicateBlock(BlockId),
    MissingBlock(BlockId),
    UndefinedValue(ValueId),
    DuplicateValue(ValueId),
    UseBeforeDefinition(ValueId),
    UseNotDominated {
        value: ValueId,
        block: BlockId,
    },
    InvalidPhiIncoming {
        block: BlockId,
        predecessor: BlockId,
    },
    DuplicatePhiIncoming {
        block: BlockId,
        predecessor: BlockId,
    },
    MissingPhiIncoming {
        block: BlockId,
        predecessor: BlockId,
    },
    PhiAfterNonPhi {
        block: BlockId,
    },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExecuteError {
    Invalid(VerifyError),
    StepLimitExceeded,
    MissingArgument(u32),
    UnexpectedArgumentCount { expected: usize, provided: usize },
    DivisionByZero,
    ArithmeticOverflow,
    NegativeExponent,
    UnsupportedInstruction,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LowerError {
    UnsupportedExpression,
    NoLowerableAssignment,
}

/// The parser-independent input accepted by the typed-HIR lowering seam.
/// Nodes are post-order, so a child ID must name an earlier node.  The
/// database owns conversion from its interned HIR representation to these
/// small owned records; CIR never needs to depend on the compiler database.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TypedExprNode {
    pub id: u32,
    pub kind: String,
    pub detail: Option<String>,
    pub children: Vec<u32>,
    pub literal: Option<TypedLiteral>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TypedLiteral {
    Int(i64),
    Bool(bool),
}

/// Evaluate a constant `range` call when its expansion is small enough to
/// unroll safely. `None` means the call is dynamic, invalid, or too large.
pub fn const_range_values(expr: &lucid_syntax::Expr) -> Option<Vec<i64>> {
    fn constant_int(expr: &lucid_syntax::Expr) -> Option<i64> {
        match expr {
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Int(value),
                ..
            } => Some(*value),
            lucid_syntax::Expr::Unary {
                op: lucid_syntax::UnaryOp::Neg,
                expr,
                ..
            } => constant_int(expr)?.checked_neg(),
            lucid_syntax::Expr::Unary {
                op: lucid_syntax::UnaryOp::Pos,
                expr,
                ..
            } => constant_int(expr),
            lucid_syntax::Expr::Unary {
                op: lucid_syntax::UnaryOp::Invert,
                expr,
                ..
            } => Some(!constant_int(expr)?),
            lucid_syntax::Expr::Binary {
                op, left, right, ..
            } => {
                let left = constant_int(left)?;
                let right = constant_int(right)?;
                match op {
                    lucid_syntax::BinaryOp::Add => left.checked_add(right),
                    lucid_syntax::BinaryOp::Sub => left.checked_sub(right),
                    lucid_syntax::BinaryOp::Mul => left.checked_mul(right),
                    lucid_syntax::BinaryOp::Pow => u32::try_from(right)
                        .ok()
                        .and_then(|exponent| left.checked_pow(exponent)),
                    lucid_syntax::BinaryOp::Div => left.checked_div(right),
                    lucid_syntax::BinaryOp::FloorDiv => {
                        let quotient = left.checked_div(right)?;
                        let remainder = left.checked_rem(right)?;
                        if remainder != 0 && (left < 0) != (right < 0) {
                            quotient.checked_sub(1)
                        } else {
                            Some(quotient)
                        }
                    }
                    lucid_syntax::BinaryOp::Mod => {
                        let remainder = left.checked_rem(right)?;
                        if remainder != 0 && (left < 0) != (right < 0) {
                            remainder.checked_add(right)
                        } else {
                            Some(remainder)
                        }
                    }
                    lucid_syntax::BinaryOp::BitAnd => Some(left & right),
                    lucid_syntax::BinaryOp::BitOr => Some(left | right),
                    lucid_syntax::BinaryOp::BitXor => Some(left ^ right),
                    lucid_syntax::BinaryOp::Shl => left.checked_shl(u32::try_from(right).ok()?),
                    lucid_syntax::BinaryOp::Shr => left.checked_shr(u32::try_from(right).ok()?),
                    _ => None,
                }
            }
            _ => None,
        }
    }
    let lucid_syntax::Expr::Call { func, args, .. } = expr else {
        return None;
    };
    if !matches!(func.as_ref(), lucid_syntax::Expr::Ident { name, .. } if name == "range")
        || args.iter().any(|arg| {
            arg.name.is_some() || arg.is_spread || arg.is_dict_spread || arg.is_gather_spread
        })
    {
        return None;
    }
    let values = args
        .iter()
        .map(|arg| constant_int(&arg.value))
        .collect::<Option<Vec<_>>>()?;
    let (mut current, stop, step) = match values.as_slice() {
        [stop] => (0, *stop, 1),
        [start, stop] => (*start, *stop, 1),
        [start, stop, step] if *step != 0 => (*start, *stop, *step),
        _ => return None,
    };
    let empty = (step > 0 && current >= stop) || (step < 0 && current <= stop);
    if empty {
        return Some(Vec::new());
    }
    let mut result = Vec::new();
    while (step > 0 && current < stop) || (step < 0 && current > stop) {
        if result.len() >= 1024 {
            return None;
        }
        result.push(current);
        current = current.checked_add(step)?;
    }
    Some(result)
}

/// Return whether an expression is a compile-time-known empty iterable.
/// Only literal collections/strings and integer-only `range` calls qualify;
/// all other expressions remain dynamic and must be evaluated normally.
pub fn is_const_empty_iterable(expr: &lucid_syntax::Expr) -> bool {
    match expr {
        lucid_syntax::Expr::List { elements, .. } | lucid_syntax::Expr::Set { elements, .. } => {
            elements.is_empty()
        }
        lucid_syntax::Expr::Dict { entries, .. } => entries.is_empty(),
        lucid_syntax::Expr::Literal {
            value: lucid_syntax::LiteralValue::Str(value),
            ..
        } => value.is_empty(),
        lucid_syntax::Expr::Call { .. } => {
            const_range_values(expr).is_some_and(|values| values.is_empty())
        }
        _ => false,
    }
}

impl Function {
    /// Return whether this function contains an operation whose failure must
    /// cross the shared recoverable-result ABI. Keeping this knowledge in CIR
    /// prevents front ends from accidentally selecting a trapping backend.
    pub fn has_recoverable_operations(&self) -> bool {
        self.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Add { .. }
                        | Instruction::Sub { .. }
                        | Instruction::Mul { .. }
                        | Instruction::Pow { .. }
                        | Instruction::Neg { .. }
                        | Instruction::Shl { .. }
                        | Instruction::Shr { .. }
                        | Instruction::Div { .. }
                        | Instruction::FloorDiv { .. }
                        | Instruction::Mod { .. }
                )
            })
        })
    }

    /// Return whether this function contains division-family operations whose
    /// status propagation has stricter CFG requirements in the native result
    /// backend.
    pub fn has_division_operations(&self) -> bool {
        self.blocks.iter().any(|block| {
            block.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Div { .. }
                        | Instruction::FloorDiv { .. }
                        | Instruction::Mod { .. }
                )
            })
        })
    }

    /// Lower the integer/boolean expression subset directly from typed HIR.
    /// This is deliberately separate from AST lowering: the checker has
    /// already established the expression graph and CIR consumes that graph
    /// without reparsing or re-traversing syntax trees.
    pub fn from_typed_initializers(
        nodes: &[TypedExprNode],
        roots: &[u32],
    ) -> Result<Self, LowerError> {
        Self::from_typed_graph(nodes, roots, &[], &[])
    }

    /// Lower one expression-bearing function body from typed HIR. Names that
    /// match `parameter_names` become explicit CIR parameter reads; other
    /// names remain unsupported rather than being resolved from syntax.
    pub fn from_typed_function_body(
        nodes: &[TypedExprNode],
        root: u32,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        Self::from_typed_function_body_with_locals(nodes, root, parameter_names, &[])
    }

    /// Lower a function body whose local bindings have already been resolved
    /// to the typed-HIR node that computes each value.  The bindings are
    /// immutable SSA aliases; repeated assignment is represented by a later
    /// alias for the same source name.
    pub fn from_typed_function_body_with_locals(
        nodes: &[TypedExprNode],
        root: u32,
        parameter_names: &[String],
        local_bindings: &[(String, u32)],
    ) -> Result<Self, LowerError> {
        Self::from_typed_graph(nodes, &[root], parameter_names, local_bindings)
    }

    fn from_typed_graph(
        nodes: &[TypedExprNode],
        roots: &[u32],
        parameter_names: &[String],
        local_bindings: &[(String, u32)],
    ) -> Result<Self, LowerError> {
        if roots.is_empty() {
            return Err(LowerError::NoLowerableAssignment);
        }
        // A typed conditional must remain a control-flow diamond.  Lowering
        // both arms eagerly into one block would execute the unchosen arm and
        // erase the cleanup/error boundary that typed HIR established.
        if roots.len() == 1 {
            if let Some(node) = nodes.get(roots[0] as usize) {
                // Fold a condition that is provably constant before lowering
                // either arm. This is important for recoverable operations:
                // an unchosen branch must not execute (or report overflow)
                // merely because it appears in the typed graph.
                if node.kind == "if" && node.children.len() == 3 {
                    fn invalid_constant(nodes: &[TypedExprNode], id: u32) -> bool {
                        let Some(node) = nodes.get(id as usize) else {
                            return false;
                        };
                        if node
                            .children
                            .iter()
                            .any(|child| invalid_constant(nodes, *child))
                        {
                            return true;
                        }
                        if node.kind != "binary" || node.children.len() != 2 {
                            return false;
                        }
                        let right_zero = matches!(
                            nodes
                                .get(node.children[1] as usize)
                                .and_then(|node| node.literal),
                            Some(TypedLiteral::Int(0))
                        );
                        matches!(node.detail.as_deref(), Some("Div" | "FloorDiv" | "Mod"))
                            && right_zero
                    }
                    if invalid_constant(nodes, node.children[0]) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    fn constant_bool(nodes: &[TypedExprNode], id: u32) -> Option<bool> {
                        let node = nodes.get(id as usize)?;
                        if let Some(literal) = node.literal {
                            return match literal {
                                TypedLiteral::Bool(value) => Some(value),
                                TypedLiteral::Int(value) => Some(value != 0),
                            };
                        }
                        if node.kind != "binary" || node.children.len() != 2 {
                            return None;
                        }
                        // A power may exceed i64 even though its comparison
                        // result is decidable (for example `2 ** 63 == 0`).
                        // Keep this small proof separate from value folding
                        // so the dead branch remains unlowered.
                        if matches!(
                            node.detail.as_deref(),
                            Some("Eq" | "Identity" | "Is" | "NotEq" | "NotIdentity" | "IsNot")
                        ) {
                            let is_zero = |id| {
                                matches!(
                                    nodes.get(id as usize).and_then(|node| node.literal),
                                    Some(TypedLiteral::Int(0))
                                )
                            };
                            let definitely_nonzero = |id| {
                                let node = nodes.get(id as usize)?;
                                if let Some(TypedLiteral::Int(value)) = node.literal {
                                    return Some(value != 0);
                                }
                                if node.kind == "binary"
                                    && node.detail.as_deref() == Some("Pow")
                                    && node.children.len() == 2
                                {
                                    let base = constant_int(nodes, node.children[0])?;
                                    let exponent = constant_int(nodes, node.children[1])?;
                                    return Some(exponent > 0 && base.abs() > 1);
                                }
                                None
                            };
                            if is_zero(node.children[1])
                                && definitely_nonzero(node.children[0]) == Some(true)
                            {
                                return Some(matches!(
                                    node.detail.as_deref(),
                                    Some("NotEq" | "NotIdentity" | "IsNot")
                                ));
                            }
                            if is_zero(node.children[0])
                                && definitely_nonzero(node.children[1]) == Some(true)
                            {
                                return Some(matches!(
                                    node.detail.as_deref(),
                                    Some("NotEq" | "NotIdentity" | "IsNot")
                                ));
                            }
                        }
                        let left = constant_int(nodes, node.children[0])?;
                        let right = constant_int(nodes, node.children[1])?;
                        match node.detail.as_deref()? {
                            "Eq" | "Identity" | "Is" => Some(left == right),
                            "NotEq" | "NotIdentity" | "IsNot" => Some(left != right),
                            "Lt" => Some(left < right),
                            "LtEq" => Some(left <= right),
                            "Gt" => Some(left > right),
                            "GtEq" => Some(left >= right),
                            _ => None,
                        }
                    }
                    fn constant_int(nodes: &[TypedExprNode], id: u32) -> Option<i64> {
                        let node = nodes.get(id as usize)?;
                        if let Some(TypedLiteral::Int(value)) = node.literal {
                            return Some(value);
                        }
                        if node.kind == "unary" && node.children.len() == 1 {
                            let value = constant_int(nodes, node.children[0])?;
                            return match node.detail.as_deref()? {
                                "Pos" => Some(value),
                                "Neg" => value.checked_neg(),
                                "Invert" => Some(!value),
                                _ => None,
                            };
                        }
                        if node.kind == "binary" && node.children.len() == 2 {
                            let left = constant_int(nodes, node.children[0])?;
                            let right = constant_int(nodes, node.children[1])?;
                            return match node.detail.as_deref()? {
                                "Add" => left.checked_add(right),
                                "Sub" => left.checked_sub(right),
                                "Mul" => left.checked_mul(right),
                                "Pow" => {
                                    u32::try_from(right).ok().and_then(|e| left.checked_pow(e))
                                }
                                "BitAnd" => Some(left & right),
                                "BitOr" => Some(left | right),
                                "BitXor" => Some(left ^ right),
                                "Shl" => left.checked_shl(u32::try_from(right).ok()?),
                                "Shr" => left.checked_shr(u32::try_from(right).ok()?),
                                _ => None,
                            };
                        }
                        None
                    }
                    if let Some(value) = constant_bool(nodes, node.children[0]) {
                        return Self::from_typed_graph(
                            nodes,
                            &[if value {
                                node.children[1]
                            } else {
                                node.children[2]
                            }],
                            parameter_names,
                            local_bindings,
                        );
                    }
                }
                if node.kind == "binary"
                    && matches!(node.detail.as_deref(), Some("And") | Some("Or"))
                    && node.children.len() == 2
                {
                    if let Some(TypedLiteral::Bool(left)) = nodes
                        .get(node.children[0] as usize)
                        .and_then(|child| child.literal)
                    {
                        let selects_left = (node.detail.as_deref() == Some("And") && !left)
                            || (node.detail.as_deref() == Some("Or") && left);
                        let selected = if selects_left {
                            node.children[0]
                        } else {
                            node.children[1]
                        };
                        return Self::from_typed_graph(
                            nodes,
                            &[selected],
                            parameter_names,
                            local_bindings,
                        );
                    }
                    // Encode short-circuiting as a synthetic typed `if`:
                    // `left and right` selects `right` or `false`, while
                    // `left or right` selects `true` or `right`.  The
                    // synthetic nodes stay in this lowering call only; the
                    // source HIR remains untouched and both branches still
                    // lower through the ordinary typed-CIR path.
                    let mut expanded = nodes.to_vec();
                    let literal_id = u32::try_from(expanded.len())
                        .map_err(|_| LowerError::UnsupportedExpression)?;
                    expanded.push(TypedExprNode {
                        id: literal_id,
                        kind: "literal".into(),
                        detail: None,
                        children: Vec::new(),
                        literal: Some(TypedLiteral::Bool(node.detail.as_deref() == Some("Or"))),
                    });
                    let if_id = u32::try_from(expanded.len())
                        .map_err(|_| LowerError::UnsupportedExpression)?;
                    let (then_root, else_root) = if node.detail.as_deref() == Some("And") {
                        (node.children[1], literal_id)
                    } else {
                        (literal_id, node.children[1])
                    };
                    expanded.push(TypedExprNode {
                        id: if_id,
                        kind: "if".into(),
                        detail: None,
                        children: vec![node.children[0], then_root, else_root],
                        literal: None,
                    });
                    return Self::from_typed_graph(
                        &expanded,
                        &[if_id],
                        parameter_names,
                        local_bindings,
                    );
                }
                if node.kind == "match" && node.children.len() == 3 {
                    let literal = node.detail.as_deref().and_then(|detail| {
                        detail
                            .strip_prefix("literal-int:")
                            .and_then(|value| value.parse::<i64>().ok())
                            .map(TypedLiteral::Int)
                            .or_else(|| {
                                detail
                                    .strip_prefix("literal-bool:")
                                    .and_then(|value| value.parse::<bool>().ok())
                                    .map(TypedLiteral::Bool)
                            })
                    });
                    let Some(literal) = literal else {
                        return Err(LowerError::UnsupportedExpression);
                    };
                    let mut expanded = nodes.to_vec();
                    let literal_id = u32::try_from(expanded.len())
                        .map_err(|_| LowerError::UnsupportedExpression)?;
                    expanded.push(TypedExprNode {
                        id: literal_id,
                        kind: "literal".into(),
                        detail: None,
                        children: Vec::new(),
                        literal: Some(literal),
                    });
                    let comparison_id = u32::try_from(expanded.len())
                        .map_err(|_| LowerError::UnsupportedExpression)?;
                    expanded.push(TypedExprNode {
                        id: comparison_id,
                        kind: "binary".into(),
                        detail: Some("Eq".into()),
                        children: vec![node.children[0], literal_id],
                        literal: None,
                    });
                    let if_id = u32::try_from(expanded.len())
                        .map_err(|_| LowerError::UnsupportedExpression)?;
                    expanded.push(TypedExprNode {
                        id: if_id,
                        kind: "if".into(),
                        detail: None,
                        children: vec![comparison_id, node.children[1], node.children[2]],
                        literal: None,
                    });
                    return Self::from_typed_graph(
                        &expanded,
                        &[if_id],
                        parameter_names,
                        local_bindings,
                    );
                }
                if matches!(
                    node.kind.as_str(),
                    "match-chain" | "optional-match-chain" | "void-match-chain"
                ) && !node.children.is_empty()
                {
                    let optional_chain = node.kind == "optional-match-chain";
                    let void_chain = node.kind == "void-match-chain";
                    let Some(detail) = node.detail.as_deref() else {
                        return Err(LowerError::UnsupportedExpression);
                    };
                    let Some(patterns) = detail.strip_prefix("literal-chain:") else {
                        return Err(LowerError::UnsupportedExpression);
                    };
                    let patterns = patterns
                        .split(',')
                        .map(|value| {
                            if let Some(value) = value.strip_prefix('i') {
                                value.parse::<i64>().ok().map(TypedLiteral::Int)
                            } else {
                                value
                                    .strip_prefix('b')
                                    .and_then(|value| value.parse::<bool>().ok())
                                    .map(TypedLiteral::Bool)
                            }
                        })
                        .collect::<Option<Vec<_>>>();
                    let Some(patterns) = patterns else {
                        return Err(LowerError::UnsupportedExpression);
                    };
                    let expected_children = if void_chain {
                        1
                    } else {
                        patterns.len() + if optional_chain { 1 } else { 2 }
                    };
                    if expected_children != node.children.len() {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    fn remap_instruction(instruction: &Instruction, offset: u32) -> Instruction {
                        let value = |value: ValueId| ValueId(value.0 + offset);
                        match instruction {
                            Instruction::Param { result, index } => Instruction::Param {
                                result: value(*result),
                                index: *index,
                            },
                            Instruction::ConstInt {
                                result,
                                value: literal,
                            } => Instruction::ConstInt {
                                result: value(*result),
                                value: *literal,
                            },
                            Instruction::ConstBool {
                                result,
                                value: literal,
                            } => Instruction::ConstBool {
                                result: value(*result),
                                value: *literal,
                            },
                            Instruction::Add {
                                result,
                                left,
                                right,
                            } => Instruction::Add {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Sub {
                                result,
                                left,
                                right,
                            } => Instruction::Sub {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Mul {
                                result,
                                left,
                                right,
                            } => Instruction::Mul {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Pow {
                                result,
                                left,
                                right,
                            } => Instruction::Pow {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Div {
                                result,
                                left,
                                right,
                            } => Instruction::Div {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::FloorDiv {
                                result,
                                left,
                                right,
                            } => Instruction::FloorDiv {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Mod {
                                result,
                                left,
                                right,
                            } => Instruction::Mod {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::BitAnd {
                                result,
                                left,
                                right,
                            } => Instruction::BitAnd {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::BitOr {
                                result,
                                left,
                                right,
                            } => Instruction::BitOr {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::BitXor {
                                result,
                                left,
                                right,
                            } => Instruction::BitXor {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Shl {
                                result,
                                left,
                                right,
                            } => Instruction::Shl {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Shr {
                                result,
                                left,
                                right,
                            } => Instruction::Shr {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::CmpEq {
                                result,
                                left,
                                right,
                            } => Instruction::CmpEq {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::CmpNe {
                                result,
                                left,
                                right,
                            } => Instruction::CmpNe {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::CmpLe {
                                result,
                                left,
                                right,
                            } => Instruction::CmpLe {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::CmpGt {
                                result,
                                left,
                                right,
                            } => Instruction::CmpGt {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::CmpGe {
                                result,
                                left,
                                right,
                            } => Instruction::CmpGe {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::CmpLt {
                                result,
                                left,
                                right,
                            } => Instruction::CmpLt {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Neg { result, operand } => Instruction::Neg {
                                result: value(*result),
                                operand: value(*operand),
                            },
                            Instruction::BitNot { result, operand } => Instruction::BitNot {
                                result: value(*result),
                                operand: value(*operand),
                            },
                            Instruction::Not { result, operand } => Instruction::Not {
                                result: value(*result),
                                operand: value(*operand),
                            },
                            Instruction::And {
                                result,
                                left,
                                right,
                            } => Instruction::And {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Or {
                                result,
                                left,
                                right,
                            } => Instruction::Or {
                                result: value(*result),
                                left: value(*left),
                                right: value(*right),
                            },
                            Instruction::Phi { result, incomings } => Instruction::Phi {
                                result: value(*result),
                                incomings: incomings
                                    .iter()
                                    .map(|(block, incoming)| (*block, value(*incoming)))
                                    .collect(),
                            },
                        }
                    }
                    fn result_id(instruction: &Instruction) -> ValueId {
                        match instruction {
                            Instruction::Param { result, .. }
                            | Instruction::ConstInt { result, .. }
                            | Instruction::ConstBool { result, .. }
                            | Instruction::Add { result, .. }
                            | Instruction::Sub { result, .. }
                            | Instruction::Mul { result, .. }
                            | Instruction::Pow { result, .. }
                            | Instruction::Div { result, .. }
                            | Instruction::FloorDiv { result, .. }
                            | Instruction::Mod { result, .. }
                            | Instruction::BitAnd { result, .. }
                            | Instruction::BitOr { result, .. }
                            | Instruction::BitXor { result, .. }
                            | Instruction::Shl { result, .. }
                            | Instruction::Shr { result, .. }
                            | Instruction::CmpEq { result, .. }
                            | Instruction::CmpNe { result, .. }
                            | Instruction::CmpLe { result, .. }
                            | Instruction::CmpGt { result, .. }
                            | Instruction::CmpGe { result, .. }
                            | Instruction::CmpLt { result, .. }
                            | Instruction::Neg { result, .. }
                            | Instruction::BitNot { result, .. }
                            | Instruction::Not { result, .. }
                            | Instruction::And { result, .. }
                            | Instruction::Or { result, .. }
                            | Instruction::Phi { result, .. } => *result,
                        }
                    }
                    fn single_block(
                        function: Function,
                    ) -> Result<(Vec<Instruction>, ValueId), LowerError> {
                        let [block] = function.blocks.as_slice() else {
                            return Err(LowerError::UnsupportedExpression);
                        };
                        let Terminator::Return(Some(value)) = block.terminator else {
                            return Err(LowerError::UnsupportedExpression);
                        };
                        Ok((block.instructions.clone(), value))
                    }
                    fn remap_block(
                        instructions: Vec<Instruction>,
                        value: ValueId,
                        offset: &mut u32,
                    ) -> (Vec<Instruction>, ValueId) {
                        let current = *offset;
                        let max_value = instructions
                            .iter()
                            .map(result_id)
                            .chain(std::iter::once(value))
                            .map(|value| value.0 + 1)
                            .max()
                            .unwrap_or(0);
                        *offset += max_value;
                        (
                            instructions
                                .iter()
                                .map(|instruction| remap_instruction(instruction, current))
                                .collect(),
                            ValueId(value.0 + current),
                        )
                    }

                    let mut conditions = Vec::with_capacity(patterns.len());
                    for pattern in &patterns {
                        let mut expanded = nodes.to_vec();
                        let literal_id = u32::try_from(expanded.len())
                            .map_err(|_| LowerError::UnsupportedExpression)?;
                        expanded.push(TypedExprNode {
                            id: literal_id,
                            kind: "literal".into(),
                            detail: None,
                            children: Vec::new(),
                            literal: Some(*pattern),
                        });
                        let comparison_id = u32::try_from(expanded.len())
                            .map_err(|_| LowerError::UnsupportedExpression)?;
                        expanded.push(TypedExprNode {
                            id: comparison_id,
                            kind: "binary".into(),
                            detail: Some("Eq".into()),
                            children: vec![node.children[0], literal_id],
                            literal: None,
                        });
                        conditions.push(single_block(Self::from_typed_graph(
                            &expanded,
                            &[comparison_id],
                            parameter_names,
                            local_bindings,
                        )?)?);
                    }
                    if void_chain {
                        let mut next_value_offset = 0;
                        let mut blocks = Vec::with_capacity(patterns.len() * 2 + 1);
                        for (index, (instructions, condition_value)) in
                            conditions.into_iter().enumerate()
                        {
                            let condition_block = BlockId(
                                u32::try_from(index * 2)
                                    .map_err(|_| LowerError::UnsupportedExpression)?,
                            );
                            let result_block = BlockId(
                                u32::try_from(index * 2 + 1)
                                    .map_err(|_| LowerError::UnsupportedExpression)?,
                            );
                            let else_block = if index + 1 == patterns.len() {
                                BlockId(
                                    u32::try_from(patterns.len() * 2)
                                        .map_err(|_| LowerError::UnsupportedExpression)?,
                                )
                            } else {
                                BlockId(
                                    u32::try_from((index + 1) * 2)
                                        .map_err(|_| LowerError::UnsupportedExpression)?,
                                )
                            };
                            let (instructions, condition_value) =
                                remap_block(instructions, condition_value, &mut next_value_offset);
                            blocks.push(Block {
                                id: condition_block,
                                instructions,
                                terminator: Terminator::Branch {
                                    condition: condition_value,
                                    then_block: result_block,
                                    else_block,
                                },
                            });
                            blocks.push(Block {
                                id: result_block,
                                instructions: Vec::new(),
                                terminator: Terminator::Return(None),
                            });
                        }
                        blocks.push(Block {
                            id: BlockId(
                                u32::try_from(patterns.len() * 2)
                                    .map_err(|_| LowerError::UnsupportedExpression)?,
                            ),
                            instructions: Vec::new(),
                            terminator: Terminator::Return(None),
                        });
                        let function = Self {
                            entry: BlockId(0),
                            blocks,
                        };
                        function
                            .verify()
                            .map_err(|_| LowerError::UnsupportedExpression)?;
                        return Ok(function);
                    }
                    let mut results = Vec::with_capacity(node.children.len() - 1);
                    for result in &node.children[1..] {
                        results.push(single_block(Self::from_typed_graph(
                            nodes,
                            &[*result],
                            parameter_names,
                            local_bindings,
                        )?)?);
                    }
                    let mut next_value_offset = 0;
                    let mut blocks =
                        Vec::with_capacity(patterns.len() * 2 + if optional_chain { 1 } else { 2 });
                    let merge_block = BlockId(
                        u32::try_from(patterns.len() * 2 + 1)
                            .map_err(|_| LowerError::UnsupportedExpression)?,
                    );
                    let mut phi_incomings = Vec::with_capacity(results.len());
                    for (index, (instructions, condition_value)) in
                        conditions.into_iter().enumerate()
                    {
                        let condition_block = BlockId(
                            u32::try_from(index * 2)
                                .map_err(|_| LowerError::UnsupportedExpression)?,
                        );
                        let result_block = BlockId(
                            u32::try_from(index * 2 + 1)
                                .map_err(|_| LowerError::UnsupportedExpression)?,
                        );
                        let else_block = if index + 1 == patterns.len() {
                            BlockId(
                                u32::try_from(patterns.len() * 2)
                                    .map_err(|_| LowerError::UnsupportedExpression)?,
                            )
                        } else {
                            BlockId(
                                u32::try_from((index + 1) * 2)
                                    .map_err(|_| LowerError::UnsupportedExpression)?,
                            )
                        };
                        let (instructions, condition_value) =
                            remap_block(instructions, condition_value, &mut next_value_offset);
                        blocks.push(Block {
                            id: condition_block,
                            instructions,
                            terminator: Terminator::Branch {
                                condition: condition_value,
                                then_block: result_block,
                                else_block,
                            },
                        });
                        let (result_instructions, result_value) = remap_block(
                            results[index].0.clone(),
                            results[index].1,
                            &mut next_value_offset,
                        );
                        phi_incomings.push((result_block, result_value));
                        let result_terminator = if optional_chain {
                            Terminator::Return(Some(result_value))
                        } else {
                            Terminator::Jump(merge_block)
                        };
                        blocks.push(Block {
                            id: result_block,
                            instructions: result_instructions,
                            terminator: result_terminator,
                        });
                    }
                    let fallback_block = BlockId(
                        u32::try_from(patterns.len() * 2)
                            .map_err(|_| LowerError::UnsupportedExpression)?,
                    );
                    if optional_chain {
                        blocks.push(Block {
                            id: fallback_block,
                            instructions: Vec::new(),
                            terminator: Terminator::Return(None),
                        });
                        let function = Self {
                            entry: BlockId(0),
                            blocks,
                        };
                        function
                            .verify()
                            .map_err(|_| LowerError::UnsupportedExpression)?;
                        return Ok(function);
                    }
                    let fallback = results.last().ok_or(LowerError::UnsupportedExpression)?;
                    let (fallback_instructions, fallback_value) =
                        remap_block(fallback.0.clone(), fallback.1, &mut next_value_offset);
                    phi_incomings.push((fallback_block, fallback_value));
                    blocks.push(Block {
                        id: fallback_block,
                        instructions: fallback_instructions,
                        terminator: Terminator::Jump(merge_block),
                    });
                    let merge_value = ValueId(next_value_offset);
                    blocks.push(Block {
                        id: merge_block,
                        instructions: vec![Instruction::Phi {
                            result: merge_value,
                            incomings: phi_incomings,
                        }],
                        terminator: Terminator::Return(Some(merge_value)),
                    });
                    let function = Self {
                        entry: BlockId(0),
                        blocks,
                    };
                    function
                        .verify()
                        .map_err(|_| LowerError::UnsupportedExpression)?;
                    return Ok(function);
                }
                if node.id == roots[0]
                    && node.kind == "if"
                    && node.children.len() == 3
                    && !matches!(
                        nodes
                            .get(node.children[0] as usize)
                            .and_then(|condition| condition.literal),
                        Some(TypedLiteral::Bool(_))
                    )
                {
                    return Self::from_typed_dynamic_if(
                        nodes,
                        node.children[0],
                        node.children[1],
                        node.children[2],
                        parameter_names,
                        local_bindings,
                    );
                }
            }
        }
        let mut instructions = Vec::new();
        let mut lowered = std::collections::HashMap::<u32, ValueId>::new();
        fn lower(
            id: u32,
            nodes: &[TypedExprNode],
            lowered: &mut std::collections::HashMap<u32, ValueId>,
            instructions: &mut Vec<Instruction>,
            next: &mut u32,
            parameter_names: &[String],
            local_bindings: &[(String, u32)],
        ) -> Result<ValueId, LowerError> {
            if let Some(value) = lowered.get(&id) {
                return Ok(*value);
            }
            let node = nodes
                .get(id as usize)
                .ok_or(LowerError::UnsupportedExpression)?;
            if node.id != id || node.children.iter().any(|child| *child >= id) {
                return Err(LowerError::UnsupportedExpression);
            }
            let provisional_result = ValueId(*next);
            *next += 1;
            let mut result = provisional_result;
            if node.kind == "name"
                && !parameter_names
                    .iter()
                    .any(|name| node.detail.as_deref() == Some(name.as_str()))
            {
                if let Some((_, binding_id)) =
                    local_bindings.iter().rev().find(|(name, binding_id)| {
                        node.detail.as_deref() == Some(name.as_str()) && *binding_id < id
                    })
                {
                    result = lower(
                        *binding_id,
                        nodes,
                        lowered,
                        instructions,
                        next,
                        parameter_names,
                        local_bindings,
                    )?;
                    lowered.insert(id, result);
                    return Ok(result);
                }
            }
            if let Some(literal) = node.literal {
                match literal {
                    TypedLiteral::Int(value) => {
                        instructions.push(Instruction::ConstInt { result, value })
                    }
                    TypedLiteral::Bool(value) => {
                        instructions.push(Instruction::ConstBool { result, value })
                    }
                }
            } else {
                let values = node
                    .children
                    .iter()
                    .map(|child| {
                        lower(
                            *child,
                            nodes,
                            lowered,
                            instructions,
                            next,
                            parameter_names,
                            local_bindings,
                        )
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                match (node.kind.as_str(), node.detail.as_deref()) {
                    ("if", _) => {
                        // Constant conditions can be folded at the typed-HIR
                        // boundary.  The primitive subset has no effects, so
                        // lowering both already-validated branches is safe;
                        // only the selected value becomes the node's result.
                        if values.len() != 3 {
                            return Err(LowerError::UnsupportedExpression);
                        }
                        let condition = nodes
                            .get(node.children[0] as usize)
                            .and_then(|condition| condition.literal)
                            .and_then(|literal| match literal {
                                TypedLiteral::Bool(value) => Some(value),
                                _ => None,
                            })
                            .ok_or(LowerError::UnsupportedExpression)?;
                        result = values[if condition { 1 } else { 2 }];
                    }
                    ("name", Some(name)) => {
                        let index = parameter_names
                            .iter()
                            .position(|parameter| parameter == name)
                            .ok_or(LowerError::UnsupportedExpression)?;
                        instructions.push(Instruction::Param {
                            result,
                            index: index as u32,
                        });
                    }
                    ("binary", detail) => {
                        if values.len() != 2 {
                            return Err(LowerError::UnsupportedExpression);
                        }
                        let (left, right) = (values[0], values[1]);
                        let instruction = match detail {
                            Some("Add") => Instruction::Add {
                                result,
                                left,
                                right,
                            },
                            Some("Sub") => Instruction::Sub {
                                result,
                                left,
                                right,
                            },
                            Some("Mul") => Instruction::Mul {
                                result,
                                left,
                                right,
                            },
                            Some("Pow") => Instruction::Pow {
                                result,
                                left,
                                right,
                            },
                            Some("Div") => Instruction::Div {
                                result,
                                left,
                                right,
                            },
                            Some("FloorDiv") => Instruction::FloorDiv {
                                result,
                                left,
                                right,
                            },
                            Some("Mod") => Instruction::Mod {
                                result,
                                left,
                                right,
                            },
                            Some("BitAnd") => Instruction::BitAnd {
                                result,
                                left,
                                right,
                            },
                            Some("BitOr") => Instruction::BitOr {
                                result,
                                left,
                                right,
                            },
                            Some("BitXor") => Instruction::BitXor {
                                result,
                                left,
                                right,
                            },
                            Some("Shl") => Instruction::Shl {
                                result,
                                left,
                                right,
                            },
                            Some("Shr") => Instruction::Shr {
                                result,
                                left,
                                right,
                            },
                            Some("Eq") | Some("Identity") | Some("Is") => Instruction::CmpEq {
                                result,
                                left,
                                right,
                            },
                            Some("NotEq") | Some("NotIdentity") | Some("IsNot") => {
                                Instruction::CmpNe {
                                    result,
                                    left,
                                    right,
                                }
                            }
                            Some("Lt") => Instruction::CmpLt {
                                result,
                                left,
                                right,
                            },
                            Some("LtEq") => Instruction::CmpLe {
                                result,
                                left,
                                right,
                            },
                            Some("Gt") => Instruction::CmpGt {
                                result,
                                left,
                                right,
                            },
                            Some("GtEq") => Instruction::CmpGe {
                                result,
                                left,
                                right,
                            },
                            _ => return Err(LowerError::UnsupportedExpression),
                        };
                        instructions.push(instruction);
                    }
                    ("unary", detail) => {
                        if values.len() != 1 {
                            return Err(LowerError::UnsupportedExpression);
                        }
                        let operand = values[0];
                        if detail == Some("Pos") {
                            result = operand;
                            lowered.insert(id, result);
                            return Ok(result);
                        }
                        let instruction = match detail {
                            Some("Neg") => Instruction::Neg { result, operand },
                            Some("Invert") => Instruction::BitNot { result, operand },
                            Some("Not") => Instruction::Not { result, operand },
                            _ => return Err(LowerError::UnsupportedExpression),
                        };
                        instructions.push(instruction);
                    }
                    _ => return Err(LowerError::UnsupportedExpression),
                }
            }
            lowered.insert(id, result);
            Ok(result)
        }
        let mut last = None;
        let mut next = 0;
        for root in roots {
            last = Some(lower(
                *root,
                nodes,
                &mut lowered,
                &mut instructions,
                &mut next,
                parameter_names,
                local_bindings,
            )?);
        }
        let function = Self {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions,
                terminator: Terminator::Return(last),
            }],
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a dynamic statement-level conditional from typed expression
    /// roots. Each root must be a single-block primitive expression; the
    /// resulting CIR contains an explicit branch and value merge.
    pub fn from_typed_statement_if(
        nodes: &[TypedExprNode],
        condition_root: u32,
        then_root: u32,
        else_root: u32,
        parameter_names: &[String],
        local_bindings: &[(String, u32)],
    ) -> Result<Self, LowerError> {
        let mut seen = std::collections::HashSet::with_capacity(parameter_names.len());
        if parameter_names.iter().any(|name| !seen.insert(name)) {
            return Err(LowerError::UnsupportedExpression);
        }
        Self::from_typed_dynamic_if(
            nodes,
            condition_root,
            then_root,
            else_root,
            parameter_names,
            local_bindings,
        )
    }

    /// Lower a typed conditional whose then arm returns a value and whose
    /// else arm falls through with a void return.
    pub fn from_typed_statement_if_optional(
        nodes: &[TypedExprNode],
        condition_root: u32,
        then_root: u32,
        parameter_names: &[String],
        local_bindings: &[(String, u32)],
    ) -> Result<Self, LowerError> {
        let mut extended = nodes.to_vec();
        let else_root =
            u32::try_from(extended.len()).map_err(|_| LowerError::UnsupportedExpression)?;
        extended.push(TypedExprNode {
            id: else_root,
            kind: "literal".into(),
            detail: None,
            children: Vec::new(),
            literal: Some(TypedLiteral::Int(0)),
        });
        let mut function = Self::from_typed_statement_if(
            &extended,
            condition_root,
            then_root,
            else_root,
            parameter_names,
            local_bindings,
        )?;
        let then_value = match function
            .blocks
            .get(3)
            .and_then(|block| block.instructions.first())
        {
            Some(Instruction::Phi { incomings, .. }) => incomings
                .iter()
                .find(|(block, _)| *block == BlockId(1))
                .map(|(_, value)| *value)
                .ok_or(LowerError::UnsupportedExpression)?,
            _ => return Err(LowerError::UnsupportedExpression),
        };
        function.blocks[1].terminator = Terminator::Return(Some(then_value));
        function.blocks[2].instructions.clear();
        function.blocks[2].terminator = Terminator::Return(None);
        function.blocks.truncate(3);
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    fn from_typed_dynamic_if(
        nodes: &[TypedExprNode],
        condition_root: u32,
        then_root: u32,
        else_root: u32,
        parameter_names: &[String],
        local_bindings: &[(String, u32)],
    ) -> Result<Self, LowerError> {
        let condition =
            Self::from_typed_graph(nodes, &[condition_root], parameter_names, local_bindings)?;
        let then_branch =
            Self::from_typed_graph(nodes, &[then_root], parameter_names, local_bindings)?;
        let else_branch =
            Self::from_typed_graph(nodes, &[else_root], parameter_names, local_bindings)?;
        if condition.blocks.len() != 1
            || then_branch.blocks.len() != 1
            || else_branch.blocks.len() != 1
        {
            return Err(LowerError::UnsupportedExpression);
        }
        let condition_block = &condition.blocks[0];
        let then_block = &then_branch.blocks[0];
        let else_block = &else_branch.blocks[0];
        let condition_value = match condition_block.terminator {
            Terminator::Return(Some(value)) => value,
            _ => return Err(LowerError::UnsupportedExpression),
        };
        let then_value = match then_block.terminator {
            Terminator::Return(Some(value)) => value,
            _ => return Err(LowerError::UnsupportedExpression),
        };
        let else_value = match else_block.terminator {
            Terminator::Return(Some(value)) => value,
            _ => return Err(LowerError::UnsupportedExpression),
        };

        fn remap_instruction(instruction: &Instruction, offset: u32) -> Instruction {
            let value = |value: ValueId| ValueId(value.0 + offset);
            let block = |block: BlockId| BlockId(block.0 + offset);
            match instruction {
                Instruction::Param { result, index } => Instruction::Param {
                    result: value(*result),
                    index: *index,
                },
                Instruction::ConstInt {
                    result,
                    value: literal,
                } => Instruction::ConstInt {
                    result: value(*result),
                    value: *literal,
                },
                Instruction::ConstBool {
                    result,
                    value: literal,
                } => Instruction::ConstBool {
                    result: value(*result),
                    value: *literal,
                },
                Instruction::Add {
                    result,
                    left,
                    right,
                } => Instruction::Add {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Sub {
                    result,
                    left,
                    right,
                } => Instruction::Sub {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Mul {
                    result,
                    left,
                    right,
                } => Instruction::Mul {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Pow {
                    result,
                    left,
                    right,
                } => Instruction::Pow {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Div {
                    result,
                    left,
                    right,
                } => Instruction::Div {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::FloorDiv {
                    result,
                    left,
                    right,
                } => Instruction::FloorDiv {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Mod {
                    result,
                    left,
                    right,
                } => Instruction::Mod {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::BitAnd {
                    result,
                    left,
                    right,
                } => Instruction::BitAnd {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::BitOr {
                    result,
                    left,
                    right,
                } => Instruction::BitOr {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::BitXor {
                    result,
                    left,
                    right,
                } => Instruction::BitXor {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Shl {
                    result,
                    left,
                    right,
                } => Instruction::Shl {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Shr {
                    result,
                    left,
                    right,
                } => Instruction::Shr {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::CmpEq {
                    result,
                    left,
                    right,
                } => Instruction::CmpEq {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::CmpNe {
                    result,
                    left,
                    right,
                } => Instruction::CmpNe {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::CmpLe {
                    result,
                    left,
                    right,
                } => Instruction::CmpLe {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::CmpGt {
                    result,
                    left,
                    right,
                } => Instruction::CmpGt {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::CmpGe {
                    result,
                    left,
                    right,
                } => Instruction::CmpGe {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::CmpLt {
                    result,
                    left,
                    right,
                } => Instruction::CmpLt {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Neg { result, operand } => Instruction::Neg {
                    result: value(*result),
                    operand: value(*operand),
                },
                Instruction::BitNot { result, operand } => Instruction::BitNot {
                    result: value(*result),
                    operand: value(*operand),
                },
                Instruction::Not { result, operand } => Instruction::Not {
                    result: value(*result),
                    operand: value(*operand),
                },
                Instruction::And {
                    result,
                    left,
                    right,
                } => Instruction::And {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Or {
                    result,
                    left,
                    right,
                } => Instruction::Or {
                    result: value(*result),
                    left: value(*left),
                    right: value(*right),
                },
                Instruction::Phi { result, incomings } => Instruction::Phi {
                    result: value(*result),
                    incomings: incomings
                        .iter()
                        .map(|(predecessor, incoming)| (block(*predecessor), value(*incoming)))
                        .collect(),
                },
            }
        }
        fn result_id(instruction: &Instruction) -> ValueId {
            match instruction {
                Instruction::Param { result, .. }
                | Instruction::ConstInt { result, .. }
                | Instruction::ConstBool { result, .. }
                | Instruction::Add { result, .. }
                | Instruction::Sub { result, .. }
                | Instruction::Mul { result, .. }
                | Instruction::Pow { result, .. }
                | Instruction::Div { result, .. }
                | Instruction::FloorDiv { result, .. }
                | Instruction::Mod { result, .. }
                | Instruction::BitAnd { result, .. }
                | Instruction::BitOr { result, .. }
                | Instruction::BitXor { result, .. }
                | Instruction::Shl { result, .. }
                | Instruction::Shr { result, .. }
                | Instruction::CmpEq { result, .. }
                | Instruction::CmpNe { result, .. }
                | Instruction::CmpLe { result, .. }
                | Instruction::CmpGt { result, .. }
                | Instruction::CmpGe { result, .. }
                | Instruction::CmpLt { result, .. }
                | Instruction::Neg { result, .. }
                | Instruction::BitNot { result, .. }
                | Instruction::Not { result, .. }
                | Instruction::And { result, .. }
                | Instruction::Or { result, .. }
                | Instruction::Phi { result, .. } => *result,
            }
        }
        let condition_offset = 0;
        let then_offset = condition_block
            .instructions
            .iter()
            .map(result_id)
            .chain(std::iter::once(condition_value))
            .map(|value| value.0 + 1)
            .max()
            .unwrap_or(0);
        let else_offset = then_offset
            + then_block
                .instructions
                .iter()
                .map(result_id)
                .chain(std::iter::once(then_value))
                .map(|value| value.0 + 1)
                .max()
                .unwrap_or(0);
        let branch_has_recoverable_operation = then_block
            .instructions
            .iter()
            .chain(else_block.instructions.iter())
            .any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Div { .. }
                        | Instruction::FloorDiv { .. }
                        | Instruction::Mod { .. }
                )
            });
        let condition_has_recoverable_operation =
            condition_block.instructions.iter().any(|instruction| {
                matches!(
                    instruction,
                    Instruction::Div { .. }
                        | Instruction::FloorDiv { .. }
                        | Instruction::Mod { .. }
                )
            });
        if branch_has_recoverable_operation {
            if condition_has_recoverable_operation {
                return Err(LowerError::UnsupportedExpression);
            }
            let mut blocks = vec![Block {
                id: BlockId(0),
                instructions: condition_block.instructions.clone(),
                terminator: Terminator::Branch {
                    condition: ValueId(condition_value.0 + condition_offset),
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            }];
            blocks.push(Block {
                id: BlockId(1),
                instructions: then_block
                    .instructions
                    .iter()
                    .map(|instruction| remap_instruction(instruction, then_offset))
                    .collect(),
                terminator: Terminator::Return(Some(ValueId(then_value.0 + then_offset))),
            });
            blocks.push(Block {
                id: BlockId(2),
                instructions: else_block
                    .instructions
                    .iter()
                    .map(|instruction| remap_instruction(instruction, else_offset))
                    .collect(),
                terminator: Terminator::Return(Some(ValueId(else_value.0 + else_offset))),
            });
            let function = Self {
                entry: BlockId(0),
                blocks,
            };
            function
                .verify()
                .map_err(|_| LowerError::UnsupportedExpression)?;
            return Ok(function);
        }
        let merge_value = ValueId(
            else_offset
                + else_block
                    .instructions
                    .iter()
                    .map(result_id)
                    .chain(std::iter::once(else_value))
                    .map(|value| value.0 + 1)
                    .max()
                    .unwrap_or(0),
        );
        let mut blocks = vec![Block {
            id: BlockId(0),
            instructions: condition_block.instructions.clone(),
            terminator: Terminator::Branch {
                condition: ValueId(condition_value.0 + condition_offset),
                then_block: BlockId(1),
                else_block: BlockId(2),
            },
        }];
        blocks.push(Block {
            id: BlockId(1),
            instructions: then_block
                .instructions
                .iter()
                .map(|instruction| remap_instruction(instruction, then_offset))
                .collect(),
            terminator: Terminator::Jump(BlockId(3)),
        });
        blocks.push(Block {
            id: BlockId(2),
            instructions: else_block
                .instructions
                .iter()
                .map(|instruction| remap_instruction(instruction, else_offset))
                .collect(),
            terminator: Terminator::Jump(BlockId(3)),
        });
        blocks.push(Block {
            id: BlockId(3),
            instructions: vec![Instruction::Phi {
                result: merge_value,
                incomings: vec![
                    (BlockId(1), ValueId(then_value.0 + then_offset)),
                    (BlockId(2), ValueId(else_value.0 + else_offset)),
                ],
            }],
            terminator: Terminator::Return(Some(merge_value)),
        });
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Serialize this function in a deterministic, human-readable form for
    /// snapshots and backend diagnostics.
    pub fn to_text(&self) -> String {
        let mut output = format!("entry b{}\n", self.entry.0);
        for block in &self.blocks {
            let _ = writeln!(output, "b{}:", block.id.0);
            for instruction in &block.instructions {
                let _ = writeln!(output, "  {instruction:?}");
            }
            let _ = writeln!(output, "  {:?}", block.terminator);
        }
        output
    }

    /// Lower the first top-level assignment in a module. Statement and
    /// function lowering can extend this boundary without duplicating AST
    /// selection in callers.
    pub fn from_module(module: &lucid_syntax::Module) -> Result<Self, LowerError> {
        match Self::from_module_linear(module) {
            Ok(function) => Ok(function),
            Err(error) => {
                // Preserve expression-level lowering for a single initializer
                // whose value is already a structured expression (for example
                // a short-circuit or `if` expression). Multi-statement modules
                // never fall back to the old truncating behavior.
                fn initializer(statement: &lucid_syntax::Stmt) -> Option<&lucid_syntax::Expr> {
                    match statement {
                        lucid_syntax::Stmt::Assignment { value, .. }
                        | lucid_syntax::Stmt::VarDef {
                            value: Some(value), ..
                        } => Some(value),
                        lucid_syntax::Stmt::Export(inner) => initializer(inner),
                        _ => None,
                    }
                }
                if module.statements.len() == 1 {
                    initializer(&module.statements[0])
                        .map(Self::from_expr)
                        .unwrap_or(Err(error))
                } else {
                    Err(error)
                }
            }
        }
    }

    /// Lower a small dynamic-if module into a real CIR diamond. This accepts
    /// one `if` statement whose branches assign the same name from supported
    /// expressions; it is the seed for general statement lowering and proves
    /// branch-local definitions cross the merge only through a Phi.
    pub fn from_module_if(module: &lucid_syntax::Module) -> Result<Self, LowerError> {
        let [lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch,
            elif_branches,
            ..
        }] = module.statements.as_slice()
        else {
            return Err(LowerError::UnsupportedExpression);
        };
        fn static_truth(expr: &lucid_syntax::Expr) -> Option<bool> {
            match expr {
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Bool(value),
                    ..
                } => Some(*value),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Int(value),
                    ..
                } => Some(*value != 0),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Str(value),
                    ..
                } => Some(!value.is_empty()),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::None,
                    ..
                } => Some(false),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Float(value),
                    ..
                } => Some(*value != 0.0),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Complex(value),
                    ..
                } => Some(*value != 0.0),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::BigInt(value),
                    ..
                } => {
                    let digits = value.trim().trim_start_matches(['+', '-']).replace('_', "");
                    let (digits, radix) = if let Some(value) = digits
                        .strip_prefix("0x")
                        .or_else(|| digits.strip_prefix("0X"))
                    {
                        (value, 16)
                    } else if let Some(value) = digits
                        .strip_prefix("0o")
                        .or_else(|| digits.strip_prefix("0O"))
                    {
                        (value, 8)
                    } else if let Some(value) = digits
                        .strip_prefix("0b")
                        .or_else(|| digits.strip_prefix("0B"))
                    {
                        (value, 2)
                    } else {
                        (digits.as_str(), 10)
                    };
                    Some(digits.chars().any(|digit| digit.to_digit(radix) != Some(0)))
                }
                lucid_syntax::Expr::Literal {
                    value:
                        lucid_syntax::LiteralValue::Sentinel(_) | lucid_syntax::LiteralValue::Ellipsis,
                    ..
                } => Some(true),
                lucid_syntax::Expr::Binary {
                    op: lucid_syntax::BinaryOp::And,
                    left,
                    right,
                    ..
                } => match static_truth(left) {
                    Some(false) => Some(false),
                    Some(true) => static_truth(right),
                    None => None,
                },
                lucid_syntax::Expr::Binary {
                    op: lucid_syntax::BinaryOp::Or,
                    left,
                    right,
                    ..
                } => match static_truth(left) {
                    Some(true) => Some(true),
                    Some(false) => static_truth(right),
                    None => None,
                },
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Not,
                    expr,
                    ..
                } => static_truth(expr).map(|value| !value),
                lucid_syntax::Expr::Binary {
                    op, left, right, ..
                } => {
                    if let (
                        lucid_syntax::Expr::Literal {
                            value: lucid_syntax::LiteralValue::Bool(left),
                            ..
                        },
                        lucid_syntax::Expr::Literal {
                            value: lucid_syntax::LiteralValue::Bool(right),
                            ..
                        },
                    ) = (left.as_ref(), right.as_ref())
                    {
                        return Some(match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => left == right,
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => left != right,
                            _ => return None,
                        });
                    }
                    let comparison = |left: f64, right: f64| match op {
                        lucid_syntax::BinaryOp::Eq
                        | lucid_syntax::BinaryOp::Identity
                        | lucid_syntax::BinaryOp::Is => Some(left == right),
                        lucid_syntax::BinaryOp::NotEq
                        | lucid_syntax::BinaryOp::NotIdentity
                        | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                        lucid_syntax::BinaryOp::Lt => Some(left < right),
                        lucid_syntax::BinaryOp::LtEq => Some(left <= right),
                        lucid_syntax::BinaryOp::Gt => Some(left > right),
                        lucid_syntax::BinaryOp::GtEq => Some(left >= right),
                        _ => None,
                    };
                    match (left.as_ref(), right.as_ref()) {
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Str(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Str(right),
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(left == right),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::None,
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::None,
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(true),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(false),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Sentinel(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Sentinel(right),
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(left == right),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Ellipsis,
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Ellipsis,
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(true),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(false),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::BigInt(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::BigInt(right),
                                ..
                            },
                        ) => {
                            let normalize = |value: &str| {
                                let value = value.trim().replace('_', "");
                                let negative = value.starts_with('-');
                                let digits =
                                    value.trim_start_matches(['+', '-']).trim_start_matches('0');
                                let digits = if digits.is_empty() { "0" } else { digits };
                                if negative && digits != "0" {
                                    format!("-{digits}")
                                } else {
                                    digits.to_string()
                                }
                            };
                            let equal = normalize(left) == normalize(right);
                            match op {
                                lucid_syntax::BinaryOp::Eq
                                | lucid_syntax::BinaryOp::Identity
                                | lucid_syntax::BinaryOp::Is => Some(equal),
                                lucid_syntax::BinaryOp::NotEq
                                | lucid_syntax::BinaryOp::NotIdentity
                                | lucid_syntax::BinaryOp::IsNot => Some(!equal),
                                _ => None,
                            }
                        }
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Int(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Int(right),
                                ..
                            },
                        ) => comparison(*left as f64, *right as f64),
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Float(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Float(right),
                                ..
                            },
                        ) => comparison(*left, *right),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        let selected_else: &[lucid_syntax::Stmt] = if elif_branches.is_empty() {
            else_branch
                .as_deref()
                .ok_or(LowerError::UnsupportedExpression)?
        } else {
            let mut selected: Option<&[lucid_syntax::Stmt]> = None;
            for (elif_condition, elif_body) in elif_branches {
                let truth = match elif_condition {
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Bool(value),
                        ..
                    } => *value,
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Int(value),
                        ..
                    } => *value != 0,
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Str(value),
                        ..
                    } => !value.is_empty(),
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::None,
                        ..
                    } => false,
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Float(value),
                        ..
                    } => *value != 0.0,
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Complex(value),
                        ..
                    } => *value != 0.0,
                    lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::BigInt(value),
                        ..
                    } => {
                        let digits = value.trim().trim_start_matches(['+', '-']).replace('_', "");
                        let (digits, radix) = if let Some(value) = digits
                            .strip_prefix("0x")
                            .or_else(|| digits.strip_prefix("0X"))
                        {
                            (value, 16)
                        } else if let Some(value) = digits
                            .strip_prefix("0o")
                            .or_else(|| digits.strip_prefix("0O"))
                        {
                            (value, 8)
                        } else if let Some(value) = digits
                            .strip_prefix("0b")
                            .or_else(|| digits.strip_prefix("0B"))
                        {
                            (value, 2)
                        } else {
                            (digits.as_str(), 10)
                        };
                        digits.chars().any(|digit| digit.to_digit(radix) != Some(0))
                    }
                    lucid_syntax::Expr::Literal {
                        value:
                            lucid_syntax::LiteralValue::Sentinel(_)
                            | lucid_syntax::LiteralValue::Ellipsis,
                        ..
                    } => true,
                    lucid_syntax::Expr::Binary {
                        op: lucid_syntax::BinaryOp::And | lucid_syntax::BinaryOp::Or,
                        ..
                    } => static_truth(elif_condition).ok_or(LowerError::UnsupportedExpression)?,
                    lucid_syntax::Expr::Unary {
                        op: lucid_syntax::UnaryOp::Not,
                        ..
                    } => static_truth(elif_condition).ok_or(LowerError::UnsupportedExpression)?,
                    lucid_syntax::Expr::Binary { .. } => {
                        static_truth(elif_condition).ok_or(LowerError::UnsupportedExpression)?
                    }
                    _ => return Err(LowerError::UnsupportedExpression),
                };
                if truth {
                    selected = Some(elif_body);
                    break;
                }
            }
            selected
                .or(else_branch.as_deref())
                .ok_or(LowerError::UnsupportedExpression)?
        };
        fn branch_name(statements: &[lucid_syntax::Stmt]) -> Result<&str, LowerError> {
            match statements.last() {
                Some(lucid_syntax::Stmt::Assignment {
                    target: lucid_syntax::Expr::Ident { name, .. },
                    ..
                }) => Ok(name),
                Some(lucid_syntax::Stmt::VarDef {
                    pattern: lucid_syntax::Pattern::Ident(name, _),
                    value: Some(_),
                    ..
                }) => Ok(name),
                Some(lucid_syntax::Stmt::Export(inner)) => {
                    branch_name(std::slice::from_ref(inner.as_ref()))
                }
                _ => Err(LowerError::UnsupportedExpression),
            }
        }
        let name_then = branch_name(then_branch)?;
        let name_else = branch_name(selected_else)?;
        if name_then != name_else {
            return Err(LowerError::UnsupportedExpression);
        }
        let condition_fn = Self::from_expr(condition)?;
        let [condition_block] = condition_fn.blocks.as_slice() else {
            return Err(LowerError::UnsupportedExpression);
        };
        let Terminator::Return(Some(condition_value)) = condition_block.terminator else {
            return Err(LowerError::UnsupportedExpression);
        };
        fn shift(value: ValueId, offset: u32) -> ValueId {
            ValueId(value.0 + offset)
        }
        fn shift_instruction(instruction: &Instruction, offset: u32) -> Instruction {
            let s = |value| shift(value, offset);
            match instruction {
                Instruction::Param { result, index } => Instruction::Param {
                    result: s(*result),
                    index: *index,
                },
                Instruction::ConstInt { result, value } => Instruction::ConstInt {
                    result: s(*result),
                    value: *value,
                },
                Instruction::ConstBool { result, value } => Instruction::ConstBool {
                    result: s(*result),
                    value: *value,
                },
                Instruction::Add {
                    result,
                    left,
                    right,
                } => Instruction::Add {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Sub {
                    result,
                    left,
                    right,
                } => Instruction::Sub {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Mul {
                    result,
                    left,
                    right,
                } => Instruction::Mul {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Pow {
                    result,
                    left,
                    right,
                } => Instruction::Pow {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Div {
                    result,
                    left,
                    right,
                } => Instruction::Div {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::FloorDiv {
                    result,
                    left,
                    right,
                } => Instruction::FloorDiv {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Mod {
                    result,
                    left,
                    right,
                } => Instruction::Mod {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::BitAnd {
                    result,
                    left,
                    right,
                } => Instruction::BitAnd {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::BitOr {
                    result,
                    left,
                    right,
                } => Instruction::BitOr {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::BitXor {
                    result,
                    left,
                    right,
                } => Instruction::BitXor {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Shl {
                    result,
                    left,
                    right,
                } => Instruction::Shl {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Shr {
                    result,
                    left,
                    right,
                } => Instruction::Shr {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::CmpEq {
                    result,
                    left,
                    right,
                } => Instruction::CmpEq {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::CmpNe {
                    result,
                    left,
                    right,
                } => Instruction::CmpNe {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::CmpLe {
                    result,
                    left,
                    right,
                } => Instruction::CmpLe {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::CmpGt {
                    result,
                    left,
                    right,
                } => Instruction::CmpGt {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::CmpGe {
                    result,
                    left,
                    right,
                } => Instruction::CmpGe {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::CmpLt {
                    result,
                    left,
                    right,
                } => Instruction::CmpLt {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Neg { result, operand } => Instruction::Neg {
                    result: s(*result),
                    operand: s(*operand),
                },
                Instruction::BitNot { result, operand } => Instruction::BitNot {
                    result: s(*result),
                    operand: s(*operand),
                },
                Instruction::Not { result, operand } => Instruction::Not {
                    result: s(*result),
                    operand: s(*operand),
                },
                Instruction::And {
                    result,
                    left,
                    right,
                } => Instruction::And {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Or {
                    result,
                    left,
                    right,
                } => Instruction::Or {
                    result: s(*result),
                    left: s(*left),
                    right: s(*right),
                },
                Instruction::Phi { result, incomings } => Instruction::Phi {
                    result: s(*result),
                    incomings: incomings
                        .iter()
                        .map(|(block, value)| (*block, s(*value)))
                        .collect(),
                },
            }
        }
        let condition_count = condition_block.instructions.len() as u32;
        let then_function = Self::from_module(&lucid_syntax::Module {
            statements: then_branch.clone(),
            span: module.span,
        })?;
        let else_function = Self::from_module(&lucid_syntax::Module {
            statements: selected_else.to_vec(),
            span: module.span,
        })?;
        let [then_block] = then_function.blocks.as_slice() else {
            return Err(LowerError::UnsupportedExpression);
        };
        let [else_block] = else_function.blocks.as_slice() else {
            return Err(LowerError::UnsupportedExpression);
        };
        let then_offset = condition_count;
        let else_offset = then_offset + then_block.instructions.len() as u32;
        let then_value = match then_block.terminator {
            Terminator::Return(Some(value)) => shift(value, then_offset),
            _ => return Err(LowerError::UnsupportedExpression),
        };
        let else_value = match else_block.terminator {
            Terminator::Return(Some(value)) => shift(value, else_offset),
            _ => return Err(LowerError::UnsupportedExpression),
        };
        let result = ValueId(else_offset + else_block.instructions.len() as u32);
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: condition_block.instructions.clone(),
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                instructions: then_block
                    .instructions
                    .iter()
                    .map(|instruction| shift_instruction(instruction, then_offset))
                    .collect(),
                terminator: Terminator::Jump(BlockId(3)),
            },
            Block {
                id: BlockId(2),
                instructions: else_block
                    .instructions
                    .iter()
                    .map(|instruction| shift_instruction(instruction, else_offset))
                    .collect(),
                terminator: Terminator::Jump(BlockId(3)),
            },
            Block {
                id: BlockId(3),
                instructions: vec![Instruction::Phi {
                    result,
                    incomings: vec![(BlockId(1), then_value), (BlockId(2), else_value)],
                }],
                terminator: Terminator::Return(Some(result)),
            },
        ];
        let function = Self {
            entry: BlockId(0),
            blocks: std::mem::take(&mut blocks),
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    fn canonical_loop_tail(tail: &[lucid_syntax::Stmt]) -> bool {
        match tail {
            [] => true,
            [lucid_syntax::Stmt::Continue(_)] | [lucid_syntax::Stmt::Pass(_)] => true,
            statements => statements
                .iter()
                .all(|statement| matches!(statement, lucid_syntax::Stmt::Pass(_))),
        }
    }

    fn int_literal_expr(expr: &lucid_syntax::Expr) -> Option<i64> {
        match expr {
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Int(value),
                ..
            } => Some(*value),
            lucid_syntax::Expr::Unary {
                op: lucid_syntax::UnaryOp::Neg,
                expr,
                ..
            } => Self::int_literal_expr(expr).and_then(i64::checked_neg),
            lucid_syntax::Expr::Unary {
                op: lucid_syntax::UnaryOp::Pos,
                expr,
                ..
            } => Self::int_literal_expr(expr),
            _ => None,
        }
    }

    /// Recognize and lower `while x > 0: x -= 1; return x`-style integer
    /// induction loops. The initial value must be a positional parameter;
    /// the body must contain exactly one augmented assignment to that same
    /// name with an integer literal. Other loop shapes remain explicit
    /// lowering errors until their typed-CIR representation is added.
    fn from_counted_while(
        module: &lucid_syntax::Module,
        parameter_names: &[String],
    ) -> Option<Result<Self, LowerError>> {
        fn initialized_ident(
            statement: &lucid_syntax::Stmt,
        ) -> Option<(&String, &lucid_syntax::Expr)> {
            match statement {
                lucid_syntax::Stmt::Assignment {
                    target: lucid_syntax::Expr::Ident { name, .. },
                    value,
                    ..
                }
                | lucid_syntax::Stmt::VarDef {
                    pattern: lucid_syntax::Pattern::Ident(name, _),
                    value: Some(value),
                    ..
                } => Some((name, value)),
                _ => None,
            }
        }
        fn initializer_instruction(
            statement: &lucid_syntax::Stmt,
            target: &str,
            result: ValueId,
            parameter_names: &[String],
        ) -> Option<Instruction> {
            let (target_name, value) = initialized_ident(statement)?;
            if target_name != target {
                return None;
            }
            match Function::int_literal_expr(value) {
                Some(value) => Some(Instruction::ConstInt { result, value }),
                None => match value {
                    lucid_syntax::Expr::Ident { name, .. } => Some(Instruction::Param {
                        result,
                        index: parameter_names
                            .iter()
                            .position(|parameter| parameter == name)?
                            as u32,
                    }),
                    _ => None,
                },
            }
        }
        fn parameter_instruction(
            name: &str,
            result: ValueId,
            parameter_names: &[String],
        ) -> Option<Instruction> {
            Some(Instruction::Param {
                result,
                index: parameter_names
                    .iter()
                    .position(|parameter| parameter == name)? as u32,
            })
        }
        let (initial, bound_initial, while_statement, return_name) =
            match module.statements.as_slice() {
                [while_statement, lucid_syntax::Stmt::Return {
                    value: Some(lucid_syntax::Expr::Ident { name, .. }),
                    ..
                }] => (None, None, while_statement, Some(name)),
                [while_statement] => (None, None, while_statement, None),
                [initial, while_statement, lucid_syntax::Stmt::Return {
                    value: Some(lucid_syntax::Expr::Ident { name, .. }),
                    ..
                }] => (Some(initial), None, while_statement, Some(name)),
                [initial, while_statement] => (Some(initial), None, while_statement, None),
                [first_statement, second_statement, while_statement] => {
                    let lucid_syntax::Stmt::While { condition, .. } = while_statement else {
                        return None;
                    };
                    let lucid_syntax::Expr::Binary { left, right, .. } = condition else {
                        return None;
                    };
                    let lucid_syntax::Expr::Ident {
                        name: induction_name,
                        ..
                    } = left.as_ref()
                    else {
                        return None;
                    };
                    let lucid_syntax::Expr::Ident {
                        name: bound_name, ..
                    } = right.as_ref()
                    else {
                        return None;
                    };
                    if induction_name == bound_name {
                        return None;
                    }
                    let (first_name, _) = initialized_ident(first_statement)?;
                    let (second_name, _) = initialized_ident(second_statement)?;
                    if first_name == induction_name && second_name == bound_name {
                        (
                            Some(first_statement),
                            Some(second_statement),
                            while_statement,
                            None,
                        )
                    } else if first_name == bound_name && second_name == induction_name {
                        (
                            Some(second_statement),
                            Some(first_statement),
                            while_statement,
                            None,
                        )
                    } else {
                        return None;
                    }
                }
                [first_statement, second_statement, while_statement, lucid_syntax::Stmt::Return {
                    value: Some(lucid_syntax::Expr::Ident { name, .. }),
                    ..
                }] => {
                    let lucid_syntax::Stmt::While { condition, .. } = while_statement else {
                        return None;
                    };
                    let lucid_syntax::Expr::Binary { left, right, .. } = condition else {
                        return None;
                    };
                    let lucid_syntax::Expr::Ident {
                        name: induction_name,
                        ..
                    } = left.as_ref()
                    else {
                        return None;
                    };
                    let lucid_syntax::Expr::Ident {
                        name: bound_name, ..
                    } = right.as_ref()
                    else {
                        return None;
                    };
                    if induction_name == bound_name {
                        return None;
                    }
                    let (first_name, _) = initialized_ident(first_statement)?;
                    let (second_name, _) = initialized_ident(second_statement)?;
                    if first_name == induction_name && second_name == bound_name {
                        (
                            Some(first_statement),
                            Some(second_statement),
                            while_statement,
                            Some(name),
                        )
                    } else if first_name == bound_name && second_name == induction_name {
                        (
                            Some(second_statement),
                            Some(first_statement),
                            while_statement,
                            Some(name),
                        )
                    } else {
                        return None;
                    }
                }
                _ => return None,
            };
        let lucid_syntax::Stmt::While {
            condition,
            body,
            if_broken,
            ..
        } = while_statement
        else {
            return None;
        };
        let lucid_syntax::Expr::Binary {
            op, left, right, ..
        } = condition
        else {
            return None;
        };
        let lucid_syntax::Expr::Ident {
            name: condition_name,
            ..
        } = left.as_ref()
        else {
            return None;
        };
        if return_name
            .as_ref()
            .is_some_and(|name| *name != condition_name)
        {
            return None;
        }
        let name = condition_name;
        // A malformed/empty loop body must be rejected as an unsupported
        // shape, not indexed optimistically and panicked on. A trailing
        // `continue` is semantically equivalent to reaching the loop back
        // edge after the induction update, so the canonical lowering can
        // safely accept those forms too. `pass` is likewise a no-op and does
        // not alter the loop's observable behavior.
        if body.is_empty() || body.len() > 3 || !Self::canonical_loop_tail(&body[1..]) {
            return None;
        }
        // This canonical loop shape contains no `break`; reaching the end of
        // the body always takes the back edge and an exhausted condition exits
        // normally.  Lucid's `if_broken` suite therefore cannot run here, so
        // it is safe to leave even effectful suites unlowered.
        let _ = if_broken;
        let (update_op, step) = match &body[0] {
            lucid_syntax::Stmt::AugAssign {
                target:
                    lucid_syntax::Expr::Ident {
                        name: update_name, ..
                    },
                op: update_op,
                value,
                ..
            } if update_name == name
                && matches!(
                    update_op,
                    lucid_syntax::BinaryOp::Add | lucid_syntax::BinaryOp::Sub
                ) =>
            {
                (update_op.clone(), Function::int_literal_expr(value)?)
            }
            lucid_syntax::Stmt::Assignment {
                target:
                    lucid_syntax::Expr::Ident {
                        name: update_name, ..
                    },
                value:
                    lucid_syntax::Expr::Binary {
                        op, left, right, ..
                    },
                ..
            } if update_name == name
                && matches!(left.as_ref(), lucid_syntax::Expr::Ident { name: left_name, .. } if left_name == name)
                && Function::int_literal_expr(right.as_ref()).is_some() =>
            {
                if !matches!(
                    op,
                    lucid_syntax::BinaryOp::Add | lucid_syntax::BinaryOp::Sub
                ) {
                    return None;
                }
                (op.clone(), Function::int_literal_expr(right.as_ref())?)
            }
            lucid_syntax::Stmt::Assignment {
                target:
                    lucid_syntax::Expr::Ident {
                        name: update_name, ..
                    },
                value:
                    lucid_syntax::Expr::Binary {
                        op: lucid_syntax::BinaryOp::Add,
                        left,
                        right,
                        ..
                    },
                ..
            } if update_name == name
                && Function::int_literal_expr(left.as_ref()).is_some()
                && matches!(right.as_ref(), lucid_syntax::Expr::Ident { name: right_name, .. } if right_name == name) =>
            {
                (
                    lucid_syntax::BinaryOp::Add,
                    Function::int_literal_expr(left.as_ref())?,
                )
            }
            _ => return None,
        };
        if !matches!(
            update_op,
            lucid_syntax::BinaryOp::Add | lucid_syntax::BinaryOp::Sub
        ) {
            return None;
        }
        let initial_instruction = match initial {
            None => parameter_instruction(name, ValueId(0), parameter_names)?,
            Some(statement) => {
                initializer_instruction(statement, name, ValueId(0), parameter_names)?
            }
        };
        let bound_instruction = match (right.as_ref(), bound_initial) {
            (expr, None) if Function::int_literal_expr(expr).is_some() => {
                Some(Instruction::ConstInt {
                    result: ValueId(2),
                    value: Function::int_literal_expr(expr)?,
                })
            }
            (lucid_syntax::Expr::Ident { .. }, _) => None,
            _ => return None,
        };
        let mut entry_instructions = vec![initial_instruction];
        match right.as_ref() {
            expr if Function::int_literal_expr(expr).is_some() => {}
            lucid_syntax::Expr::Ident {
                name: bound_name, ..
            } if bound_name != name => {
                if let Some(statement) = bound_initial {
                    entry_instructions.push(initializer_instruction(
                        statement,
                        bound_name,
                        ValueId(2),
                        parameter_names,
                    )?);
                } else {
                    entry_instructions.push(parameter_instruction(
                        bound_name,
                        ValueId(2),
                        parameter_names,
                    )?);
                }
            }
            _ => return None,
        }
        let comparison = match op {
            lucid_syntax::BinaryOp::NotEq
            | lucid_syntax::BinaryOp::NotIdentity
            | lucid_syntax::BinaryOp::IsNot => Instruction::CmpNe {
                result: ValueId(3),
                left: ValueId(1),
                right: ValueId(2),
            },
            lucid_syntax::BinaryOp::Lt => Instruction::CmpLt {
                result: ValueId(3),
                left: ValueId(1),
                right: ValueId(2),
            },
            lucid_syntax::BinaryOp::LtEq => Instruction::CmpLe {
                result: ValueId(3),
                left: ValueId(1),
                right: ValueId(2),
            },
            lucid_syntax::BinaryOp::Gt => Instruction::CmpGt {
                result: ValueId(3),
                left: ValueId(1),
                right: ValueId(2),
            },
            lucid_syntax::BinaryOp::GtEq => Instruction::CmpGe {
                result: ValueId(3),
                left: ValueId(1),
                right: ValueId(2),
            },
            _ => return None,
        };
        let update = match update_op {
            lucid_syntax::BinaryOp::Add => Instruction::Add {
                result: ValueId(5),
                left: ValueId(1),
                right: ValueId(4),
            },
            lucid_syntax::BinaryOp::Sub => Instruction::Sub {
                result: ValueId(5),
                left: ValueId(1),
                right: ValueId(4),
            },
            _ => return None,
        };
        let function = Self {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: entry_instructions,
                    terminator: Terminator::Jump(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    instructions: {
                        let mut instructions = vec![Instruction::Phi {
                            result: ValueId(1),
                            incomings: vec![(BlockId(0), ValueId(0)), (BlockId(2), ValueId(5))],
                        }];
                        if let Some(bound_instruction) = bound_instruction {
                            instructions.push(bound_instruction);
                        }
                        instructions.push(comparison);
                        instructions
                    },
                    terminator: Terminator::Branch {
                        condition: ValueId(3),
                        then_block: BlockId(2),
                        else_block: BlockId(3),
                    },
                },
                Block {
                    id: BlockId(2),
                    instructions: vec![
                        Instruction::ConstInt {
                            result: ValueId(4),
                            value: step,
                        },
                        update,
                    ],
                    terminator: Terminator::Jump(BlockId(1)),
                },
                Block {
                    id: BlockId(3),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(return_name.map(|_| ValueId(1))),
                },
            ],
        };
        Some(
            function
                .verify()
                .map(|_| function)
                .map_err(|_| LowerError::UnsupportedExpression),
        )
    }

    /// Lower `total = 0; while n > 0: total += 1; n -= 1; return total`.
    /// This is the accumulator-bearing counterpart to [`Self::from_counted_while`]:
    /// the induction variable and accumulator are both loop-carried header Phis.
    fn from_counted_while_accumulator(
        module: &lucid_syntax::Module,
        parameter_names: &[String],
    ) -> Option<Result<Self, LowerError>> {
        fn initialized_ident(
            statement: &lucid_syntax::Stmt,
        ) -> Option<(&String, &lucid_syntax::Expr)> {
            match statement {
                lucid_syntax::Stmt::Assignment {
                    target: lucid_syntax::Expr::Ident { name, .. },
                    value,
                    ..
                }
                | lucid_syntax::Stmt::VarDef {
                    pattern: lucid_syntax::Pattern::Ident(name, _),
                    value: Some(value),
                    ..
                } => Some((name, value)),
                _ => None,
            }
        }
        fn initializer_instruction(
            expr: &lucid_syntax::Expr,
            result: ValueId,
            parameter_names: &[String],
        ) -> Option<Instruction> {
            match Function::int_literal_expr(expr) {
                Some(value) => Some(Instruction::ConstInt { result, value }),
                None => match expr {
                    lucid_syntax::Expr::Ident { name, .. } => Some(Instruction::Param {
                        result,
                        index: parameter_names
                            .iter()
                            .position(|parameter| parameter == name)?
                            as u32,
                    }),
                    _ => None,
                },
            }
        }
        fn bound_literal_instruction(
            expr: &lucid_syntax::Expr,
            result: ValueId,
        ) -> Option<Instruction> {
            Function::int_literal_expr(expr).map(|value| Instruction::ConstInt { result, value })
        }
        fn parameter_instruction(
            name: &str,
            result: ValueId,
            parameter_names: &[String],
        ) -> Option<Instruction> {
            Some(Instruction::Param {
                result,
                index: parameter_names
                    .iter()
                    .position(|parameter| parameter == name)? as u32,
            })
        }
        let statements = module.statements.as_slice();
        let (
            acc_name,
            initial_expr,
            induction_initial,
            bound_initial,
            condition,
            body,
            if_broken,
            return_name,
        ) = match statements {
            [acc_statement, while_statement, return_statement] => {
                let (acc_name, initial_expr) = initialized_ident(acc_statement)?;
                let lucid_syntax::Stmt::While {
                    condition,
                    body,
                    if_broken,
                    ..
                } = while_statement
                else {
                    return None;
                };
                let lucid_syntax::Stmt::Return {
                    value:
                        Some(lucid_syntax::Expr::Ident {
                            name: return_name, ..
                        }),
                    ..
                } = return_statement
                else {
                    return None;
                };
                (
                    acc_name,
                    initial_expr,
                    None,
                    None,
                    condition,
                    body,
                    if_broken,
                    return_name,
                )
            }
            [first_statement, second_statement, while_statement, return_statement] => {
                let lucid_syntax::Stmt::While {
                    condition,
                    body,
                    if_broken,
                    ..
                } = while_statement
                else {
                    return None;
                };
                let lucid_syntax::Stmt::Return {
                    value:
                        Some(lucid_syntax::Expr::Ident {
                            name: return_name, ..
                        }),
                    ..
                } = return_statement
                else {
                    return None;
                };
                let (first_name, first_expr) = initialized_ident(first_statement)?;
                let (second_name, second_expr) = initialized_ident(second_statement)?;
                let (acc_name, initial_expr, induction_statement) = if first_name == return_name {
                    (first_name, first_expr, second_statement)
                } else if second_name == return_name {
                    (second_name, second_expr, first_statement)
                } else {
                    return None;
                };
                (
                    acc_name,
                    initial_expr,
                    Some(induction_statement),
                    None,
                    condition,
                    body,
                    if_broken,
                    return_name,
                )
            }
            [first_statement, second_statement, third_statement, while_statement, return_statement] =>
            {
                let lucid_syntax::Stmt::While {
                    condition,
                    body,
                    if_broken,
                    ..
                } = while_statement
                else {
                    return None;
                };
                let lucid_syntax::Stmt::Return {
                    value:
                        Some(lucid_syntax::Expr::Ident {
                            name: return_name, ..
                        }),
                    ..
                } = return_statement
                else {
                    return None;
                };
                let lucid_syntax::Expr::Binary { left, right, .. } = condition else {
                    return None;
                };
                let lucid_syntax::Expr::Ident {
                    name: induction_name,
                    ..
                } = left.as_ref()
                else {
                    return None;
                };
                let lucid_syntax::Expr::Ident {
                    name: bound_name, ..
                } = right.as_ref()
                else {
                    return None;
                };
                if bound_name == induction_name {
                    return None;
                }
                let initializers = [
                    initialized_ident(first_statement)?,
                    initialized_ident(second_statement)?,
                    initialized_ident(third_statement)?,
                ];
                let acc_index = initializers
                    .iter()
                    .position(|(name, _)| *name == return_name)?;
                let induction_index = initializers
                    .iter()
                    .position(|(name, _)| *name == induction_name)?;
                let bound_index = initializers
                    .iter()
                    .position(|(name, _)| *name == bound_name)?;
                if acc_index == induction_index
                    || acc_index == bound_index
                    || induction_index == bound_index
                {
                    return None;
                }
                let initializer_statements = [first_statement, second_statement, third_statement];
                let (acc_name, initial_expr) = initializers[acc_index];
                (
                    acc_name,
                    initial_expr,
                    Some(initializer_statements[induction_index]),
                    Some(initializer_statements[bound_index]),
                    condition,
                    body,
                    if_broken,
                    return_name,
                )
            }
            _ => return None,
        };
        if induction_initial
            .and_then(initialized_ident)
            .is_some_and(|(name, _)| name == acc_name)
            || bound_initial
                .and_then(initialized_ident)
                .is_some_and(|(name, _)| name == acc_name)
        {
            return None;
        };
        if return_name != acc_name
            || body.len() < 2
            || body.len() > 4
            || !Self::canonical_loop_tail(&body[2..])
        {
            return None;
        }
        let _ = if_broken;
        let lucid_syntax::Expr::Binary {
            op, left, right, ..
        } = condition
        else {
            return None;
        };
        let lucid_syntax::Expr::Ident {
            name: induction_name,
            ..
        } = left.as_ref()
        else {
            return None;
        };
        let accumulator_instruction =
            initializer_instruction(initial_expr, ValueId(1), parameter_names)?;
        let induction_instruction = match induction_initial {
            Some(statement) => {
                let (target_name, initial_expr) = initialized_ident(statement)?;
                if target_name != induction_name {
                    return None;
                }
                initializer_instruction(initial_expr, ValueId(0), parameter_names)?
            }
            None => parameter_instruction(induction_name, ValueId(0), parameter_names)?,
        };
        let bound_instruction = match (right.as_ref(), bound_initial) {
            (expr, None) if bound_literal_instruction(expr, ValueId(4)).is_some() => {
                bound_literal_instruction(expr, ValueId(4))
            }
            (
                lucid_syntax::Expr::Ident {
                    name: bound_name, ..
                },
                None,
            ) if bound_name != induction_name => None,
            (
                lucid_syntax::Expr::Ident {
                    name: bound_name, ..
                },
                Some(statement),
            ) if bound_name != induction_name => {
                let (target_name, _) = initialized_ident(statement)?;
                if target_name != bound_name {
                    return None;
                }
                None
            }
            _ => return None,
        };
        let mut entry_instructions = vec![induction_instruction, accumulator_instruction];
        if let lucid_syntax::Expr::Ident {
            name: bound_name, ..
        } = right.as_ref()
        {
            if let Some(statement) = bound_initial {
                let (_, initial_expr) = initialized_ident(statement)?;
                entry_instructions.push(initializer_instruction(
                    initial_expr,
                    ValueId(4),
                    parameter_names,
                )?);
            } else {
                entry_instructions.push(parameter_instruction(
                    bound_name,
                    ValueId(4),
                    parameter_names,
                )?);
            }
        }
        #[derive(Clone, Copy)]
        enum AccumulatorOperand {
            Literal(i64),
            Induction,
        }
        fn accumulator_operand(
            expr: &lucid_syntax::Expr,
            induction_name: &str,
        ) -> Option<AccumulatorOperand> {
            match expr {
                lucid_syntax::Expr::Ident { name, .. } if name == induction_name => {
                    Some(AccumulatorOperand::Induction)
                }
                _ => Function::int_literal_expr(expr).map(AccumulatorOperand::Literal),
            }
        }
        fn accumulator_self_update(
            statement: &lucid_syntax::Stmt,
            target: &str,
            induction_name: &str,
        ) -> Option<(lucid_syntax::BinaryOp, AccumulatorOperand)> {
            match statement {
                lucid_syntax::Stmt::AugAssign {
                    target:
                        lucid_syntax::Expr::Ident {
                            name: update_name, ..
                        },
                    op: update_op @ (lucid_syntax::BinaryOp::Add | lucid_syntax::BinaryOp::Sub),
                    value,
                    ..
                } if update_name == target => Some((
                    update_op.clone(),
                    accumulator_operand(value, induction_name)?,
                )),
                lucid_syntax::Stmt::Assignment {
                    target:
                        lucid_syntax::Expr::Ident {
                            name: update_name, ..
                        },
                    value:
                        lucid_syntax::Expr::Binary {
                            op:
                                update_op @ (lucid_syntax::BinaryOp::Add | lucid_syntax::BinaryOp::Sub),
                            left,
                            right,
                            ..
                        },
                    ..
                } if update_name == target
                    && matches!(left.as_ref(), lucid_syntax::Expr::Ident { name, .. } if name == target) =>
                {
                    Some((
                        update_op.clone(),
                        accumulator_operand(right.as_ref(), induction_name)?,
                    ))
                }
                lucid_syntax::Stmt::Assignment {
                    target:
                        lucid_syntax::Expr::Ident {
                            name: update_name, ..
                        },
                    value:
                        lucid_syntax::Expr::Binary {
                            op: lucid_syntax::BinaryOp::Add,
                            left,
                            right,
                            ..
                        },
                    ..
                } if update_name == target
                    && matches!(right.as_ref(), lucid_syntax::Expr::Ident { name, .. } if name == target) =>
                {
                    Some((
                        lucid_syntax::BinaryOp::Add,
                        accumulator_operand(left.as_ref(), induction_name)?,
                    ))
                }
                _ => None,
            }
        }
        fn literal_self_update(
            statement: &lucid_syntax::Stmt,
            target: &str,
        ) -> Option<(lucid_syntax::BinaryOp, i64)> {
            match statement {
                lucid_syntax::Stmt::AugAssign {
                    target:
                        lucid_syntax::Expr::Ident {
                            name: update_name, ..
                        },
                    op: update_op @ (lucid_syntax::BinaryOp::Add | lucid_syntax::BinaryOp::Sub),
                    value,
                    ..
                } if update_name == target => {
                    Some((update_op.clone(), Function::int_literal_expr(value)?))
                }
                lucid_syntax::Stmt::Assignment {
                    target:
                        lucid_syntax::Expr::Ident {
                            name: update_name, ..
                        },
                    value:
                        lucid_syntax::Expr::Binary {
                            op:
                                update_op @ (lucid_syntax::BinaryOp::Add | lucid_syntax::BinaryOp::Sub),
                            left,
                            right,
                            ..
                        },
                    ..
                } if update_name == target
                    && matches!(left.as_ref(), lucid_syntax::Expr::Ident { name, .. } if name == target) =>
                {
                    Some((
                        update_op.clone(),
                        Function::int_literal_expr(right.as_ref())?,
                    ))
                }
                lucid_syntax::Stmt::Assignment {
                    target:
                        lucid_syntax::Expr::Ident {
                            name: update_name, ..
                        },
                    value:
                        lucid_syntax::Expr::Binary {
                            op: lucid_syntax::BinaryOp::Add,
                            left,
                            right,
                            ..
                        },
                    ..
                } if update_name == target
                    && matches!(right.as_ref(), lucid_syntax::Expr::Ident { name, .. } if name == target) =>
                {
                    Some((
                        lucid_syntax::BinaryOp::Add,
                        Function::int_literal_expr(left.as_ref())?,
                    ))
                }
                _ => None,
            }
        }
        let (acc_op, acc_operand) = accumulator_self_update(&body[0], acc_name, induction_name)?;
        let (induction_op, induction_step) = literal_self_update(&body[1], induction_name)?;
        let comparison = match op {
            lucid_syntax::BinaryOp::NotEq
            | lucid_syntax::BinaryOp::NotIdentity
            | lucid_syntax::BinaryOp::IsNot => Instruction::CmpNe {
                result: ValueId(5),
                left: ValueId(2),
                right: ValueId(4),
            },
            lucid_syntax::BinaryOp::Lt => Instruction::CmpLt {
                result: ValueId(5),
                left: ValueId(2),
                right: ValueId(4),
            },
            lucid_syntax::BinaryOp::LtEq => Instruction::CmpLe {
                result: ValueId(5),
                left: ValueId(2),
                right: ValueId(4),
            },
            lucid_syntax::BinaryOp::Gt => Instruction::CmpGt {
                result: ValueId(5),
                left: ValueId(2),
                right: ValueId(4),
            },
            lucid_syntax::BinaryOp::GtEq => Instruction::CmpGe {
                result: ValueId(5),
                left: ValueId(2),
                right: ValueId(4),
            },
            _ => return None,
        };
        let acc_operand_value = match acc_operand {
            AccumulatorOperand::Literal(_) => ValueId(6),
            AccumulatorOperand::Induction => ValueId(2),
        };
        let accumulator_update = match acc_op {
            lucid_syntax::BinaryOp::Add => Instruction::Add {
                result: ValueId(7),
                left: ValueId(3),
                right: acc_operand_value,
            },
            lucid_syntax::BinaryOp::Sub => Instruction::Sub {
                result: ValueId(7),
                left: ValueId(3),
                right: acc_operand_value,
            },
            _ => return None,
        };
        let induction_update = match induction_op {
            lucid_syntax::BinaryOp::Add => Instruction::Add {
                result: ValueId(9),
                left: ValueId(2),
                right: ValueId(8),
            },
            lucid_syntax::BinaryOp::Sub => Instruction::Sub {
                result: ValueId(9),
                left: ValueId(2),
                right: ValueId(8),
            },
            _ => return None,
        };
        let mut body_instructions = Vec::new();
        if let AccumulatorOperand::Literal(acc_step) = acc_operand {
            body_instructions.push(Instruction::ConstInt {
                result: ValueId(6),
                value: acc_step,
            });
        }
        body_instructions.push(accumulator_update);
        body_instructions.push(Instruction::ConstInt {
            result: ValueId(8),
            value: induction_step,
        });
        body_instructions.push(induction_update);
        let function = Self {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: entry_instructions,
                    terminator: Terminator::Jump(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    instructions: {
                        let mut instructions = vec![
                            Instruction::Phi {
                                result: ValueId(2),
                                incomings: vec![(BlockId(0), ValueId(0)), (BlockId(2), ValueId(9))],
                            },
                            Instruction::Phi {
                                result: ValueId(3),
                                incomings: vec![(BlockId(0), ValueId(1)), (BlockId(2), ValueId(7))],
                            },
                        ];
                        if let Some(bound_instruction) = bound_instruction {
                            instructions.push(bound_instruction);
                        }
                        instructions.push(comparison);
                        instructions
                    },
                    terminator: Terminator::Branch {
                        condition: ValueId(5),
                        then_block: BlockId(2),
                        else_block: BlockId(3),
                    },
                },
                Block {
                    id: BlockId(2),
                    instructions: body_instructions,
                    terminator: Terminator::Jump(BlockId(1)),
                },
                Block {
                    id: BlockId(3),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Some(ValueId(3))),
                },
            ],
        };
        Some(
            function
                .verify()
                .map(|_| function)
                .map_err(|_| LowerError::UnsupportedExpression),
        )
    }

    /// Lower `total = 0; for i in range(limit): total += i; return total`.
    /// The index and accumulator are independent loop-carried values, each
    /// represented by a header Phi and updated in the body block.
    fn from_counted_for(
        module: &lucid_syntax::Module,
        parameter_names: &[String],
    ) -> Option<Result<Self, LowerError>> {
        fn initialized_ident(
            statement: &lucid_syntax::Stmt,
        ) -> Option<(&String, &lucid_syntax::Expr)> {
            match statement {
                lucid_syntax::Stmt::Assignment {
                    target: lucid_syntax::Expr::Ident { name, .. },
                    value,
                    ..
                }
                | lucid_syntax::Stmt::VarDef {
                    pattern: lucid_syntax::Pattern::Ident(name, _),
                    value: Some(value),
                    ..
                } => Some((name, value)),
                _ => None,
            }
        }
        let (
            acc_name,
            initial_expr,
            bound_aliases,
            index_name,
            func,
            args,
            body,
            if_broken,
            return_name,
        ) = {
            let [setup @ .., for_statement, return_statement] = module.statements.as_slice() else {
                return None;
            };
            if setup.is_empty() {
                return None;
            }
            let lucid_syntax::Stmt::For {
                target: lucid_syntax::Pattern::Ident(index_name, _),
                iterable: lucid_syntax::Expr::Call { func, args, .. },
                body,
                if_broken,
                ..
            } = for_statement
            else {
                return None;
            };
            let lucid_syntax::Stmt::Return {
                value:
                    Some(lucid_syntax::Expr::Ident {
                        name: return_name, ..
                    }),
                ..
            } = return_statement
            else {
                return None;
            };
            let initializers = setup
                .iter()
                .map(initialized_ident)
                .collect::<Option<Vec<_>>>()?;
            let acc_index = initializers
                .iter()
                .position(|(name, _)| *name == return_name)?;
            let (acc_name, initial_expr) = initializers[acc_index];
            let mut seen_aliases = std::collections::HashSet::new();
            let mut bound_aliases = Vec::new();
            for (index, statement) in setup.iter().enumerate() {
                if index == acc_index {
                    continue;
                }
                let (alias_name, _) = initializers[index];
                if alias_name == acc_name
                    || alias_name == index_name
                    || !seen_aliases.insert(alias_name.as_str())
                {
                    return None;
                }
                bound_aliases.push(statement);
            }
            (
                acc_name,
                initial_expr,
                bound_aliases,
                index_name,
                func,
                args,
                body,
                if_broken,
                return_name,
            )
        };
        if return_name != acc_name
            || body.is_empty()
            || body.len() > 3
            || !Self::canonical_loop_tail(&body[1..])
            || !(1..=3).contains(&args.len())
            || !matches!(func.as_ref(), lucid_syntax::Expr::Ident { name, .. } if name == "range")
        {
            return None;
        }
        // The accepted range-accumulation shape has no `break`, so the
        // `if_broken` suite is unreachable and must not restrict lowering.
        let _ = if_broken;
        if args.iter().any(|arg| {
            arg.name.is_some() || arg.is_spread || arg.is_dict_spread || arg.is_gather_spread
        }) {
            return None;
        }
        #[derive(Clone, Copy)]
        enum RangeAccumulatorOperand {
            Literal(i64),
            Induction,
        }
        let accumulator_operand = |expr: &lucid_syntax::Expr| -> Option<RangeAccumulatorOperand> {
            match expr {
                lucid_syntax::Expr::Ident { name, .. } if name == index_name => {
                    Some(RangeAccumulatorOperand::Induction)
                }
                _ => Self::int_literal_expr(expr).map(RangeAccumulatorOperand::Literal),
            }
        };
        let accumulator_update = match &body[0] {
            lucid_syntax::Stmt::AugAssign {
                target:
                    lucid_syntax::Expr::Ident {
                        name: update_name, ..
                    },
                op: update_op @ (lucid_syntax::BinaryOp::Add | lucid_syntax::BinaryOp::Sub),
                value,
                ..
            } if update_name == acc_name => Some((update_op.clone(), accumulator_operand(value)?)),
            lucid_syntax::Stmt::Assignment {
                target:
                    lucid_syntax::Expr::Ident {
                        name: update_name, ..
                    },
                value:
                    lucid_syntax::Expr::Binary {
                        op: update_op @ (lucid_syntax::BinaryOp::Add | lucid_syntax::BinaryOp::Sub),
                        left,
                        right,
                        ..
                    },
                ..
            } => {
                let ordinary_update =
                    matches!(left.as_ref(), lucid_syntax::Expr::Ident { name, .. } if name == acc_name)
                        .then(|| accumulator_operand(right.as_ref()))
                        .flatten();
                let commuted_add = *update_op == lucid_syntax::BinaryOp::Add
                    && matches!(right.as_ref(), lucid_syntax::Expr::Ident { name, .. } if name == acc_name);
                let commuted_update = commuted_add
                    .then(|| accumulator_operand(left.as_ref()))
                    .flatten();
                if update_name == acc_name {
                    ordinary_update
                        .or(commuted_update)
                        .map(|operand| (update_op.clone(), operand))
                } else {
                    None
                }
            }
            _ => None,
        };
        let (accumulator_update, accumulator_operand) = accumulator_update?;
        let zero = lucid_syntax::Expr::Literal {
            value: lucid_syntax::LiteralValue::Int(0),
            span: func.span(),
        };
        let (start_expr, stop_expr, step) = match args.as_slice() {
            [stop] => (&zero, &stop.value, 1),
            [start, stop] => (&start.value, &stop.value, 1),
            [start, stop, step] => {
                let step = Self::int_literal_expr(&step.value)?;
                if step == 0 {
                    return None;
                }
                (&start.value, &stop.value, step)
            }
            _ => return None,
        };
        fn bound_alias_value<'a>(
            name: &str,
            bound_aliases: &[&'a lucid_syntax::Stmt],
        ) -> Option<&'a lucid_syntax::Expr> {
            for statement in bound_aliases {
                let (alias_name, value) = initialized_ident(statement)?;
                if alias_name == name {
                    return Some(value);
                }
            }
            None
        }
        fn expr_uses_bound_alias(
            expr: &lucid_syntax::Expr,
            alias: &str,
            bound_aliases: &[&lucid_syntax::Stmt],
            depth: usize,
        ) -> Option<bool> {
            if depth > bound_aliases.len() {
                return Some(false);
            }
            let lucid_syntax::Expr::Ident { name, .. } = expr else {
                return Some(false);
            };
            if name == alias {
                return Some(true);
            }
            match bound_alias_value(name, bound_aliases) {
                Some(value) => expr_uses_bound_alias(value, alias, bound_aliases, depth + 1),
                None => Some(false),
            }
        }
        fn operand(
            expr: &lucid_syntax::Expr,
            result: ValueId,
            bound_aliases: &[&lucid_syntax::Stmt],
            parameter_names: &[String],
            depth: usize,
        ) -> Option<Instruction> {
            if depth > bound_aliases.len() {
                return None;
            }
            match Function::int_literal_expr(expr) {
                Some(value) => Some(Instruction::ConstInt { result, value }),
                None => match expr {
                    lucid_syntax::Expr::Ident { name, .. } => {
                        if let Some(value) = bound_alias_value(name, bound_aliases) {
                            operand(value, result, bound_aliases, parameter_names, depth + 1)
                        } else {
                            Some(Instruction::Param {
                                result,
                                index: parameter_names
                                    .iter()
                                    .position(|parameter| parameter == name)?
                                    as u32,
                            })
                        }
                    }
                    _ => None,
                },
            }
        }
        for statement in &bound_aliases {
            let (alias_name, _) = initialized_ident(statement)?;
            if !expr_uses_bound_alias(start_expr, alias_name, &bound_aliases, 0)?
                && !expr_uses_bound_alias(stop_expr, alias_name, &bound_aliases, 0)?
            {
                return None;
            }
        }
        let start_instruction =
            operand(start_expr, ValueId(1), &bound_aliases, parameter_names, 0)?;
        let stop_instruction = operand(stop_expr, ValueId(0), &bound_aliases, parameter_names, 0)?;
        let accumulator_instruction =
            operand(initial_expr, ValueId(2), &bound_aliases, parameter_names, 0)?;
        let accumulator_operand_value = match accumulator_operand {
            RangeAccumulatorOperand::Literal(_) => ValueId(9),
            RangeAccumulatorOperand::Induction => ValueId(3),
        };
        let accumulator_update_instruction = match accumulator_update {
            lucid_syntax::BinaryOp::Add => Instruction::Add {
                result: ValueId(6),
                left: ValueId(4),
                right: accumulator_operand_value,
            },
            lucid_syntax::BinaryOp::Sub => Instruction::Sub {
                result: ValueId(6),
                left: ValueId(4),
                right: accumulator_operand_value,
            },
            _ => return None,
        };
        let mut body_instructions = Vec::new();
        if let RangeAccumulatorOperand::Literal(value) = accumulator_operand {
            body_instructions.push(Instruction::ConstInt {
                result: ValueId(9),
                value,
            });
        }
        body_instructions.push(accumulator_update_instruction);
        body_instructions.push(Instruction::ConstInt {
            result: ValueId(8),
            value: step,
        });
        body_instructions.push(Instruction::Add {
            result: ValueId(7),
            left: ValueId(3),
            right: ValueId(8),
        });
        let function = Self {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: vec![
                        stop_instruction,
                        start_instruction,
                        accumulator_instruction,
                    ],
                    terminator: Terminator::Jump(BlockId(1)),
                },
                Block {
                    id: BlockId(1),
                    instructions: vec![
                        Instruction::Phi {
                            result: ValueId(3),
                            incomings: vec![(BlockId(0), ValueId(1)), (BlockId(2), ValueId(7))],
                        },
                        Instruction::Phi {
                            result: ValueId(4),
                            incomings: vec![(BlockId(0), ValueId(2)), (BlockId(2), ValueId(6))],
                        },
                        if step > 0 {
                            Instruction::CmpLt {
                                result: ValueId(5),
                                left: ValueId(3),
                                right: ValueId(0),
                            }
                        } else {
                            Instruction::CmpGt {
                                result: ValueId(5),
                                left: ValueId(3),
                                right: ValueId(0),
                            }
                        },
                    ],
                    terminator: Terminator::Branch {
                        condition: ValueId(5),
                        then_block: BlockId(2),
                        else_block: BlockId(3),
                    },
                },
                Block {
                    id: BlockId(2),
                    instructions: body_instructions,
                    terminator: Terminator::Jump(BlockId(1)),
                },
                Block {
                    id: BlockId(3),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(Some(ValueId(4))),
                },
            ],
        };
        Some(
            function
                .verify()
                .map(|_| function)
                .map_err(|_| LowerError::UnsupportedExpression),
        )
    }

    /// Lower a straight-line module, retaining bindings between assignments.
    /// This is the first statement-level lowering boundary: control-flow
    /// statements still belong to the structured lowering pass, but ordinary
    /// declarations no longer collapse to the first assignment in a file.
    pub fn from_module_linear(module: &lucid_syntax::Module) -> Result<Self, LowerError> {
        Self::from_module_linear_with_params(module, &[])
    }

    /// Lower a straight-line module with positional parameter bindings.
    /// Parameter-free module initializers use [`Self::from_module_linear`];
    /// function lowering supplies the names here so constant-loop bodies can
    /// reuse the same statement and expression implementation.
    pub fn from_module_linear_with_params(
        module: &lucid_syntax::Module,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        // Lower the canonical counted-loop form directly to a back-edge CFG.
        // This is deliberately structural: the loop value is a header Phi,
        // so both the interpreter and native backend observe the same SSA
        // semantics instead of receiving an eagerly-unrolled approximation.
        if let Some(function) = Self::from_counted_while_accumulator(module, parameter_names) {
            return function;
        }
        if let Some(function) = Self::from_counted_while(module, parameter_names) {
            return function;
        }
        if let Some(function) = Self::from_counted_for(module, parameter_names) {
            return function;
        }
        // Expression lowering owns short-circuit CFGs. Keep a standalone
        // initializer containing `and`/`or` on that path instead of using the
        // eager boolean instructions used for already-materialized statement
        // conditions below.
        fn contains_logical(expr: &lucid_syntax::Expr) -> bool {
            match expr {
                lucid_syntax::Expr::Binary {
                    op: lucid_syntax::BinaryOp::And | lucid_syntax::BinaryOp::Or,
                    ..
                } => true,
                lucid_syntax::Expr::Binary { left, right, .. } => {
                    contains_logical(left) || contains_logical(right)
                }
                lucid_syntax::Expr::Unary { expr, .. } => contains_logical(expr),
                lucid_syntax::Expr::IfExpr {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    contains_logical(condition)
                        || contains_logical(then_branch)
                        || contains_logical(else_branch)
                }
                _ => false,
            }
        }
        if let [lucid_syntax::Stmt::Assignment { value, .. }
        | lucid_syntax::Stmt::VarDef {
            value: Some(value), ..
        }] = module.statements.as_slice()
        {
            if contains_logical(value) {
                return Err(LowerError::UnsupportedExpression);
            }
        }
        if let [lucid_syntax::Stmt::If {
            condition: lucid_syntax::Expr::Binary { op, .. },
            ..
        }] = module.statements.as_slice()
        {
            if !matches!(op, lucid_syntax::BinaryOp::And | lucid_syntax::BinaryOp::Or) {
                return Self::from_module_if(module);
            }
        }
        if let [lucid_syntax::Stmt::If {
            condition:
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Not,
                    expr,
                    ..
                },
            ..
        }] = module.statements.as_slice()
        {
            if matches!(expr.as_ref(), lucid_syntax::Expr::Binary { .. }) {
                return Self::from_module_if(module);
            }
        }
        if let [lucid_syntax::Stmt::Export(inner)] = module.statements.as_slice() {
            if matches!(inner.as_ref(), lucid_syntax::Stmt::If { condition: lucid_syntax::Expr::Binary { op, .. }, .. } if !matches!(op, lucid_syntax::BinaryOp::And | lucid_syntax::BinaryOp::Or))
            {
                let unwrapped = lucid_syntax::Module {
                    statements: vec![inner.as_ref().clone()],
                    span: module.span,
                };
                return Self::from_module_if(&unwrapped);
            }
        }
        use std::collections::HashMap;
        let mut instructions = Vec::new();
        let mut bindings = HashMap::<String, ValueId>::new();
        let mut next = 0u32;
        for (index, name) in parameter_names.iter().enumerate() {
            let result = ValueId(next);
            next = next
                .checked_add(1)
                .ok_or(LowerError::UnsupportedExpression)?;
            instructions.push(Instruction::Param {
                result,
                index: u32::try_from(index).map_err(|_| LowerError::UnsupportedExpression)?,
            });
            bindings.insert(name.clone(), result);
        }
        fn lower(
            expr: &lucid_syntax::Expr,
            bindings: &HashMap<String, ValueId>,
            instructions: &mut Vec<Instruction>,
            next: &mut u32,
        ) -> Result<ValueId, LowerError> {
            let result = |next: &mut u32| {
                let value = ValueId(*next);
                *next += 1;
                value
            };
            match expr {
                lucid_syntax::Expr::Ident { name, .. } => bindings
                    .get(name)
                    .copied()
                    .ok_or(LowerError::UnsupportedExpression),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Int(value),
                    ..
                } => {
                    let id = result(next);
                    instructions.push(Instruction::ConstInt {
                        result: id,
                        value: *value,
                    });
                    Ok(id)
                }
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Bool(value),
                    ..
                } => {
                    let id = result(next);
                    instructions.push(Instruction::ConstBool {
                        result: id,
                        value: *value,
                    });
                    Ok(id)
                }
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Pos,
                    expr,
                    ..
                } => lower(expr, bindings, instructions, next),
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Neg,
                    expr,
                    ..
                } => {
                    let operand = lower(expr, bindings, instructions, next)?;
                    let id = result(next);
                    instructions.push(Instruction::Neg {
                        result: id,
                        operand,
                    });
                    Ok(id)
                }
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Invert,
                    expr,
                    ..
                } => {
                    let operand = lower(expr, bindings, instructions, next)?;
                    let id = result(next);
                    instructions.push(Instruction::BitNot {
                        result: id,
                        operand,
                    });
                    Ok(id)
                }
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Not,
                    expr,
                    ..
                } => {
                    let operand = lower(expr, bindings, instructions, next)?;
                    let id = result(next);
                    instructions.push(Instruction::Not {
                        result: id,
                        operand,
                    });
                    Ok(id)
                }
                lucid_syntax::Expr::Binary {
                    left, op, right, ..
                } => {
                    let left = lower(left, bindings, instructions, next)?;
                    let right = lower(right, bindings, instructions, next)?;
                    let id = result(next);
                    let instruction = match op {
                        lucid_syntax::BinaryOp::Add => Instruction::Add {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Sub => Instruction::Sub {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Mul => Instruction::Mul {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Pow => Instruction::Pow {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Div => Instruction::Div {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::FloorDiv => Instruction::FloorDiv {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Mod => Instruction::Mod {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::BitAnd => Instruction::BitAnd {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::BitOr => Instruction::BitOr {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::BitXor => Instruction::BitXor {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Shl => Instruction::Shl {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Shr => Instruction::Shr {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Eq
                        | lucid_syntax::BinaryOp::Identity
                        | lucid_syntax::BinaryOp::Is => Instruction::CmpEq {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::NotEq
                        | lucid_syntax::BinaryOp::NotIdentity
                        | lucid_syntax::BinaryOp::IsNot => Instruction::CmpNe {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::LtEq => Instruction::CmpLe {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Gt => Instruction::CmpGt {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::GtEq => Instruction::CmpGe {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Lt => Instruction::CmpLt {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::And => Instruction::And {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Or => Instruction::Or {
                            result: id,
                            left,
                            right,
                        },
                        _ => return Err(LowerError::UnsupportedExpression),
                    };
                    instructions.push(instruction);
                    Ok(id)
                }
                _ => Err(LowerError::UnsupportedExpression),
            }
        }
        let mut last = None;
        fn visit_all(
            statements: &[lucid_syntax::Stmt],
            bindings: &mut HashMap<String, ValueId>,
            instructions: &mut Vec<Instruction>,
            next: &mut u32,
            last: &mut Option<ValueId>,
        ) -> Result<(), LowerError> {
            for statement in statements {
                visit(statement, bindings, instructions, next, last)?;
            }
            Ok(())
        }
        fn contains_return(statements: &[lucid_syntax::Stmt]) -> bool {
            statements.iter().any(|statement| match statement {
                lucid_syntax::Stmt::Return { .. } => true,
                lucid_syntax::Stmt::If {
                    then_branch,
                    elif_branches,
                    else_branch,
                    ..
                } => {
                    contains_return(then_branch)
                        || elif_branches
                            .iter()
                            .any(|(_, branch)| contains_return(branch))
                        || else_branch.as_deref().is_some_and(contains_return)
                }
                lucid_syntax::Stmt::For {
                    body, if_broken, ..
                }
                | lucid_syntax::Stmt::While {
                    body, if_broken, ..
                } => contains_return(body) || if_broken.as_deref().is_some_and(contains_return),
                lucid_syntax::Stmt::With { body, .. } => contains_return(body),
                _ => false,
            })
        }
        fn literal_int(expr: &lucid_syntax::Expr) -> Option<i64> {
            match expr {
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Int(value),
                    ..
                } => Some(*value),
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Neg,
                    expr,
                    ..
                } => literal_int(expr)?.checked_neg(),
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Pos,
                    expr,
                    ..
                } => literal_int(expr),
                _ => None,
            }
        }
        fn literal_bool(expr: &lucid_syntax::Expr) -> Option<bool> {
            match expr {
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Bool(value),
                    ..
                } => Some(*value),
                _ => None,
            }
        }
        fn constant_truth(expr: &lucid_syntax::Expr) -> Option<bool> {
            match expr {
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Bool(value),
                    ..
                } => Some(*value),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Int(value),
                    ..
                } => Some(*value != 0),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Str(value),
                    ..
                } => Some(!value.is_empty()),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::None,
                    ..
                } => Some(false),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::BigInt(value),
                    ..
                } => {
                    let digits = value.trim().trim_start_matches(['+', '-']).replace('_', "");
                    let (digits, radix) = if let Some(value) = digits
                        .strip_prefix("0x")
                        .or_else(|| digits.strip_prefix("0X"))
                    {
                        (value, 16)
                    } else if let Some(value) = digits
                        .strip_prefix("0o")
                        .or_else(|| digits.strip_prefix("0O"))
                    {
                        (value, 8)
                    } else if let Some(value) = digits
                        .strip_prefix("0b")
                        .or_else(|| digits.strip_prefix("0B"))
                    {
                        (value, 2)
                    } else {
                        (digits.as_str(), 10)
                    };
                    Some(digits.chars().any(|digit| digit.to_digit(radix) != Some(0)))
                }
                lucid_syntax::Expr::Literal {
                    value:
                        lucid_syntax::LiteralValue::Sentinel(_) | lucid_syntax::LiteralValue::Ellipsis,
                    ..
                } => Some(true),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Float(value),
                    ..
                } => Some(*value != 0.0),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Complex(value),
                    ..
                } => Some(*value != 0.0),
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Not,
                    expr,
                    ..
                } => constant_truth(expr).map(|value| !value),
                lucid_syntax::Expr::Binary {
                    op: lucid_syntax::BinaryOp::And,
                    left,
                    right,
                    ..
                } => match constant_truth(left) {
                    Some(false) => Some(false),
                    Some(true) => constant_truth(right),
                    None => None,
                },
                lucid_syntax::Expr::Binary {
                    op: lucid_syntax::BinaryOp::Or,
                    left,
                    right,
                    ..
                } => match constant_truth(left) {
                    Some(true) => Some(true),
                    Some(false) => constant_truth(right),
                    None => None,
                },
                lucid_syntax::Expr::Binary {
                    op, left, right, ..
                } => {
                    let compare = |left: f64, right: f64| match op {
                        lucid_syntax::BinaryOp::Eq
                        | lucid_syntax::BinaryOp::Identity
                        | lucid_syntax::BinaryOp::Is => Some(left == right),
                        lucid_syntax::BinaryOp::NotEq
                        | lucid_syntax::BinaryOp::NotIdentity
                        | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                        lucid_syntax::BinaryOp::Lt => Some(left < right),
                        lucid_syntax::BinaryOp::LtEq => Some(left <= right),
                        lucid_syntax::BinaryOp::Gt => Some(left > right),
                        lucid_syntax::BinaryOp::GtEq => Some(left >= right),
                        _ => None,
                    };
                    match (left.as_ref(), right.as_ref()) {
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Str(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Str(right),
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(left == right),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::None,
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::None,
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(true),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(false),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Sentinel(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Sentinel(right),
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(left == right),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Ellipsis,
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Ellipsis,
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(true),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(false),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::BigInt(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::BigInt(right),
                                ..
                            },
                        ) => {
                            let normalize = |value: &str| {
                                let value = value.trim().replace('_', "");
                                let negative = value.starts_with('-');
                                let digits =
                                    value.trim_start_matches(['+', '-']).trim_start_matches('0');
                                let digits = if digits.is_empty() { "0" } else { digits };
                                if negative && digits != "0" {
                                    format!("-{digits}")
                                } else {
                                    digits.to_string()
                                }
                            };
                            let equal = normalize(left) == normalize(right);
                            match op {
                                lucid_syntax::BinaryOp::Eq
                                | lucid_syntax::BinaryOp::Identity
                                | lucid_syntax::BinaryOp::Is => Some(equal),
                                lucid_syntax::BinaryOp::NotEq
                                | lucid_syntax::BinaryOp::NotIdentity
                                | lucid_syntax::BinaryOp::IsNot => Some(!equal),
                                _ => None,
                            }
                        }
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Bool(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Bool(right),
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(left == right),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Int(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Int(right),
                                ..
                            },
                        ) => compare(*left as f64, *right as f64),
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Float(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Float(right),
                                ..
                            },
                        ) => compare(*left, *right),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        fn instruction_value(instruction: &Instruction) -> ValueId {
            match instruction {
                Instruction::Param { result, .. }
                | Instruction::ConstInt { result, .. }
                | Instruction::ConstBool { result, .. }
                | Instruction::Add { result, .. }
                | Instruction::Sub { result, .. }
                | Instruction::Mul { result, .. }
                | Instruction::Pow { result, .. }
                | Instruction::Div { result, .. }
                | Instruction::FloorDiv { result, .. }
                | Instruction::Mod { result, .. }
                | Instruction::BitAnd { result, .. }
                | Instruction::BitOr { result, .. }
                | Instruction::BitXor { result, .. }
                | Instruction::Shl { result, .. }
                | Instruction::Shr { result, .. }
                | Instruction::CmpEq { result, .. }
                | Instruction::CmpNe { result, .. }
                | Instruction::CmpLe { result, .. }
                | Instruction::CmpGt { result, .. }
                | Instruction::CmpGe { result, .. }
                | Instruction::CmpLt { result, .. }
                | Instruction::Neg { result, .. }
                | Instruction::BitNot { result, .. }
                | Instruction::Not { result, .. }
                | Instruction::And { result, .. }
                | Instruction::Or { result, .. }
                | Instruction::Phi { result, .. } => *result,
            }
        }
        fn constant_int(value: ValueId, instructions: &[Instruction]) -> Option<i64> {
            let instruction = instructions
                .iter()
                .find(|instruction| instruction_value(instruction) == value)?;
            match instruction {
                Instruction::ConstInt { value, .. } => Some(*value),
                Instruction::ConstBool { value, .. } => Some(i64::from(*value)),
                Instruction::Add { left, right, .. } => constant_int(*left, instructions)?
                    .checked_add(constant_int(*right, instructions)?),
                Instruction::Sub { left, right, .. } => constant_int(*left, instructions)?
                    .checked_sub(constant_int(*right, instructions)?),
                Instruction::Mul { left, right, .. } => constant_int(*left, instructions)?
                    .checked_mul(constant_int(*right, instructions)?),
                Instruction::Div { left, right, .. } => {
                    let left = constant_int(*left, instructions)?;
                    let right = constant_int(*right, instructions)?;
                    (right != 0).then(|| left.checked_div(right)).flatten()
                }
                Instruction::FloorDiv { left, right, .. } => {
                    let left = constant_int(*left, instructions)?;
                    let right = constant_int(*right, instructions)?;
                    (right != 0)
                        .then(|| left.checked_div_euclid(right))
                        .flatten()
                }
                Instruction::Mod { left, right, .. } => {
                    let left = constant_int(*left, instructions)?;
                    let right = constant_int(*right, instructions)?;
                    (right != 0)
                        .then(|| left.checked_rem_euclid(right))
                        .flatten()
                }
                Instruction::Neg { operand, .. } => {
                    constant_int(*operand, instructions)?.checked_neg()
                }
                Instruction::Pow { left, right, .. } => {
                    u32::try_from(constant_int(*right, instructions)?)
                        .ok()
                        .and_then(|exponent| {
                            constant_int(*left, instructions)?.checked_pow(exponent)
                        })
                }
                Instruction::BitAnd { left, right, .. } => {
                    Some(constant_int(*left, instructions)? & constant_int(*right, instructions)?)
                }
                Instruction::BitOr { left, right, .. } => {
                    Some(constant_int(*left, instructions)? | constant_int(*right, instructions)?)
                }
                Instruction::BitXor { left, right, .. } => {
                    Some(constant_int(*left, instructions)? ^ constant_int(*right, instructions)?)
                }
                Instruction::BitNot { operand, .. } => Some(!constant_int(*operand, instructions)?),
                Instruction::Shl { left, right, .. } => {
                    let shift = u32::try_from(constant_int(*right, instructions)?).ok()?;
                    (shift < 64)
                        .then(|| constant_int(*left, instructions)?.checked_shl(shift))
                        .flatten()
                }
                Instruction::Shr { left, right, .. } => {
                    let shift = u32::try_from(constant_int(*right, instructions)?).ok()?;
                    (shift < 64)
                        .then(|| Some(constant_int(*left, instructions)? >> shift))
                        .flatten()
                }
                _ => None,
            }
        }
        fn constant_value_truth(value: ValueId, instructions: &[Instruction]) -> Option<bool> {
            let instruction = instructions
                .iter()
                .find(|instruction| instruction_value(instruction) == value)?;
            match instruction {
                Instruction::ConstBool { value, .. } => Some(*value),
                Instruction::ConstInt { value, .. } => Some(*value != 0),
                Instruction::CmpEq { left, right, .. } => {
                    Some(constant_int(*left, instructions)? == constant_int(*right, instructions)?)
                }
                Instruction::CmpNe { left, right, .. } => {
                    Some(constant_int(*left, instructions)? != constant_int(*right, instructions)?)
                }
                Instruction::CmpLe { left, right, .. } => {
                    Some(constant_int(*left, instructions)? <= constant_int(*right, instructions)?)
                }
                Instruction::CmpLt { left, right, .. } => {
                    Some(constant_int(*left, instructions)? < constant_int(*right, instructions)?)
                }
                Instruction::CmpGe { left, right, .. } => {
                    Some(constant_int(*left, instructions)? >= constant_int(*right, instructions)?)
                }
                Instruction::CmpGt { left, right, .. } => {
                    Some(constant_int(*left, instructions)? > constant_int(*right, instructions)?)
                }
                Instruction::Not { operand, .. } => {
                    Some(!constant_value_truth(*operand, instructions)?)
                }
                Instruction::And { left, right, .. } => Some(
                    constant_value_truth(*left, instructions)?
                        && constant_value_truth(*right, instructions)?,
                ),
                Instruction::Or { left, right, .. } => Some(
                    constant_value_truth(*left, instructions)?
                        || constant_value_truth(*right, instructions)?,
                ),
                _ => constant_int(value, instructions).map(|value| value != 0),
            }
        }
        fn statement_static_truth(
            expr: &lucid_syntax::Expr,
            bindings: &HashMap<String, ValueId>,
            instructions: &[Instruction],
        ) -> Option<bool> {
            match expr {
                lucid_syntax::Expr::Ident { name, .. } => {
                    constant_value_truth(*bindings.get(name)?, instructions)
                }
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Not,
                    expr,
                    ..
                } => statement_static_truth(expr, bindings, instructions).map(|value| !value),
                lucid_syntax::Expr::Binary {
                    op: lucid_syntax::BinaryOp::And,
                    left,
                    right,
                    ..
                } => match statement_static_truth(left, bindings, instructions) {
                    Some(false) => Some(false),
                    Some(true) => statement_static_truth(right, bindings, instructions),
                    None => None,
                },
                lucid_syntax::Expr::Binary {
                    op: lucid_syntax::BinaryOp::Or,
                    left,
                    right,
                    ..
                } => match statement_static_truth(left, bindings, instructions) {
                    Some(true) => Some(true),
                    Some(false) => statement_static_truth(right, bindings, instructions),
                    None => None,
                },
                _ => constant_truth(expr),
            }
        }
        fn visit(
            stmt: &lucid_syntax::Stmt,
            bindings: &mut HashMap<String, ValueId>,
            instructions: &mut Vec<Instruction>,
            next: &mut u32,
            last: &mut Option<ValueId>,
        ) -> Result<(), LowerError> {
            match stmt {
                lucid_syntax::Stmt::Export(inner) => {
                    visit(inner, bindings, instructions, next, last)
                }
                lucid_syntax::Stmt::Assignment {
                    target: lucid_syntax::Expr::Ident { name, .. },
                    value,
                    ..
                } => {
                    let id = lower(value, bindings, instructions, next)?;
                    bindings.insert(name.clone(), id);
                    *last = Some(id);
                    Ok(())
                }
                lucid_syntax::Stmt::AugAssign {
                    target: lucid_syntax::Expr::Ident { name, .. },
                    op,
                    value,
                    ..
                } => {
                    let left = bindings
                        .get(name)
                        .copied()
                        .ok_or(LowerError::UnsupportedExpression)?;
                    let right = lower(value, bindings, instructions, next)?;
                    let id = ValueId(*next);
                    *next += 1;
                    let instruction = match op {
                        lucid_syntax::BinaryOp::Add => Instruction::Add {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Sub => Instruction::Sub {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Mul => Instruction::Mul {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Pow => Instruction::Pow {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Div => Instruction::Div {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::FloorDiv => Instruction::FloorDiv {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Mod => Instruction::Mod {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::BitAnd => Instruction::BitAnd {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::BitOr => Instruction::BitOr {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::BitXor => Instruction::BitXor {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Shl => Instruction::Shl {
                            result: id,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Shr => Instruction::Shr {
                            result: id,
                            left,
                            right,
                        },
                        _ => return Err(LowerError::UnsupportedExpression),
                    };
                    instructions.push(instruction);
                    bindings.insert(name.clone(), id);
                    *last = Some(id);
                    Ok(())
                }
                lucid_syntax::Stmt::Return {
                    value: Some(value), ..
                } => {
                    // A return is a valid statement-level terminator for the
                    // current single-block CIR subset. Lowering it here
                    // preserves the value instead of requiring callers to
                    // manufacture a throwaway assignment first.
                    let id = lower(value, bindings, instructions, next)?;
                    *last = Some(id);
                    Ok(())
                }
                lucid_syntax::Stmt::Return { value: None, .. } => {
                    // An explicit bare return terminates the value channel.
                    // Clear `last` rather than leaving the value produced by
                    // an earlier binding as the function result.  The
                    // statement subset has no separate terminator while it
                    // is being collected, so this marker is consumed by the
                    // final `Return(last)` below.
                    *last = None;
                    Ok(())
                }
                lucid_syntax::Stmt::VarDef {
                    pattern: lucid_syntax::Pattern::Ident(name, _),
                    value: Some(value),
                    ..
                } => {
                    let id = lower(value, bindings, instructions, next)?;
                    bindings.insert(name.clone(), id);
                    *last = Some(id);
                    Ok(())
                }
                lucid_syntax::Stmt::If {
                    condition: lucid_syntax::Expr::Ident { name, .. },
                    then_branch,
                    elif_branches,
                    else_branch,
                    ..
                } => {
                    let binding = bindings
                        .get(name)
                        .copied()
                        .ok_or(LowerError::UnsupportedExpression)?;
                    let truth = constant_value_truth(binding, instructions)
                        .ok_or(LowerError::UnsupportedExpression)?;
                    if truth {
                        return visit_all(then_branch, bindings, instructions, next, last);
                    }
                    for (condition, branch) in elif_branches {
                        match statement_static_truth(condition, bindings, instructions) {
                            Some(true) => {
                                return visit_all(branch, bindings, instructions, next, last);
                            }
                            Some(false) => continue,
                            None => return Err(LowerError::UnsupportedExpression),
                        }
                    }
                    if let Some(branch) = else_branch {
                        visit_all(branch, bindings, instructions, next, last)
                    } else {
                        Ok(())
                    }
                }
                lucid_syntax::Stmt::If {
                    condition:
                        lucid_syntax::Expr::Unary {
                            op: lucid_syntax::UnaryOp::Not,
                            expr,
                            ..
                        },
                    then_branch,
                    elif_branches,
                    else_branch,
                    ..
                } => {
                    let truth = !statement_static_truth(expr, bindings, instructions)
                        .ok_or(LowerError::UnsupportedExpression)?;
                    if truth {
                        return visit_all(then_branch, bindings, instructions, next, last);
                    }
                    for (condition, branch) in elif_branches {
                        match statement_static_truth(condition, bindings, instructions) {
                            Some(true) => {
                                return visit_all(branch, bindings, instructions, next, last);
                            }
                            Some(false) => continue,
                            None => return Err(LowerError::UnsupportedExpression),
                        }
                    }
                    if let Some(branch) = else_branch {
                        visit_all(branch, bindings, instructions, next, last)
                    } else {
                        Ok(())
                    }
                }
                lucid_syntax::Stmt::If {
                    condition:
                        condition @ lucid_syntax::Expr::Binary {
                            op: lucid_syntax::BinaryOp::And | lucid_syntax::BinaryOp::Or,
                            ..
                        },
                    then_branch,
                    elif_branches,
                    else_branch,
                    ..
                } => match statement_static_truth(condition, bindings, instructions) {
                    Some(true) => visit_all(then_branch, bindings, instructions, next, last),
                    Some(false) => {
                        for (condition, branch) in elif_branches {
                            match statement_static_truth(condition, bindings, instructions) {
                                Some(true) => {
                                    return visit_all(branch, bindings, instructions, next, last);
                                }
                                Some(false) => continue,
                                None => return Err(LowerError::UnsupportedExpression),
                            }
                        }
                        if let Some(branch) = else_branch {
                            visit_all(branch, bindings, instructions, next, last)
                        } else {
                            Ok(())
                        }
                    }
                    None => Err(LowerError::UnsupportedExpression),
                },
                lucid_syntax::Stmt::If {
                    condition: condition @ lucid_syntax::Expr::Binary { .. },
                    then_branch,
                    elif_branches,
                    else_branch,
                    ..
                } => match statement_static_truth(condition, bindings, instructions) {
                    Some(true) => visit_all(then_branch, bindings, instructions, next, last),
                    Some(false) => {
                        for (condition, branch) in elif_branches {
                            match statement_static_truth(condition, bindings, instructions) {
                                Some(true) => {
                                    return visit_all(branch, bindings, instructions, next, last);
                                }
                                Some(false) => continue,
                                None => return Err(LowerError::UnsupportedExpression),
                            }
                        }
                        if let Some(branch) = else_branch {
                            visit_all(branch, bindings, instructions, next, last)
                        } else {
                            Ok(())
                        }
                    }
                    None => Err(LowerError::UnsupportedExpression),
                },
                lucid_syntax::Stmt::If {
                    condition: condition @ lucid_syntax::Expr::Literal { .. },
                    then_branch,
                    elif_branches,
                    else_branch,
                    ..
                } => {
                    let truth = statement_static_truth(condition, bindings, instructions)
                        .ok_or(LowerError::UnsupportedExpression)?;
                    if truth {
                        return visit_all(then_branch, bindings, instructions, next, last);
                    }
                    for (condition, branch) in elif_branches {
                        match statement_static_truth(condition, bindings, instructions) {
                            Some(true) => {
                                return visit_all(branch, bindings, instructions, next, last);
                            }
                            Some(false) => continue,
                            None => return Err(LowerError::UnsupportedExpression),
                        }
                    }
                    if let Some(branch) = else_branch {
                        visit_all(branch, bindings, instructions, next, last)
                    } else {
                        Ok(())
                    }
                }
                lucid_syntax::Stmt::While { condition, .. } => {
                    // A statically false loop has no effects and can be
                    // removed before single-block lowering. Statically true
                    // or dynamic loops need back-edges and remain outside
                    // this linear subset.
                    if statement_static_truth(condition, bindings, instructions) == Some(false) {
                        Ok(())
                    } else {
                        Err(LowerError::UnsupportedExpression)
                    }
                }
                lucid_syntax::Stmt::For {
                    target: lucid_syntax::Pattern::Ident(name, _),
                    iterable:
                        lucid_syntax::Expr::List { elements, .. }
                        | lucid_syntax::Expr::Set { elements, .. },
                    body,
                    ..
                } => {
                    if elements.is_empty() {
                        return Ok(());
                    }
                    if contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for element in elements {
                        let value = lower(element, bindings, instructions, next)?;
                        bindings.insert(name.clone(), value);
                        visit_all(body, bindings, instructions, next, last)?;
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target: lucid_syntax::Pattern::Wildcard(_),
                    iterable:
                        lucid_syntax::Expr::List { elements, .. }
                        | lucid_syntax::Expr::Set { elements, .. },
                    body,
                    ..
                } => {
                    if elements.is_empty() {
                        return Ok(());
                    }
                    if contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for element in elements {
                        let _ = lower(element, bindings, instructions, next)?;
                        visit_all(body, bindings, instructions, next, last)?;
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target:
                        lucid_syntax::Pattern::Literal(lucid_syntax::LiteralValue::Int(expected), _),
                    iterable:
                        lucid_syntax::Expr::List { elements, .. }
                        | lucid_syntax::Expr::Set { elements, .. },
                    body,
                    ..
                } => {
                    let has_match = elements
                        .iter()
                        .any(|element| literal_int(element) == Some(*expected));
                    if has_match && contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for element in elements {
                        let _ = lower(element, bindings, instructions, next)?;
                        if literal_int(element) == Some(*expected) {
                            visit_all(body, bindings, instructions, next, last)?;
                        }
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target:
                        lucid_syntax::Pattern::Literal(lucid_syntax::LiteralValue::Bool(expected), _),
                    iterable:
                        lucid_syntax::Expr::List { elements, .. }
                        | lucid_syntax::Expr::Set { elements, .. },
                    body,
                    ..
                } => {
                    let has_match = elements
                        .iter()
                        .any(|element| literal_bool(element) == Some(*expected));
                    if has_match && contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for element in elements {
                        let _ = lower(element, bindings, instructions, next)?;
                        if literal_bool(element) == Some(*expected) {
                            visit_all(body, bindings, instructions, next, last)?;
                        }
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target: lucid_syntax::Pattern::Ident(name, _),
                    iterable: lucid_syntax::Expr::Dict { entries, .. },
                    body,
                    ..
                } => {
                    if entries.is_empty() {
                        return Ok(());
                    }
                    if contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for (element, dict_value) in entries {
                        let key_value = lower(element, bindings, instructions, next)?;
                        let _ = lower(dict_value, bindings, instructions, next)?;
                        bindings.insert(name.clone(), key_value);
                        visit_all(body, bindings, instructions, next, last)?;
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target: lucid_syntax::Pattern::Wildcard(_),
                    iterable: lucid_syntax::Expr::Dict { entries, .. },
                    body,
                    ..
                } => {
                    if entries.is_empty() {
                        return Ok(());
                    }
                    if contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for (element, value) in entries {
                        let _ = lower(element, bindings, instructions, next)?;
                        let _ = lower(value, bindings, instructions, next)?;
                        visit_all(body, bindings, instructions, next, last)?;
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target:
                        lucid_syntax::Pattern::Literal(lucid_syntax::LiteralValue::Int(expected), _),
                    iterable: lucid_syntax::Expr::Dict { entries, .. },
                    body,
                    ..
                } => {
                    let has_match = entries
                        .iter()
                        .any(|(element, _)| literal_int(element) == Some(*expected));
                    if has_match && contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for (element, _) in entries {
                        let _ = lower(element, bindings, instructions, next)?;
                        if literal_int(element) == Some(*expected) {
                            visit_all(body, bindings, instructions, next, last)?;
                        }
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target:
                        lucid_syntax::Pattern::Literal(lucid_syntax::LiteralValue::Bool(expected), _),
                    iterable: lucid_syntax::Expr::Dict { entries, .. },
                    body,
                    ..
                } => {
                    let has_match = entries
                        .iter()
                        .any(|(element, _)| literal_bool(element) == Some(*expected));
                    if has_match && contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for (element, _) in entries {
                        let _ = lower(element, bindings, instructions, next)?;
                        if literal_bool(element) == Some(*expected) {
                            visit_all(body, bindings, instructions, next, last)?;
                        }
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target: lucid_syntax::Pattern::Ident(name, _),
                    iterable,
                    body,
                    ..
                } if const_range_values(iterable).is_some() => {
                    let values = const_range_values(iterable).unwrap_or_default();
                    if values.is_empty() {
                        return Ok(());
                    }
                    if contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for element in values {
                        let value = ValueId(*next);
                        *next += 1;
                        instructions.push(Instruction::ConstInt {
                            result: value,
                            value: element,
                        });
                        bindings.insert(name.clone(), value);
                        visit_all(body, bindings, instructions, next, last)?;
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target: lucid_syntax::Pattern::Wildcard(_),
                    iterable,
                    body,
                    ..
                } if const_range_values(iterable).is_some() => {
                    let values = const_range_values(iterable).unwrap_or_default();
                    if values.is_empty() {
                        return Ok(());
                    }
                    if contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for _ in values {
                        visit_all(body, bindings, instructions, next, last)?;
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For {
                    target:
                        lucid_syntax::Pattern::Literal(lucid_syntax::LiteralValue::Int(expected), _),
                    iterable,
                    body,
                    ..
                } if const_range_values(iterable).is_some() => {
                    let values = const_range_values(iterable).unwrap_or_default();
                    let has_match = values.iter().any(|element| element == expected);
                    if has_match && contains_return(body) {
                        return Err(LowerError::UnsupportedExpression);
                    }
                    for element in values {
                        if element == *expected {
                            visit_all(body, bindings, instructions, next, last)?;
                        }
                    }
                    Ok(())
                }
                lucid_syntax::Stmt::For { iterable, .. } => {
                    if is_const_empty_iterable(iterable) {
                        Ok(())
                    } else {
                        Err(LowerError::UnsupportedExpression)
                    }
                }
                lucid_syntax::Stmt::Expr(expr) => {
                    let _ = lower(expr, bindings, instructions, next)?;
                    Ok(())
                }
                lucid_syntax::Stmt::Assignment { .. }
                | lucid_syntax::Stmt::AugAssign { .. }
                | lucid_syntax::Stmt::VarDef { .. }
                | lucid_syntax::Stmt::If { .. }
                | lucid_syntax::Stmt::Match { .. }
                | lucid_syntax::Stmt::Try { .. }
                | lucid_syntax::Stmt::Raise { .. }
                | lucid_syntax::Stmt::Yield { .. }
                | lucid_syntax::Stmt::Delete { .. }
                | lucid_syntax::Stmt::Break(_)
                | lucid_syntax::Stmt::Continue(_)
                | lucid_syntax::Stmt::With { .. } => Err(LowerError::UnsupportedExpression),
                lucid_syntax::Stmt::Assert { condition, .. } => {
                    match statement_static_truth(condition, bindings, instructions) {
                        Some(true) => Ok(()),
                        _ => Err(LowerError::UnsupportedExpression),
                    }
                }
                lucid_syntax::Stmt::ClassDef { .. }
                | lucid_syntax::Stmt::InterfaceDef { .. }
                | lucid_syntax::Stmt::TraitDef { .. }
                | lucid_syntax::Stmt::ImplementDef { .. }
                | lucid_syntax::Stmt::TypeAlias { .. }
                | lucid_syntax::Stmt::Function(_)
                | lucid_syntax::Stmt::Import { .. }
                | lucid_syntax::Stmt::FromImport { .. }
                | lucid_syntax::Stmt::Pass(_) => Ok(()),
            }
        }

        // A dynamic conditional must remain a CFG diamond.  The previous
        // straight-line pass deliberately folded only conditions whose truth
        // value was known at compile time; handling a final `if` here keeps
        // bindings from the prefix visible to the condition while ensuring
        // that only the selected branch evaluates its initializer.
        fn statement_static_noop(
            statement: &lucid_syntax::Stmt,
            bindings: &HashMap<String, ValueId>,
            instructions: &[Instruction],
        ) -> bool {
            matches!(statement, lucid_syntax::Stmt::Pass(_))
                || matches!(
                    statement,
                    lucid_syntax::Stmt::Assert { condition, .. }
                        if statement_static_truth(condition, bindings, instructions) == Some(true)
                )
                || matches!(
                    statement,
                    lucid_syntax::Stmt::While { condition, .. }
                        if statement_static_truth(condition, bindings, instructions) == Some(false)
                )
                || matches!(
                    statement,
                    lucid_syntax::Stmt::For { iterable, .. }
                        if is_const_empty_iterable(iterable)
                )
        }
        struct LinearLoweringState<'a> {
            bindings: &'a mut HashMap<String, ValueId>,
            instructions: &'a mut Vec<Instruction>,
            next: &'a mut u32,
        }
        type DynamicElifArm<'a> = (&'a lucid_syntax::Expr, &'a [lucid_syntax::Stmt]);
        struct DynamicElifContinuation<'a> {
            condition: &'a lucid_syntax::Expr,
            then_branch: &'a [lucid_syntax::Stmt],
            elif_branches: Vec<DynamicElifArm<'a>>,
            else_branch: Option<&'a [lucid_syntax::Stmt]>,
            suffix: &'a [lucid_syntax::Stmt],
        }
        struct BranchLowering {
            bindings: HashMap<String, ValueId>,
            instructions: Vec<Instruction>,
            value: Option<ValueId>,
        }
        fn lower_dynamic_if(
            prefix: &[lucid_syntax::Stmt],
            condition: &lucid_syntax::Expr,
            then_branch: &[lucid_syntax::Stmt],
            else_branch: Option<&[lucid_syntax::Stmt]>,
            suffix: &[lucid_syntax::Stmt],
            state: &mut LinearLoweringState<'_>,
        ) -> Result<Function, LowerError> {
            let mut last = None;
            for statement in prefix {
                visit(
                    statement,
                    state.bindings,
                    state.instructions,
                    state.next,
                    &mut last,
                )?;
            }
            let fallthrough_value = last;
            let condition_value = lower(condition, state.bindings, state.instructions, state.next)?;
            let mut then_bindings = state.bindings.clone();
            let mut then_instructions = Vec::new();
            let mut then_last = fallthrough_value;
            let mut then_produced_value = false;
            for statement in then_branch {
                if statement_static_noop(statement, state.bindings, state.instructions) {
                    continue;
                }
                if matches!(statement, lucid_syntax::Stmt::Return { value: None, .. })
                    && then_produced_value
                {
                    continue;
                }
                visit(
                    statement,
                    &mut then_bindings,
                    &mut then_instructions,
                    state.next,
                    &mut then_last,
                )?;
                then_produced_value |= then_last.is_some();
            }
            let mut else_bindings = state.bindings.clone();
            let mut else_instructions = Vec::new();
            let mut else_last = fallthrough_value;
            let mut else_produced_value = false;
            if let Some(else_branch) = else_branch {
                for statement in else_branch {
                    if statement_static_noop(statement, state.bindings, state.instructions) {
                        continue;
                    }
                    if matches!(statement, lucid_syntax::Stmt::Return { value: None, .. })
                        && else_produced_value
                    {
                        continue;
                    }
                    visit(
                        statement,
                        &mut else_bindings,
                        &mut else_instructions,
                        state.next,
                        &mut else_last,
                    )?;
                    else_produced_value |= else_last.is_some();
                }
            }
            let (merge_instructions, terminator) = if suffix.is_empty() {
                let then_value = then_last.ok_or(LowerError::NoLowerableAssignment)?;
                let else_value = else_last.ok_or(LowerError::NoLowerableAssignment)?;
                let result = ValueId(*state.next);
                *state.next += 1;
                (
                    vec![Instruction::Phi {
                        result,
                        incomings: vec![(BlockId(1), then_value), (BlockId(2), else_value)],
                    }],
                    Terminator::Return(Some(result)),
                )
            } else {
                let branch_values = then_last.zip(else_last);
                let merged_name = branch_values.and_then(|(then_value, else_value)| {
                    then_bindings.iter().find_map(|(name, value)| {
                        (*value == then_value
                            && else_bindings.get(name).copied() == Some(else_value))
                        .then(|| (name.clone(), then_value, else_value))
                    })
                });
                let mut merge_bindings = state.bindings.clone();
                let has_phi = merged_name.is_some();
                let mut merge_instructions =
                    if let Some((merged_name, then_value, else_value)) = merged_name {
                        let result = ValueId(*state.next);
                        *state.next += 1;
                        merge_bindings.insert(merged_name, result);
                        vec![Instruction::Phi {
                            result,
                            incomings: vec![(BlockId(1), then_value), (BlockId(2), else_value)],
                        }]
                    } else {
                        Vec::new()
                    };
                let mut merge_last = if has_phi {
                    merge_instructions.last().map(instruction_value)
                } else {
                    fallthrough_value
                };
                visit_all(
                    suffix,
                    &mut merge_bindings,
                    &mut merge_instructions,
                    state.next,
                    &mut merge_last,
                )?;
                (merge_instructions, Terminator::Return(merge_last))
            };
            let function = Function {
                entry: BlockId(0),
                blocks: vec![
                    Block {
                        id: BlockId(0),
                        instructions: std::mem::take(state.instructions),
                        terminator: Terminator::Branch {
                            condition: condition_value,
                            then_block: BlockId(1),
                            else_block: BlockId(2),
                        },
                    },
                    Block {
                        id: BlockId(1),
                        instructions: then_instructions,
                        terminator: Terminator::Jump(BlockId(3)),
                    },
                    Block {
                        id: BlockId(2),
                        instructions: else_instructions,
                        terminator: Terminator::Jump(BlockId(3)),
                    },
                    Block {
                        id: BlockId(3),
                        instructions: merge_instructions,
                        terminator,
                    },
                ],
            };
            function
                .verify()
                .map_err(|_| LowerError::UnsupportedExpression)?;
            Ok(function)
        }

        fn lower_dynamic_if_elif_ladder(
            prefix: &[lucid_syntax::Stmt],
            ladder: DynamicElifContinuation<'_>,
            state: &mut LinearLoweringState<'_>,
        ) -> Result<Function, LowerError> {
            if ladder.elif_branches.is_empty() {
                return Err(LowerError::UnsupportedExpression);
            }
            fn lower_branch(
                branch: &[lucid_syntax::Stmt],
                base_bindings: &HashMap<String, ValueId>,
                base_instructions: &[Instruction],
                fallthrough_value: Option<ValueId>,
                next: &mut u32,
            ) -> Result<BranchLowering, LowerError> {
                let mut branch_bindings = base_bindings.clone();
                let mut branch_instructions = Vec::new();
                let mut branch_last = fallthrough_value;
                let mut produced_value = false;
                for statement in branch {
                    if statement_static_noop(statement, base_bindings, base_instructions) {
                        continue;
                    }
                    if matches!(statement, lucid_syntax::Stmt::Return { value: None, .. })
                        && produced_value
                    {
                        continue;
                    }
                    visit(
                        statement,
                        &mut branch_bindings,
                        &mut branch_instructions,
                        next,
                        &mut branch_last,
                    )?;
                    produced_value |= branch_last.is_some();
                }
                Ok(BranchLowering {
                    bindings: branch_bindings,
                    instructions: branch_instructions,
                    value: branch_last,
                })
            }

            let mut last = None;
            for statement in prefix {
                visit(
                    statement,
                    state.bindings,
                    state.instructions,
                    state.next,
                    &mut last,
                )?;
            }
            let fallthrough_value = last;
            let condition_value = lower(
                ladder.condition,
                state.bindings,
                state.instructions,
                state.next,
            )?;
            let base_bindings = state.bindings.clone();
            let base_instructions = state.instructions.clone();
            let then_lowering = lower_branch(
                ladder.then_branch,
                &base_bindings,
                &base_instructions,
                fallthrough_value,
                state.next,
            )?;
            let mut lowered_elifs = Vec::new();
            for (elif_condition, elif_branch) in &ladder.elif_branches {
                let mut condition_instructions = Vec::new();
                let condition_value = lower(
                    elif_condition,
                    &base_bindings,
                    &mut condition_instructions,
                    state.next,
                )?;
                let branch = lower_branch(
                    elif_branch,
                    &base_bindings,
                    &base_instructions,
                    fallthrough_value,
                    state.next,
                )?;
                lowered_elifs.push((condition_instructions, condition_value, branch));
            }
            let else_lowering = lower_branch(
                ladder.else_branch.unwrap_or(&[]),
                &base_bindings,
                &base_instructions,
                fallthrough_value,
                state.next,
            )?;
            let merged_name = then_lowering.value.and_then(|then_value| {
                let else_value = else_lowering.value?;
                then_lowering.bindings.iter().find_map(|(name, value)| {
                    if *value != then_value
                        || else_lowering.bindings.get(name).copied() != Some(else_value)
                    {
                        return None;
                    }
                    let mut elif_values = Vec::new();
                    for (_, _, branch) in &lowered_elifs {
                        let branch_value = branch.value?;
                        if branch.bindings.get(name).copied() != Some(branch_value) {
                            return None;
                        }
                        elif_values.push(branch_value);
                    }
                    Some((name.clone(), then_value, elif_values, else_value))
                })
            });
            let mut merge_bindings = base_bindings;
            let fallback_block = BlockId(
                2 + u32::try_from(ladder.elif_branches.len())
                    .map_err(|_| LowerError::UnsupportedExpression)?
                    * 2,
            );
            let merge_block = BlockId(fallback_block.0 + 1);
            let (mut merge_instructions, mut merge_last) =
                if let Some((merged_name, then_value, elif_values, else_value)) = merged_name {
                    let result = ValueId(*state.next);
                    *state.next += 1;
                    merge_bindings.insert(merged_name, result);
                    let mut incomings = vec![(BlockId(1), then_value)];
                    for (index, branch_value) in elif_values.into_iter().enumerate() {
                        let arm_block = BlockId(
                            3 + u32::try_from(index)
                                .map_err(|_| LowerError::UnsupportedExpression)?
                                * 2,
                        );
                        incomings.push((arm_block, branch_value));
                    }
                    incomings.push((fallback_block, else_value));
                    (vec![Instruction::Phi { result, incomings }], Some(result))
                } else {
                    (Vec::new(), fallthrough_value)
                };
            visit_all(
                ladder.suffix,
                &mut merge_bindings,
                &mut merge_instructions,
                state.next,
                &mut merge_last,
            )?;
            let first_elif_condition_block = BlockId(2);
            let mut blocks = vec![
                Block {
                    id: BlockId(0),
                    instructions: std::mem::take(state.instructions),
                    terminator: Terminator::Branch {
                        condition: condition_value,
                        then_block: BlockId(1),
                        else_block: first_elif_condition_block,
                    },
                },
                Block {
                    id: BlockId(1),
                    instructions: then_lowering.instructions,
                    terminator: Terminator::Jump(merge_block),
                },
            ];
            for (index, (condition_instructions, condition_value, branch)) in
                lowered_elifs.into_iter().enumerate()
            {
                let index = u32::try_from(index).map_err(|_| LowerError::UnsupportedExpression)?;
                let condition_block = BlockId(2 + index * 2);
                let arm_block = BlockId(3 + index * 2);
                let next_false_block = if usize::try_from(index + 1)
                    .map_err(|_| LowerError::UnsupportedExpression)?
                    < ladder.elif_branches.len()
                {
                    BlockId(2 + (index + 1) * 2)
                } else {
                    fallback_block
                };
                blocks.push(Block {
                    id: condition_block,
                    instructions: condition_instructions,
                    terminator: Terminator::Branch {
                        condition: condition_value,
                        then_block: arm_block,
                        else_block: next_false_block,
                    },
                });
                blocks.push(Block {
                    id: arm_block,
                    instructions: branch.instructions,
                    terminator: Terminator::Jump(merge_block),
                });
            }
            blocks.push(Block {
                id: fallback_block,
                instructions: else_lowering.instructions,
                terminator: Terminator::Jump(merge_block),
            });
            blocks.push(Block {
                id: merge_block,
                instructions: merge_instructions,
                terminator: Terminator::Return(merge_last),
            });
            let function = Function {
                entry: BlockId(0),
                blocks,
            };
            function
                .verify()
                .map_err(|_| LowerError::UnsupportedExpression)?;
            Ok(function)
        }

        for (index, statement) in module.statements.iter().enumerate() {
            if index + 1 == module.statements.len() {
                break;
            }
            let lucid_syntax::Stmt::If {
                condition,
                then_branch,
                else_branch,
                elif_branches,
                ..
            } = statement
            else {
                continue;
            };
            if constant_truth(condition).is_some() {
                continue;
            }
            let prefix = &module.statements[..index];
            let mut dynamic_elifs = Vec::new();
            let mut selected_static_else = else_branch.as_deref();
            for (elif_condition, elif_branch) in elif_branches {
                match constant_truth(elif_condition) {
                    Some(false) => continue,
                    Some(true) => {
                        selected_static_else = Some(elif_branch.as_slice());
                        break;
                    }
                    None => dynamic_elifs.push((elif_condition, elif_branch.as_slice())),
                }
            }
            if !dynamic_elifs.is_empty() {
                return lower_dynamic_if_elif_ladder(
                    prefix,
                    DynamicElifContinuation {
                        condition,
                        then_branch,
                        elif_branches: dynamic_elifs,
                        else_branch: selected_static_else,
                        suffix: &module.statements[index + 1..],
                    },
                    &mut LinearLoweringState {
                        bindings: &mut bindings,
                        instructions: &mut instructions,
                        next: &mut next,
                    },
                );
            }
            let mut probe_bindings = bindings.clone();
            let mut probe_instructions = instructions.clone();
            let mut probe_next = next;
            let mut probe_last = None;
            for statement in prefix {
                visit(
                    statement,
                    &mut probe_bindings,
                    &mut probe_instructions,
                    &mut probe_next,
                    &mut probe_last,
                )?;
            }
            let selected_else = if elif_branches.is_empty() {
                Some(else_branch.as_deref())
            } else {
                let mut selected = None;
                let mut statically_known = true;
                for (elif_condition, branch) in elif_branches {
                    match statement_static_truth(
                        elif_condition,
                        &probe_bindings,
                        &probe_instructions,
                    ) {
                        Some(true) => {
                            selected = Some(Some(branch.as_slice()));
                            break;
                        }
                        Some(false) => continue,
                        None => {
                            statically_known = false;
                            break;
                        }
                    }
                }
                if statically_known {
                    Some(selected.unwrap_or(else_branch.as_deref()))
                } else {
                    None
                }
            };
            if let Some(selected_else) = selected_else {
                return lower_dynamic_if(
                    prefix,
                    condition,
                    then_branch,
                    selected_else,
                    &module.statements[index + 1..],
                    &mut LinearLoweringState {
                        bindings: &mut bindings,
                        instructions: &mut instructions,
                        next: &mut next,
                    },
                );
            }
        }

        if let Some(lucid_syntax::Stmt::If {
            condition,
            then_branch,
            else_branch,
            elif_branches,
            ..
        }) = module.statements.last()
        {
            let prefix = &module.statements[..module.statements.len() - 1];
            let mut probe_bindings = bindings.clone();
            let mut probe_instructions = instructions.clone();
            let mut probe_next = next;
            let mut probe_last = None;
            for statement in prefix {
                visit(
                    statement,
                    &mut probe_bindings,
                    &mut probe_instructions,
                    &mut probe_next,
                    &mut probe_last,
                )?;
            }
            if constant_truth(condition).is_none() {
                let selected_else = if elif_branches.is_empty() {
                    Some(else_branch.as_deref())
                } else {
                    let mut selected = None;
                    let mut statically_known = true;
                    for (elif_condition, branch) in elif_branches {
                        match statement_static_truth(
                            elif_condition,
                            &probe_bindings,
                            &probe_instructions,
                        ) {
                            Some(true) => {
                                selected = Some(Some(branch.as_slice()));
                                break;
                            }
                            Some(false) => continue,
                            None => {
                                statically_known = false;
                                break;
                            }
                        }
                    }
                    if statically_known {
                        Some(selected.unwrap_or(else_branch.as_deref()))
                    } else {
                        None
                    }
                };
                if let Some(selected_else) = selected_else {
                    return lower_dynamic_if(
                        prefix,
                        condition,
                        then_branch,
                        selected_else,
                        &[],
                        &mut LinearLoweringState {
                            bindings: &mut bindings,
                            instructions: &mut instructions,
                            next: &mut next,
                        },
                    );
                }
            }
        }
        if let Some(lucid_syntax::Stmt::Return { value, .. }) = module.statements.last() {
            let prefix = &module.statements[..module.statements.len() - 1];
            let mut last = None;
            for statement in prefix {
                visit(
                    statement,
                    &mut bindings,
                    &mut instructions,
                    &mut next,
                    &mut last,
                )?;
            }
            let return_value = value
                .as_ref()
                .map(|expr| lower(expr, &bindings, &mut instructions, &mut next))
                .transpose()?;
            let function = Self {
                entry: BlockId(0),
                blocks: vec![Block {
                    id: BlockId(0),
                    instructions,
                    terminator: Terminator::Return(return_value),
                }],
            };
            function
                .verify()
                .map_err(|_| LowerError::UnsupportedExpression)?;
            return Ok(function);
        }
        // A single-block function cannot model instructions after a return;
        // reject that shape until the general terminator builder is in place
        // instead of silently executing unreachable source statements.
        if module
            .statements
            .iter()
            .enumerate()
            .any(|(index, statement)| {
                matches!(statement, lucid_syntax::Stmt::Return { .. })
                    && index + 1 != module.statements.len()
            })
        {
            return Err(LowerError::UnsupportedExpression);
        }
        for statement in &module.statements {
            visit(
                statement,
                &mut bindings,
                &mut instructions,
                &mut next,
                &mut last,
            )?;
        }
        // A module containing only declarations (or `pass`) has observable
        // void behavior.  Preserve that shape in CIR instead of pretending a
        // value-producing assignment exists.  This is also the natural form
        // for an empty module and lets native and interpreter entry points
        // share one verified representation.
        if last.is_none()
            && module.statements.iter().all(|statement| {
                matches!(
                    statement,
                    lucid_syntax::Stmt::ClassDef { .. }
                        | lucid_syntax::Stmt::InterfaceDef { .. }
                        | lucid_syntax::Stmt::TraitDef { .. }
                        | lucid_syntax::Stmt::ImplementDef { .. }
                        | lucid_syntax::Stmt::TypeAlias { .. }
                        | lucid_syntax::Stmt::Function(_)
                        | lucid_syntax::Stmt::Import { .. }
                        | lucid_syntax::Stmt::FromImport { .. }
                        | lucid_syntax::Stmt::Pass(_)
                )
            })
        {
            let function = Self {
                entry: BlockId(0),
                blocks: vec![Block {
                    id: BlockId(0),
                    instructions,
                    terminator: Terminator::Return(None),
                }],
            };
            function
                .verify()
                .map_err(|_| LowerError::UnsupportedExpression)?;
            return Ok(function);
        }
        let value = last.ok_or(LowerError::NoLowerableAssignment)?;
        let function = Self {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions,
                terminator: Terminator::Return(Some(value)),
            }],
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower the currently supported arithmetic expression subset into one
    /// entry block. The resulting function is verified before it is returned.
    pub fn from_expr(expr: &lucid_syntax::Expr) -> Result<Self, LowerError> {
        let mut instructions = Vec::new();
        let mut next = 0;
        fn lower(
            expr: &lucid_syntax::Expr,
            instructions: &mut Vec<Instruction>,
            next: &mut u32,
        ) -> Result<ValueId, LowerError> {
            match expr {
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Int(value),
                    ..
                } => {
                    let result = ValueId(*next);
                    *next += 1;
                    instructions.push(Instruction::ConstInt {
                        result,
                        value: *value,
                    });
                    Ok(result)
                }
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Bool(value),
                    ..
                } => {
                    let result = ValueId(*next);
                    *next += 1;
                    instructions.push(Instruction::ConstBool {
                        result,
                        value: *value,
                    });
                    Ok(result)
                }
                lucid_syntax::Expr::IfExpr {
                    condition,
                    then_branch,
                    else_branch,
                    ..
                } => {
                    // Constant conditions can be folded without changing
                    // Lucid's branch-selective evaluation rule. Dynamic
                    // conditions require explicit CFG blocks and are left to
                    // the statement/CIR lowering pass.
                    let lucid_syntax::Expr::Literal {
                        value: lucid_syntax::LiteralValue::Bool(condition),
                        ..
                    } = condition.as_ref()
                    else {
                        return Err(LowerError::UnsupportedExpression);
                    };
                    lower(
                        if *condition { then_branch } else { else_branch },
                        instructions,
                        next,
                    )
                }
                lucid_syntax::Expr::Binary {
                    left, op, right, ..
                } => {
                    if matches!(op, lucid_syntax::BinaryOp::And | lucid_syntax::BinaryOp::Or) {
                        let constant_truth = match left.as_ref() {
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Bool(value),
                                ..
                            } => Some(*value),
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Int(value),
                                ..
                            } => Some(*value != 0),
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Str(value),
                                ..
                            } => Some(!value.is_empty()),
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::None,
                                ..
                            } => Some(false),
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Float(value),
                                ..
                            } => Some(*value != 0.0),
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Complex(value),
                                ..
                            } => Some(*value != 0.0),
                            lucid_syntax::Expr::Literal {
                                value:
                                    lucid_syntax::LiteralValue::Sentinel(_)
                                    | lucid_syntax::LiteralValue::Ellipsis,
                                ..
                            } => Some(true),
                            _ => None,
                        };
                        if let Some(left_truth) = constant_truth {
                            let short_circuits = match op {
                                lucid_syntax::BinaryOp::And => !left_truth,
                                lucid_syntax::BinaryOp::Or => left_truth,
                                _ => return Err(LowerError::UnsupportedExpression),
                            };
                            if short_circuits {
                                let result = ValueId(*next);
                                *next += 1;
                                instructions.push(Instruction::ConstBool {
                                    result,
                                    value: left_truth,
                                });
                                return Ok(result);
                            }
                            let right = lower(right, instructions, next)?;
                            let zero = ValueId(*next);
                            *next += 1;
                            instructions.push(Instruction::ConstInt {
                                result: zero,
                                value: 0,
                            });
                            let result = ValueId(*next);
                            *next += 1;
                            instructions.push(Instruction::CmpNe {
                                result,
                                left: right,
                                right: zero,
                            });
                            return Ok(result);
                        }
                    }
                    let left = lower(left, instructions, next)?;
                    let right = lower(right, instructions, next)?;
                    let result = ValueId(*next);
                    *next += 1;
                    let instruction = match op {
                        lucid_syntax::BinaryOp::Add => Instruction::Add {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Sub => Instruction::Sub {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Mul => Instruction::Mul {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Pow => Instruction::Pow {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Div => Instruction::Div {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::FloorDiv => Instruction::FloorDiv {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Mod => Instruction::Mod {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::BitAnd => Instruction::BitAnd {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::BitOr => Instruction::BitOr {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::BitXor => Instruction::BitXor {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Shl => Instruction::Shl {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Shr => Instruction::Shr {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Eq
                        | lucid_syntax::BinaryOp::Identity
                        | lucid_syntax::BinaryOp::Is => Instruction::CmpEq {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::NotEq
                        | lucid_syntax::BinaryOp::NotIdentity
                        | lucid_syntax::BinaryOp::IsNot => Instruction::CmpNe {
                            result,
                            left,
                            right,
                        },
                        // Non-constant nested logical expressions must use
                        // CFG lowering; eager instructions would violate
                        // selective evaluation.
                        lucid_syntax::BinaryOp::And | lucid_syntax::BinaryOp::Or => {
                            return Err(LowerError::UnsupportedExpression);
                        }
                        lucid_syntax::BinaryOp::LtEq => Instruction::CmpLe {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Gt => Instruction::CmpGt {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::GtEq => Instruction::CmpGe {
                            result,
                            left,
                            right,
                        },
                        lucid_syntax::BinaryOp::Lt => Instruction::CmpLt {
                            result,
                            left,
                            right,
                        },
                        _ => return Err(LowerError::UnsupportedExpression),
                    };
                    instructions.push(instruction);
                    Ok(result)
                }
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Neg,
                    expr,
                    ..
                } => {
                    let operand = lower(expr, instructions, next)?;
                    let result = ValueId(*next);
                    *next += 1;
                    instructions.push(Instruction::Neg { result, operand });
                    Ok(result)
                }
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Pos,
                    expr,
                    ..
                } => {
                    // Unary plus is an identity operation for the integer CIR
                    // subset; still materialize a value so source spans and
                    // later typed lowering can attach to this expression.
                    lower(expr, instructions, next)
                }
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Invert,
                    expr,
                    ..
                } => {
                    let operand = lower(expr, instructions, next)?;
                    let result = ValueId(*next);
                    *next += 1;
                    instructions.push(Instruction::BitNot { result, operand });
                    Ok(result)
                }
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Not,
                    expr,
                    ..
                } => {
                    let operand = lower(expr, instructions, next)?;
                    let result = ValueId(*next);
                    *next += 1;
                    instructions.push(Instruction::Not { result, operand });
                    Ok(result)
                }
                _ => Err(LowerError::UnsupportedExpression),
            }
        }
        fn static_truth(expr: &lucid_syntax::Expr) -> Option<bool> {
            match expr {
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Bool(value),
                    ..
                } => Some(*value),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Int(value),
                    ..
                } => Some(*value != 0),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Str(value),
                    ..
                } => Some(!value.is_empty()),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::None,
                    ..
                } => Some(false),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Float(value),
                    ..
                } => Some(*value != 0.0),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Complex(value),
                    ..
                } => Some(*value != 0.0),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::BigInt(value),
                    ..
                } => {
                    let digits = value.trim().trim_start_matches(['+', '-']).replace('_', "");
                    let (digits, radix) = if let Some(value) = digits
                        .strip_prefix("0x")
                        .or_else(|| digits.strip_prefix("0X"))
                    {
                        (value, 16)
                    } else if let Some(value) = digits
                        .strip_prefix("0o")
                        .or_else(|| digits.strip_prefix("0O"))
                    {
                        (value, 8)
                    } else if let Some(value) = digits
                        .strip_prefix("0b")
                        .or_else(|| digits.strip_prefix("0B"))
                    {
                        (value, 2)
                    } else {
                        (digits.as_str(), 10)
                    };
                    Some(digits.chars().any(|digit| digit.to_digit(radix) != Some(0)))
                }
                lucid_syntax::Expr::Literal {
                    value:
                        lucid_syntax::LiteralValue::Sentinel(_) | lucid_syntax::LiteralValue::Ellipsis,
                    ..
                } => Some(true),
                lucid_syntax::Expr::Unary {
                    op: lucid_syntax::UnaryOp::Not,
                    expr,
                    ..
                } => static_truth(expr).map(|value| !value),
                lucid_syntax::Expr::Binary {
                    op, left, right, ..
                } => {
                    if matches!(op, lucid_syntax::BinaryOp::And | lucid_syntax::BinaryOp::Or) {
                        return match op {
                            lucid_syntax::BinaryOp::And => match static_truth(left) {
                                Some(false) => Some(false),
                                Some(true) => static_truth(right),
                                None => None,
                            },
                            lucid_syntax::BinaryOp::Or => match static_truth(left) {
                                Some(true) => Some(true),
                                Some(false) => static_truth(right),
                                None => None,
                            },
                            _ => None,
                        };
                    }
                    let comparison = |left: f64, right: f64| match op {
                        lucid_syntax::BinaryOp::Eq
                        | lucid_syntax::BinaryOp::Identity
                        | lucid_syntax::BinaryOp::Is => Some(left == right),
                        lucid_syntax::BinaryOp::NotEq
                        | lucid_syntax::BinaryOp::NotIdentity
                        | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                        lucid_syntax::BinaryOp::Lt => Some(left < right),
                        lucid_syntax::BinaryOp::LtEq => Some(left <= right),
                        lucid_syntax::BinaryOp::Gt => Some(left > right),
                        lucid_syntax::BinaryOp::GtEq => Some(left >= right),
                        _ => None,
                    };
                    match (left.as_ref(), right.as_ref()) {
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Str(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Str(right),
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(left == right),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::None,
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::None,
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(true),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(false),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Sentinel(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Sentinel(right),
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(left == right),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Ellipsis,
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Ellipsis,
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(true),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(false),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Bool(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Bool(right),
                                ..
                            },
                        ) => match op {
                            lucid_syntax::BinaryOp::Eq
                            | lucid_syntax::BinaryOp::Identity
                            | lucid_syntax::BinaryOp::Is => Some(left == right),
                            lucid_syntax::BinaryOp::NotEq
                            | lucid_syntax::BinaryOp::NotIdentity
                            | lucid_syntax::BinaryOp::IsNot => Some(left != right),
                            _ => None,
                        },
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Int(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Int(right),
                                ..
                            },
                        ) => comparison(*left as f64, *right as f64),
                        (
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Float(left),
                                ..
                            },
                            lucid_syntax::Expr::Literal {
                                value: lucid_syntax::LiteralValue::Float(right),
                                ..
                            },
                        ) => comparison(*left, *right),
                        _ => None,
                    }
                }
                _ => None,
            }
        }
        if let lucid_syntax::Expr::Binary {
            op, left, right, ..
        } = expr
        {
            if !matches!(op, lucid_syntax::BinaryOp::And | lucid_syntax::BinaryOp::Or) {
                // Fall through to ordinary expression lowering.
            } else {
                // Logical operators return canonical booleans in CIR, but their
                // right operand remains selectively evaluated. Lower the
                // short-circuit edge explicitly instead of using an eager And/Or
                // instruction.
                let literal_truth = static_truth(left);
                if let Some(left_truth) = literal_truth {
                    let short_circuits = matches!(op, lucid_syntax::BinaryOp::And) && !left_truth
                        || matches!(op, lucid_syntax::BinaryOp::Or) && left_truth;
                    if short_circuits {
                        let result = ValueId(next);
                        instructions.push(Instruction::ConstBool {
                            result,
                            value: left_truth,
                        });
                        let function = Self {
                            entry: BlockId(0),
                            blocks: vec![Block {
                                id: BlockId(0),
                                instructions,
                                terminator: Terminator::Return(Some(result)),
                            }],
                        };
                        function
                            .verify()
                            .map_err(|_| LowerError::UnsupportedExpression)?;
                        return Ok(function);
                    }
                    let right_value = lower(right, &mut instructions, &mut next)?;
                    let zero = ValueId(next);
                    next += 1;
                    instructions.push(Instruction::ConstInt {
                        result: zero,
                        value: 0,
                    });
                    let result = ValueId(next);
                    instructions.push(Instruction::CmpNe {
                        result,
                        left: right_value,
                        right: zero,
                    });
                    let function = Self {
                        entry: BlockId(0),
                        blocks: vec![Block {
                            id: BlockId(0),
                            instructions,
                            terminator: Terminator::Return(Some(result)),
                        }],
                    };
                    function
                        .verify()
                        .map_err(|_| LowerError::UnsupportedExpression)?;
                    return Ok(function);
                }
                let left_value = lower(left, &mut instructions, &mut next)?;
                let right_block = BlockId(1);
                let short_block = BlockId(2);
                let merge_block = BlockId(3);
                let (then_block, else_block, short_value) = match op {
                    lucid_syntax::BinaryOp::And => (right_block, short_block, false),
                    lucid_syntax::BinaryOp::Or => (short_block, right_block, true),
                    _ => return Err(LowerError::UnsupportedExpression),
                };
                let mut right_instructions = Vec::new();
                let right_value = lower(right, &mut right_instructions, &mut next)?;
                let zero = ValueId(next);
                next += 1;
                right_instructions.push(Instruction::ConstInt {
                    result: zero,
                    value: 0,
                });
                let right_bool = ValueId(next);
                next += 1;
                right_instructions.push(Instruction::CmpNe {
                    result: right_bool,
                    left: right_value,
                    right: zero,
                });
                let short_result = ValueId(next);
                next += 1;
                let result = ValueId(next);
                let function = Self {
                    entry: BlockId(0),
                    blocks: vec![
                        Block {
                            id: BlockId(0),
                            instructions,
                            terminator: Terminator::Branch {
                                condition: left_value,
                                then_block,
                                else_block,
                            },
                        },
                        Block {
                            id: right_block,
                            instructions: right_instructions,
                            terminator: Terminator::Jump(merge_block),
                        },
                        Block {
                            id: short_block,
                            instructions: vec![Instruction::ConstBool {
                                result: short_result,
                                value: short_value,
                            }],
                            terminator: Terminator::Jump(merge_block),
                        },
                        Block {
                            id: merge_block,
                            instructions: vec![Instruction::Phi {
                                result,
                                incomings: vec![
                                    (right_block, right_bool),
                                    (short_block, short_result),
                                ],
                            }],
                            terminator: Terminator::Return(Some(result)),
                        },
                    ],
                };
                function
                    .verify()
                    .map_err(|_| LowerError::UnsupportedExpression)?;
                return Ok(function);
            }
        }
        if let lucid_syntax::Expr::IfExpr {
            condition,
            then_branch,
            else_branch,
            ..
        } = expr
        {
            if matches!(
                condition.as_ref(),
                lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Bool(_),
                    ..
                }
            ) {
                let value = lower(expr, &mut instructions, &mut next)?;
                let function = Self {
                    entry: BlockId(0),
                    blocks: vec![Block {
                        id: BlockId(0),
                        instructions,
                        terminator: Terminator::Return(Some(value)),
                    }],
                };
                function
                    .verify()
                    .map_err(|_| LowerError::UnsupportedExpression)?;
                return Ok(function);
            }
            // Build a small, explicit diamond CFG. Branch expressions are
            // lowered independently so only the selected arm executes; the
            // merge block carries the result through an edge-based Phi.
            let condition_value = lower(condition, &mut instructions, &mut next)?;
            let then_block = BlockId(1);
            let else_block = BlockId(2);
            let merge_block = BlockId(3);
            let mut then_instructions = Vec::new();
            let then_value = lower(then_branch, &mut then_instructions, &mut next)?;
            let mut else_instructions = Vec::new();
            let else_value = lower(else_branch, &mut else_instructions, &mut next)?;
            let result = ValueId(next);
            let function = Self {
                entry: BlockId(0),
                blocks: vec![
                    Block {
                        id: BlockId(0),
                        instructions,
                        terminator: Terminator::Branch {
                            condition: condition_value,
                            then_block,
                            else_block,
                        },
                    },
                    Block {
                        id: then_block,
                        instructions: then_instructions,
                        terminator: Terminator::Jump(merge_block),
                    },
                    Block {
                        id: else_block,
                        instructions: else_instructions,
                        terminator: Terminator::Jump(merge_block),
                    },
                    Block {
                        id: merge_block,
                        instructions: vec![Instruction::Phi {
                            result,
                            incomings: vec![(then_block, then_value), (else_block, else_value)],
                        }],
                        terminator: Terminator::Return(Some(result)),
                    },
                ],
            };
            function
                .verify()
                .map_err(|_| LowerError::UnsupportedExpression)?;
            return Ok(function);
        }
        let value = lower(expr, &mut instructions, &mut next)?;
        let function = Self {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions,
                terminator: Terminator::Return(Some(value)),
            }],
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    fn lower_parameter_expr(
        expr: &lucid_syntax::Expr,
        parameters: &[String],
        instructions: &mut Vec<Instruction>,
        next: &mut u32,
    ) -> Result<ValueId, LowerError> {
        let result = |next: &mut u32| {
            let value = ValueId(*next);
            *next += 1;
            value
        };
        match expr {
            lucid_syntax::Expr::Ident { name, .. } => parameters
                .iter()
                .position(|parameter| parameter == name)
                .map(|index| ValueId(index as u32))
                .ok_or(LowerError::UnsupportedExpression),
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Int(value),
                ..
            } => {
                let value_id = result(next);
                instructions.push(Instruction::ConstInt {
                    result: value_id,
                    value: *value,
                });
                Ok(value_id)
            }
            lucid_syntax::Expr::Literal {
                value: lucid_syntax::LiteralValue::Bool(value),
                ..
            } => {
                let value_id = result(next);
                instructions.push(Instruction::ConstBool {
                    result: value_id,
                    value: *value,
                });
                Ok(value_id)
            }
            lucid_syntax::Expr::Unary { op, expr, .. } => {
                let operand = Self::lower_parameter_expr(expr, parameters, instructions, next)?;
                let value_id = result(next);
                let instruction = match op {
                    lucid_syntax::UnaryOp::Neg => Instruction::Neg {
                        result: value_id,
                        operand,
                    },
                    lucid_syntax::UnaryOp::Invert => Instruction::BitNot {
                        result: value_id,
                        operand,
                    },
                    lucid_syntax::UnaryOp::Not => Instruction::Not {
                        result: value_id,
                        operand,
                    },
                    lucid_syntax::UnaryOp::Pos => return Ok(operand),
                    lucid_syntax::UnaryOp::Spread | lucid_syntax::UnaryOp::GatherSpread => {
                        return Err(LowerError::UnsupportedExpression);
                    }
                };
                instructions.push(instruction);
                Ok(value_id)
            }
            lucid_syntax::Expr::Binary {
                left, op, right, ..
            } => {
                let left = Self::lower_parameter_expr(left, parameters, instructions, next)?;
                let right = Self::lower_parameter_expr(right, parameters, instructions, next)?;
                let value_id = result(next);
                let instruction = match op {
                    lucid_syntax::BinaryOp::Add => Instruction::Add {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Sub => Instruction::Sub {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Mul => Instruction::Mul {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Pow => Instruction::Pow {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Div => Instruction::Div {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::FloorDiv => Instruction::FloorDiv {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Mod => Instruction::Mod {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::BitAnd => Instruction::BitAnd {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::BitOr => Instruction::BitOr {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::BitXor => Instruction::BitXor {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Shl => Instruction::Shl {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Shr => Instruction::Shr {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Eq
                    | lucid_syntax::BinaryOp::Identity
                    | lucid_syntax::BinaryOp::Is => Instruction::CmpEq {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::NotEq
                    | lucid_syntax::BinaryOp::NotIdentity
                    | lucid_syntax::BinaryOp::IsNot => Instruction::CmpNe {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Lt => Instruction::CmpLt {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::LtEq => Instruction::CmpLe {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Gt => Instruction::CmpGt {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::GtEq => Instruction::CmpGe {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::And => Instruction::And {
                        result: value_id,
                        left,
                        right,
                    },
                    lucid_syntax::BinaryOp::Or => Instruction::Or {
                        result: value_id,
                        left,
                        right,
                    },
                    _ => return Err(LowerError::UnsupportedExpression),
                };
                instructions.push(instruction);
                Ok(value_id)
            }
            _ => Err(LowerError::UnsupportedExpression),
        }
    }

    /// Lower a function-shaped conditional whose expressions may read
    /// positional parameters. Parameters are materialized once in the entry
    /// block; both branch expressions then reference those stable values.
    pub fn from_parameterized_if(
        condition: &lucid_syntax::Expr,
        then_expr: &lucid_syntax::Expr,
        else_expr: &lucid_syntax::Expr,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        let mut then_instructions = Vec::new();
        let then_value = Self::lower_parameter_expr(
            then_expr,
            parameter_names,
            &mut then_instructions,
            &mut next,
        )?;
        let mut else_instructions = Vec::new();
        let else_value = Self::lower_parameter_expr(
            else_expr,
            parameter_names,
            &mut else_instructions,
            &mut next,
        )?;
        let result = ValueId(next);
        let function = Self {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: entry_instructions,
                    terminator: Terminator::Branch {
                        condition: condition_value,
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    instructions: then_instructions,
                    terminator: Terminator::Jump(BlockId(3)),
                },
                Block {
                    id: BlockId(2),
                    instructions: else_instructions,
                    terminator: Terminator::Jump(BlockId(3)),
                },
                Block {
                    id: BlockId(3),
                    instructions: vec![Instruction::Phi {
                        result,
                        incomings: vec![(BlockId(1), then_value), (BlockId(2), else_value)],
                    }],
                    terminator: Terminator::Return(Some(result)),
                },
            ],
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a parameter-aware conditional whose two arms both return void.
    /// Keeping the branch structure in CIR preserves evaluation and cleanup
    /// ordering even though no value needs a merge node.
    pub fn from_parameterized_if_void(
        condition: &lucid_syntax::Expr,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        let mut instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = {
            // Reuse the expression lowering by placing a dummy value in a
            // temporary diamond, then retain only its entry instructions.
            let probe = Self::from_parameterized_if(
                condition,
                &lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Int(0),
                    span: condition.span(),
                },
                &lucid_syntax::Expr::Literal {
                    value: lucid_syntax::LiteralValue::Int(0),
                    span: condition.span(),
                },
                parameter_names,
            )?;
            let entry = probe
                .blocks
                .first()
                .ok_or(LowerError::UnsupportedExpression)?;
            instructions.extend(
                entry
                    .instructions
                    .iter()
                    .skip(parameter_names.len())
                    .cloned(),
            );
            match entry.terminator {
                Terminator::Branch { condition, .. } => condition,
                _ => return Err(LowerError::UnsupportedExpression),
            }
        };
        // `next` is not needed after the condition; keep the local explicit
        // so the verifier sees no accidental value-ID reuse.
        let _ = &mut next;
        let function = Self {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions,
                    terminator: Terminator::Branch {
                        condition: condition_value,
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(None),
                },
                Block {
                    id: BlockId(2),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(None),
                },
            ],
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower `if`/`elif`/`else` branches whose arms all return no value.
    /// The shape mirrors [`Self::from_parameterized_if_elif_chain_direct`]
    /// but needs no value blocks beyond the condition ladder.
    pub fn from_parameterized_if_elif_void_chain(
        condition: &lucid_syntax::Expr,
        elif_conditions: &[&lucid_syntax::Expr],
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        if elif_conditions.is_empty() {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: entry_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                instructions: Vec::new(),
                terminator: Terminator::Return(None),
            },
        ];
        let mut condition_block = 2_u32;
        for (index, elif_condition) in elif_conditions.iter().enumerate() {
            let then_block = condition_block + 1;
            let false_block = condition_block + 2;
            let mut condition_instructions = Vec::new();
            let condition_value = Self::lower_parameter_expr(
                elif_condition,
                parameter_names,
                &mut condition_instructions,
                &mut next,
            )?;
            blocks.push(Block {
                id: BlockId(condition_block),
                instructions: condition_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(then_block),
                    else_block: BlockId(false_block),
                },
            });
            blocks.push(Block {
                id: BlockId(then_block),
                instructions: Vec::new(),
                terminator: Terminator::Return(None),
            });
            if index == elif_conditions.len() - 1 {
                blocks.push(Block {
                    id: BlockId(false_block),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(None),
                });
            }
            condition_block = false_block;
        }
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower `if`/`elif` branches that return values while the final
    /// fall-through returns no value.
    pub fn from_parameterized_if_elif_optional_chain(
        condition: &lucid_syntax::Expr,
        then_expr: &lucid_syntax::Expr,
        elif_branches: &[(&lucid_syntax::Expr, &lucid_syntax::Expr)],
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        if elif_branches.is_empty() {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        let mut then_instructions = Vec::new();
        let then_value = Self::lower_parameter_expr(
            then_expr,
            parameter_names,
            &mut then_instructions,
            &mut next,
        )?;
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: entry_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                instructions: then_instructions,
                terminator: Terminator::Return(Some(then_value)),
            },
        ];
        let mut condition_block = 2_u32;
        for (index, (elif_condition, elif_expr)) in elif_branches.iter().enumerate() {
            let then_block = condition_block + 1;
            let false_block = condition_block + 2;
            let mut condition_instructions = Vec::new();
            let condition_value = Self::lower_parameter_expr(
                elif_condition,
                parameter_names,
                &mut condition_instructions,
                &mut next,
            )?;
            let mut then_instructions = Vec::new();
            let then_value = Self::lower_parameter_expr(
                elif_expr,
                parameter_names,
                &mut then_instructions,
                &mut next,
            )?;
            blocks.push(Block {
                id: BlockId(condition_block),
                instructions: condition_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(then_block),
                    else_block: BlockId(false_block),
                },
            });
            blocks.push(Block {
                id: BlockId(then_block),
                instructions: then_instructions,
                terminator: Terminator::Return(Some(then_value)),
            });
            if index == elif_branches.len() - 1 {
                blocks.push(Block {
                    id: BlockId(false_block),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(None),
                });
            }
            condition_block = false_block;
        }
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower `if`/`elif`/`else` returns as direct-returning branch blocks.
    /// Each false edge falls through to the next condition block, preserving
    /// the source ladder without manufacturing a nested value expression.
    pub fn from_parameterized_if_elif_chain_direct(
        condition: &lucid_syntax::Expr,
        then_expr: &lucid_syntax::Expr,
        elif_branches: &[(&lucid_syntax::Expr, &lucid_syntax::Expr)],
        else_expr: &lucid_syntax::Expr,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        if elif_branches.is_empty() {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        let mut then_instructions = Vec::new();
        let then_value = Self::lower_parameter_expr(
            then_expr,
            parameter_names,
            &mut then_instructions,
            &mut next,
        )?;
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: entry_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                instructions: then_instructions,
                terminator: Terminator::Return(Some(then_value)),
            },
        ];
        let mut condition_block = 2_u32;
        for (index, (elif_condition, elif_expr)) in elif_branches.iter().enumerate() {
            let then_block = condition_block + 1;
            let false_block = condition_block + 2;
            let mut condition_instructions = Vec::new();
            let condition_value = Self::lower_parameter_expr(
                elif_condition,
                parameter_names,
                &mut condition_instructions,
                &mut next,
            )?;
            let mut then_instructions = Vec::new();
            let then_value = Self::lower_parameter_expr(
                elif_expr,
                parameter_names,
                &mut then_instructions,
                &mut next,
            )?;
            blocks.push(Block {
                id: BlockId(condition_block),
                instructions: condition_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(then_block),
                    else_block: BlockId(false_block),
                },
            });
            blocks.push(Block {
                id: BlockId(then_block),
                instructions: then_instructions,
                terminator: Terminator::Return(Some(then_value)),
            });
            if index == elif_branches.len() - 1 {
                let mut else_instructions = Vec::new();
                let else_value = Self::lower_parameter_expr(
                    else_expr,
                    parameter_names,
                    &mut else_instructions,
                    &mut next,
                )?;
                blocks.push(Block {
                    id: BlockId(false_block),
                    instructions: else_instructions,
                    terminator: Terminator::Return(Some(else_value)),
                });
            }
            condition_block = false_block;
        }
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower an `if`/`elif` guard ladder where each selected branch returns
    /// either a value or no value. The resulting CIR uses direct branch
    /// returns for every arm, so mixed value/void control flow never passes
    /// through a Phi.
    pub fn from_parameterized_if_elif_mixed_return_chain(
        condition: &lucid_syntax::Expr,
        then_expr: Option<&lucid_syntax::Expr>,
        elif_branches: &[(&lucid_syntax::Expr, Option<&lucid_syntax::Expr>)],
        fallback_expr: Option<&lucid_syntax::Expr>,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        if elif_branches.is_empty() {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        fn return_block(
            id: BlockId,
            expr: Option<&lucid_syntax::Expr>,
            parameter_names: &[String],
            next: &mut u32,
        ) -> Result<Block, LowerError> {
            let mut instructions = Vec::new();
            let value = expr
                .map(|expr| {
                    Function::lower_parameter_expr(expr, parameter_names, &mut instructions, next)
                })
                .transpose()?;
            Ok(Block {
                id,
                instructions,
                terminator: Terminator::Return(value),
            })
        }
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: entry_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            return_block(BlockId(1), then_expr, parameter_names, &mut next)?,
        ];
        let mut condition_block = 2_u32;
        for (index, (elif_condition, elif_expr)) in elif_branches.iter().enumerate() {
            let then_block = condition_block + 1;
            let false_block = condition_block + 2;
            let mut condition_instructions = Vec::new();
            let condition_value = Self::lower_parameter_expr(
                elif_condition,
                parameter_names,
                &mut condition_instructions,
                &mut next,
            )?;
            blocks.push(Block {
                id: BlockId(condition_block),
                instructions: condition_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(then_block),
                    else_block: BlockId(false_block),
                },
            });
            blocks.push(return_block(
                BlockId(then_block),
                *elif_expr,
                parameter_names,
                &mut next,
            )?);
            if index == elif_branches.len() - 1 {
                blocks.push(return_block(
                    BlockId(false_block),
                    fallback_expr,
                    parameter_names,
                    &mut next,
                )?);
            }
            condition_block = false_block;
        }
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a guard ladder whose first branch returns no value while later
    /// `elif` branches and the final fall-through return values.
    pub fn from_parameterized_if_void_elif_chain_direct(
        condition: &lucid_syntax::Expr,
        elif_branches: &[(&lucid_syntax::Expr, &lucid_syntax::Expr)],
        else_expr: &lucid_syntax::Expr,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        if elif_branches.is_empty() {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: entry_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                instructions: Vec::new(),
                terminator: Terminator::Return(None),
            },
        ];
        let mut condition_block = 2_u32;
        for (index, (elif_condition, elif_expr)) in elif_branches.iter().enumerate() {
            let then_block = condition_block + 1;
            let false_block = condition_block + 2;
            let mut condition_instructions = Vec::new();
            let condition_value = Self::lower_parameter_expr(
                elif_condition,
                parameter_names,
                &mut condition_instructions,
                &mut next,
            )?;
            let mut then_instructions = Vec::new();
            let then_value = Self::lower_parameter_expr(
                elif_expr,
                parameter_names,
                &mut then_instructions,
                &mut next,
            )?;
            blocks.push(Block {
                id: BlockId(condition_block),
                instructions: condition_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(then_block),
                    else_block: BlockId(false_block),
                },
            });
            blocks.push(Block {
                id: BlockId(then_block),
                instructions: then_instructions,
                terminator: Terminator::Return(Some(then_value)),
            });
            if index == elif_branches.len() - 1 {
                let mut else_instructions = Vec::new();
                let else_value = Self::lower_parameter_expr(
                    else_expr,
                    parameter_names,
                    &mut else_instructions,
                    &mut next,
                )?;
                blocks.push(Block {
                    id: BlockId(false_block),
                    instructions: else_instructions,
                    terminator: Terminator::Return(Some(else_value)),
                });
            }
            condition_block = false_block;
        }
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a guard ladder whose first branch returns no value, later
    /// `elif` branches return values, and the final fall-through returns no
    /// value.
    pub fn from_parameterized_if_void_elif_optional_chain(
        condition: &lucid_syntax::Expr,
        elif_branches: &[(&lucid_syntax::Expr, &lucid_syntax::Expr)],
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        if elif_branches.is_empty() {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: entry_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                instructions: Vec::new(),
                terminator: Terminator::Return(None),
            },
        ];
        let mut condition_block = 2_u32;
        for (index, (elif_condition, elif_expr)) in elif_branches.iter().enumerate() {
            let then_block = condition_block + 1;
            let false_block = condition_block + 2;
            let mut condition_instructions = Vec::new();
            let condition_value = Self::lower_parameter_expr(
                elif_condition,
                parameter_names,
                &mut condition_instructions,
                &mut next,
            )?;
            let mut then_instructions = Vec::new();
            let then_value = Self::lower_parameter_expr(
                elif_expr,
                parameter_names,
                &mut then_instructions,
                &mut next,
            )?;
            blocks.push(Block {
                id: BlockId(condition_block),
                instructions: condition_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(then_block),
                    else_block: BlockId(false_block),
                },
            });
            blocks.push(Block {
                id: BlockId(then_block),
                instructions: then_instructions,
                terminator: Terminator::Return(Some(then_value)),
            });
            if index == elif_branches.len() - 1 {
                blocks.push(Block {
                    id: BlockId(false_block),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(None),
                });
            }
            condition_block = false_block;
        }
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a guard ladder whose `if` and `elif` branches return no value
    /// while the final fall-through returns a value.
    pub fn from_parameterized_if_elif_voids_value_fallback(
        condition: &lucid_syntax::Expr,
        elif_conditions: &[&lucid_syntax::Expr],
        else_expr: &lucid_syntax::Expr,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        if elif_conditions.is_empty() {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: entry_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                instructions: Vec::new(),
                terminator: Terminator::Return(None),
            },
        ];
        let mut condition_block = 2_u32;
        for (index, elif_condition) in elif_conditions.iter().enumerate() {
            let then_block = condition_block + 1;
            let false_block = condition_block + 2;
            let mut condition_instructions = Vec::new();
            let condition_value = Self::lower_parameter_expr(
                elif_condition,
                parameter_names,
                &mut condition_instructions,
                &mut next,
            )?;
            blocks.push(Block {
                id: BlockId(condition_block),
                instructions: condition_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(then_block),
                    else_block: BlockId(false_block),
                },
            });
            blocks.push(Block {
                id: BlockId(then_block),
                instructions: Vec::new(),
                terminator: Terminator::Return(None),
            });
            if index == elif_conditions.len() - 1 {
                let mut else_instructions = Vec::new();
                let else_value = Self::lower_parameter_expr(
                    else_expr,
                    parameter_names,
                    &mut else_instructions,
                    &mut next,
                )?;
                blocks.push(Block {
                    id: BlockId(false_block),
                    instructions: else_instructions,
                    terminator: Terminator::Return(Some(else_value)),
                });
            }
            condition_block = false_block;
        }
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a guard ladder whose first branch and final fall-through return
    /// values while every `elif` branch returns no value.
    pub fn from_parameterized_if_elif_voids_else_direct(
        condition: &lucid_syntax::Expr,
        then_expr: &lucid_syntax::Expr,
        elif_conditions: &[&lucid_syntax::Expr],
        else_expr: &lucid_syntax::Expr,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        if elif_conditions.is_empty() {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        let mut then_instructions = Vec::new();
        let then_value = Self::lower_parameter_expr(
            then_expr,
            parameter_names,
            &mut then_instructions,
            &mut next,
        )?;
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: entry_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                instructions: then_instructions,
                terminator: Terminator::Return(Some(then_value)),
            },
        ];
        let mut condition_block = 2_u32;
        for (index, elif_condition) in elif_conditions.iter().enumerate() {
            let then_block = condition_block + 1;
            let false_block = condition_block + 2;
            let mut condition_instructions = Vec::new();
            let condition_value = Self::lower_parameter_expr(
                elif_condition,
                parameter_names,
                &mut condition_instructions,
                &mut next,
            )?;
            blocks.push(Block {
                id: BlockId(condition_block),
                instructions: condition_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(then_block),
                    else_block: BlockId(false_block),
                },
            });
            blocks.push(Block {
                id: BlockId(then_block),
                instructions: Vec::new(),
                terminator: Terminator::Return(None),
            });
            if index == elif_conditions.len() - 1 {
                let mut else_instructions = Vec::new();
                let else_value = Self::lower_parameter_expr(
                    else_expr,
                    parameter_names,
                    &mut else_instructions,
                    &mut next,
                )?;
                blocks.push(Block {
                    id: BlockId(false_block),
                    instructions: else_instructions,
                    terminator: Terminator::Return(Some(else_value)),
                });
            }
            condition_block = false_block;
        }
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a guard ladder whose first branch returns a value while every
    /// `elif` branch and the final fall-through return no value.
    pub fn from_parameterized_if_elif_voids_optional(
        condition: &lucid_syntax::Expr,
        then_expr: &lucid_syntax::Expr,
        elif_conditions: &[&lucid_syntax::Expr],
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        if elif_conditions.is_empty() {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut entry_instructions = parameter_names
            .iter()
            .enumerate()
            .map(|(index, _)| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next = parameter_names.len() as u32;
        let condition_value = Self::lower_parameter_expr(
            condition,
            parameter_names,
            &mut entry_instructions,
            &mut next,
        )?;
        let mut then_instructions = Vec::new();
        let then_value = Self::lower_parameter_expr(
            then_expr,
            parameter_names,
            &mut then_instructions,
            &mut next,
        )?;
        let mut blocks = vec![
            Block {
                id: BlockId(0),
                instructions: entry_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(1),
                    else_block: BlockId(2),
                },
            },
            Block {
                id: BlockId(1),
                instructions: then_instructions,
                terminator: Terminator::Return(Some(then_value)),
            },
        ];
        let mut condition_block = 2_u32;
        for (index, elif_condition) in elif_conditions.iter().enumerate() {
            let then_block = condition_block + 1;
            let false_block = condition_block + 2;
            let mut condition_instructions = Vec::new();
            let condition_value = Self::lower_parameter_expr(
                elif_condition,
                parameter_names,
                &mut condition_instructions,
                &mut next,
            )?;
            blocks.push(Block {
                id: BlockId(condition_block),
                instructions: condition_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: BlockId(then_block),
                    else_block: BlockId(false_block),
                },
            });
            blocks.push(Block {
                id: BlockId(then_block),
                instructions: Vec::new(),
                terminator: Terminator::Return(None),
            });
            if index == elif_conditions.len() - 1 {
                blocks.push(Block {
                    id: BlockId(false_block),
                    instructions: Vec::new(),
                    terminator: Terminator::Return(None),
                });
            }
            condition_block = false_block;
        }
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a conditional with a value-returning then arm and a void
    /// fall-through else arm. The helper first builds the ordinary diamond,
    /// then removes the synthetic else value and merge block.
    pub fn from_parameterized_if_optional(
        condition: &lucid_syntax::Expr,
        then_expr: &lucid_syntax::Expr,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        let zero = lucid_syntax::Expr::Literal {
            value: lucid_syntax::LiteralValue::Int(0),
            span: condition.span(),
        };
        let mut function =
            Self::from_parameterized_if(condition, then_expr, &zero, parameter_names)?;
        let then_value = match function
            .blocks
            .get(3)
            .and_then(|block| block.instructions.first())
        {
            Some(Instruction::Phi { incomings, .. }) => incomings
                .iter()
                .find(|(block, _)| *block == BlockId(1))
                .map(|(_, value)| *value)
                .ok_or(LowerError::UnsupportedExpression)?,
            _ => return Err(LowerError::UnsupportedExpression),
        };
        function.blocks[1].terminator = Terminator::Return(Some(then_value));
        function.blocks[2].instructions.clear();
        function.blocks[2].terminator = Terminator::Return(None);
        function.blocks.truncate(3);
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a conditional as direct-returning branch blocks. This variant
    /// avoids a value `Phi`, which is necessary when a branch carries a
    /// recoverable operation whose status must be returned with that value.
    pub fn from_parameterized_if_direct(
        condition: &lucid_syntax::Expr,
        then_expr: &lucid_syntax::Expr,
        else_expr: &lucid_syntax::Expr,
        parameter_names: &[String],
    ) -> Result<Self, LowerError> {
        let mut function =
            Self::from_parameterized_if(condition, then_expr, else_expr, parameter_names)?;
        let (then_value, else_value) = match function
            .blocks
            .get(3)
            .and_then(|block| block.instructions.first())
        {
            Some(Instruction::Phi { incomings, .. }) => {
                let then_value = incomings
                    .iter()
                    .find(|(block, _)| *block == BlockId(1))
                    .map(|(_, value)| *value)
                    .ok_or(LowerError::UnsupportedExpression)?;
                let else_value = incomings
                    .iter()
                    .find(|(block, _)| *block == BlockId(2))
                    .map(|(_, value)| *value)
                    .ok_or(LowerError::UnsupportedExpression)?;
                (then_value, else_value)
            }
            _ => return Err(LowerError::UnsupportedExpression),
        };
        function.blocks[1].terminator = Terminator::Return(Some(then_value));
        function.blocks[2].terminator = Terminator::Return(Some(else_value));
        function.blocks.truncate(3);
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Lower a parameterized match whose scrutinee is one positional value,
    /// every explicit arm is an integer/boolean literal, and the final arm is
    /// a wildcard.  Each arm returns a primitive literal.  The decision chain
    /// is represented directly in CIR so only the selected arm executes.
    pub fn from_parameterized_literal_match(
        parameter_count: usize,
        parameter_index: usize,
        arms: &[(TypedLiteral, TypedLiteral)],
        wildcard: TypedLiteral,
    ) -> Result<Self, LowerError> {
        if arms.is_empty() || parameter_index >= parameter_count {
            return Err(LowerError::UnsupportedExpression);
        }
        let mut blocks = Vec::with_capacity(arms.len() * 2 + 2);
        let params = (0..parameter_count)
            .map(|index| Instruction::Param {
                result: ValueId(index as u32),
                index: index as u32,
            })
            .collect::<Vec<_>>();
        let mut next_value = parameter_count as u32;
        let merge_block = BlockId((arms.len() * 2 + 1) as u32);
        let value_instruction = |literal: TypedLiteral, result: ValueId| match literal {
            TypedLiteral::Int(value) => Instruction::ConstInt { result, value },
            TypedLiteral::Bool(value) => Instruction::ConstBool { result, value },
        };
        let mut incoming = Vec::with_capacity(arms.len() + 1);
        for (index, (pattern, result_literal)) in arms.iter().enumerate() {
            let test_block = BlockId((index * 2) as u32);
            let arm_block = BlockId((index * 2 + 1) as u32);
            let next_test = if index + 1 < arms.len() {
                BlockId(((index + 1) * 2) as u32)
            } else {
                BlockId((arms.len() * 2) as u32)
            };
            let pattern_value = ValueId(next_value);
            next_value += 1;
            let condition_value = ValueId(next_value);
            next_value += 1;
            let result_value = ValueId(next_value);
            next_value += 1;
            let mut test_instructions = if index == 0 {
                params.clone()
            } else {
                Vec::new()
            };
            test_instructions.push(value_instruction(*pattern, pattern_value));
            test_instructions.push(Instruction::CmpEq {
                result: condition_value,
                left: ValueId(parameter_index as u32),
                right: pattern_value,
            });
            let arm_instructions = vec![value_instruction(*result_literal, result_value)];
            incoming.push((arm_block, result_value));
            blocks.push(Block {
                id: test_block,
                instructions: test_instructions,
                terminator: Terminator::Branch {
                    condition: condition_value,
                    then_block: arm_block,
                    else_block: next_test,
                },
            });
            blocks.push(Block {
                id: arm_block,
                instructions: arm_instructions,
                terminator: Terminator::Jump(merge_block),
            });
        }
        let wildcard_block = BlockId((arms.len() * 2) as u32);
        let wildcard_value = ValueId(next_value);
        next_value += 1;
        incoming.push((wildcard_block, wildcard_value));
        blocks.push(Block {
            id: wildcard_block,
            instructions: vec![value_instruction(wildcard, wildcard_value)],
            terminator: Terminator::Jump(merge_block),
        });
        let result = ValueId(next_value);
        blocks.push(Block {
            id: merge_block,
            instructions: vec![Instruction::Phi {
                result,
                incomings: incoming,
            }],
            terminator: Terminator::Return(Some(result)),
        });
        let function = Self {
            entry: BlockId(0),
            blocks,
        };
        function
            .verify()
            .map_err(|_| LowerError::UnsupportedExpression)?;
        Ok(function)
    }

    /// Validate block and SSA-value references before either backend consumes
    /// the function. Every definition must dominate each use, including uses
    /// in terminators. This makes the SSA contract explicit instead of
    /// allowing an interpreter or native backend to observe an uninitialized
    /// value on one control-flow path.
    pub fn verify(&self) -> Result<(), VerifyError> {
        if self.blocks.is_empty() {
            return Err(VerifyError::EmptyFunction);
        }
        let mut blocks = std::collections::HashSet::new();
        for block in &self.blocks {
            if !blocks.insert(block.id) {
                return Err(VerifyError::DuplicateBlock(block.id));
            }
        }
        if !blocks.contains(&self.entry) {
            return Err(VerifyError::MissingEntry(self.entry));
        }
        let mut values = std::collections::HashSet::new();
        let mut definitions = std::collections::HashMap::new();
        // Collect definitions independently of block order, because source
        // lowering and serialization are not required to preserve block order.
        for block in &self.blocks {
            let mut seen_non_phi = false;
            for (index, instruction) in block.instructions.iter().enumerate() {
                if !matches!(instruction, Instruction::Phi { .. }) {
                    seen_non_phi = true;
                } else if seen_non_phi {
                    // Phi nodes conceptually execute on block entry and must
                    // precede ordinary instructions.  Enforcing this here
                    // keeps the interpreter and Cranelift on one SSA shape.
                    return Err(VerifyError::PhiAfterNonPhi { block: block.id });
                }
                let result = match instruction {
                    Instruction::Param { result, .. }
                    | Instruction::ConstInt { result, .. }
                    | Instruction::ConstBool { result, .. }
                    | Instruction::Add { result, .. }
                    | Instruction::Sub { result, .. }
                    | Instruction::Mul { result, .. }
                    | Instruction::Pow { result, .. }
                    | Instruction::Div { result, .. }
                    | Instruction::FloorDiv { result, .. }
                    | Instruction::Mod { result, .. }
                    | Instruction::BitAnd { result, .. }
                    | Instruction::BitOr { result, .. }
                    | Instruction::BitXor { result, .. }
                    | Instruction::Shl { result, .. }
                    | Instruction::Shr { result, .. }
                    | Instruction::CmpEq { result, .. }
                    | Instruction::CmpNe { result, .. }
                    | Instruction::CmpLe { result, .. }
                    | Instruction::CmpGt { result, .. }
                    | Instruction::CmpGe { result, .. }
                    | Instruction::CmpLt { result, .. }
                    | Instruction::Neg { result, .. } => result,
                    Instruction::BitNot { result, .. } => result,
                    Instruction::Not { result, .. } => result,
                    Instruction::And { result, .. }
                    | Instruction::Or { result, .. }
                    | Instruction::Phi { result, .. } => result,
                };
                if !values.insert(*result) {
                    return Err(VerifyError::DuplicateValue(*result));
                }
                definitions.insert(*result, (block.id, index));
            }
        }
        let require = |value: ValueId| {
            values
                .contains(&value)
                .then_some(())
                .ok_or(VerifyError::UndefinedValue(value))
        };
        // Validate successor references and build predecessor lists before
        // calculating dominance. Missing targets are reported consistently,
        // even when the malformed block is unreachable from the entry.
        let mut predecessors = std::collections::HashMap::<BlockId, Vec<BlockId>>::new();
        for block in &self.blocks {
            predecessors.entry(block.id).or_default();
            let targets = match block.terminator {
                Terminator::Jump(target) => vec![target],
                Terminator::Branch {
                    then_block,
                    else_block,
                    ..
                } => vec![then_block, else_block],
                Terminator::Return(_) => Vec::new(),
            };
            for target in targets {
                if !blocks.contains(&target) {
                    return Err(VerifyError::MissingBlock(target));
                }
                predecessors.entry(target).or_default().push(block.id);
            }
        }

        // Compute the usual iterative dominator sets for reachable blocks.
        // Unreachable blocks get only themselves as a dominator, which keeps
        // dead code locally valid without allowing it to feed live code.
        let mut reachable = std::collections::HashSet::new();
        let mut work = vec![self.entry];
        while let Some(block) = work.pop() {
            if !reachable.insert(block) {
                continue;
            }
            let Some(current) = self.blocks.iter().find(|candidate| candidate.id == block) else {
                return Err(VerifyError::MissingBlock(block));
            };
            let successors = match &current.terminator {
                Terminator::Jump(target) => vec![*target],
                Terminator::Branch {
                    then_block,
                    else_block,
                    ..
                } => vec![*then_block, *else_block],
                Terminator::Return(_) => Vec::new(),
            };
            work.extend(successors);
        }
        let all_blocks = blocks.clone();
        let mut dominators =
            std::collections::HashMap::<BlockId, std::collections::HashSet<BlockId>>::new();
        for block in &all_blocks {
            if *block == self.entry {
                dominators.insert(*block, [*block].into_iter().collect());
            } else if reachable.contains(block) {
                dominators.insert(*block, all_blocks.clone());
            } else {
                dominators.insert(*block, [*block].into_iter().collect());
            }
        }
        let mut changed = true;
        while changed {
            changed = false;
            for block in reachable.iter().filter(|block| **block != self.entry) {
                let incoming = predecessors
                    .get(block)
                    .into_iter()
                    .flatten()
                    .filter(|pred| reachable.contains(pred));
                let Some(first) = incoming.clone().next() else {
                    continue;
                };
                let mut intersection = dominators[first].clone();
                for predecessor in incoming.skip(1) {
                    intersection.retain(|candidate| dominators[predecessor].contains(candidate));
                }
                intersection.insert(*block);
                if dominators[block] != intersection {
                    dominators.insert(*block, intersection);
                    changed = true;
                }
            }
        }

        let require_operand = |block: BlockId, index: usize, value: ValueId| {
            require(value)?;
            let (definition_block, definition_index) = definitions[&value];
            if definition_block == block {
                if definition_index >= index {
                    return Err(VerifyError::UseBeforeDefinition(value));
                }
            } else if !dominators[&block].contains(&definition_block) {
                return Err(VerifyError::UseNotDominated { value, block });
            }
            Ok(())
        };
        let require_phi_operand = |merge: BlockId, predecessor: BlockId, value: ValueId| {
            require(value)?;
            if !predecessors
                .get(&merge)
                .is_some_and(|incoming| incoming.contains(&predecessor))
            {
                return Err(VerifyError::InvalidPhiIncoming {
                    block: merge,
                    predecessor,
                });
            }
            let (definition_block, _) = definitions[&value];
            if definition_block != predecessor
                && !dominators[&predecessor].contains(&definition_block)
            {
                return Err(VerifyError::UseNotDominated {
                    value,
                    block: predecessor,
                });
            }
            Ok(())
        };
        for block in &self.blocks {
            for (index, instruction) in block.instructions.iter().enumerate() {
                match instruction {
                    Instruction::Add { left, right, .. }
                    | Instruction::Sub { left, right, .. }
                    | Instruction::Mul { left, right, .. }
                    | Instruction::Pow { left, right, .. }
                    | Instruction::Div { left, right, .. }
                    | Instruction::FloorDiv { left, right, .. }
                    | Instruction::Mod { left, right, .. }
                    | Instruction::BitAnd { left, right, .. }
                    | Instruction::BitOr { left, right, .. }
                    | Instruction::BitXor { left, right, .. }
                    | Instruction::Shl { left, right, .. }
                    | Instruction::Shr { left, right, .. }
                    | Instruction::CmpEq { left, right, .. }
                    | Instruction::CmpNe { left, right, .. }
                    | Instruction::CmpLe { left, right, .. }
                    | Instruction::CmpGt { left, right, .. }
                    | Instruction::CmpGe { left, right, .. }
                    | Instruction::CmpLt { left, right, .. } => {
                        require_operand(block.id, index, *left)?;
                        require_operand(block.id, index, *right)?;
                    }
                    Instruction::Neg { operand, .. }
                    | Instruction::BitNot { operand, .. }
                    | Instruction::Not { operand, .. } => {
                        require_operand(block.id, index, *operand)?
                    }
                    Instruction::And { left, right, .. } | Instruction::Or { left, right, .. } => {
                        require_operand(block.id, index, *left)?;
                        require_operand(block.id, index, *right)?;
                    }
                    Instruction::Phi { incomings, .. } => {
                        let mut seen_predecessors = std::collections::HashSet::new();
                        for (predecessor, value) in incomings {
                            if !seen_predecessors.insert(*predecessor) {
                                return Err(VerifyError::DuplicatePhiIncoming {
                                    block: block.id,
                                    predecessor: *predecessor,
                                });
                            }
                            require_phi_operand(block.id, *predecessor, *value)?;
                        }
                        for predecessor in predecessors.get(&block.id).into_iter().flatten() {
                            if !seen_predecessors.contains(predecessor) {
                                return Err(VerifyError::MissingPhiIncoming {
                                    block: block.id,
                                    predecessor: *predecessor,
                                });
                            }
                        }
                    }
                    Instruction::Param { .. }
                    | Instruction::ConstInt { .. }
                    | Instruction::ConstBool { .. } => {}
                }
            }
            match block.terminator {
                Terminator::Jump(target) => {
                    debug_assert!(blocks.contains(&target));
                }
                Terminator::Branch {
                    condition,
                    then_block,
                    else_block,
                } => {
                    require_operand(block.id, block.instructions.len(), condition)?;
                    debug_assert!(blocks.contains(&then_block));
                    debug_assert!(blocks.contains(&else_block));
                }
                Terminator::Return(value) => {
                    if let Some(value) = value {
                        require_operand(block.id, block.instructions.len(), value)?;
                    }
                }
            }
        }
        Ok(())
    }

    /// Execute the verified integer subset of CIR. This reference path is
    /// intentionally small; it supplies a deterministic oracle while richer
    /// instructions are added and later consumed by native lowering.
    pub fn execute(&self) -> Result<Option<i64>, ExecuteError> {
        self.execute_with_args_and_step_limit_impl(&[], 1_000_000)
    }

    /// Execute with positional integer arguments. This is the interpreter
    /// seam used by parameterized CIR functions; the zero-argument
    /// [`Self::execute`] convenience method remains unchanged.
    pub fn execute_with_args(&self, args: &[i64]) -> Result<Option<i64>, ExecuteError> {
        self.execute_with_args_and_step_limit_impl(args, 1_000_000)
    }

    /// Execute with positional arguments and an explicit instruction-step
    /// budget. This is the bounded counterpart to [`Self::execute_with_args`]
    /// and keeps parameterized cyclic programs deterministic for fuzzers and
    /// compiler diagnostics.
    pub fn execute_with_args_and_step_limit(
        &self,
        args: &[i64],
        step_limit: usize,
    ) -> Result<Option<i64>, ExecuteError> {
        self.execute_with_args_and_step_limit_impl(args, step_limit)
    }

    /// Execute with an explicit instruction-step budget. This keeps malformed
    /// or cyclic CIR programs deterministic for callers such as fuzzers and
    /// compiler diagnostics.
    pub fn execute_with_step_limit(&self, step_limit: usize) -> Result<Option<i64>, ExecuteError> {
        self.execute_with_args_and_step_limit_impl(&[], step_limit)
    }

    fn execute_with_args_and_step_limit_impl(
        &self,
        args: &[i64],
        step_limit: usize,
    ) -> Result<Option<i64>, ExecuteError> {
        self.verify().map_err(ExecuteError::Invalid)?;
        let blocks = self
            .blocks
            .iter()
            .map(|block| (block.id, block))
            .collect::<std::collections::HashMap<_, _>>();
        let parameter_indices = self
            .blocks
            .iter()
            .flat_map(|block| &block.instructions)
            .filter_map(|instruction| match instruction {
                Instruction::Param { index, .. } => Some(*index as usize),
                _ => None,
            })
            .collect::<Vec<_>>();
        let expected_arguments = parameter_indices
            .iter()
            .copied()
            .max()
            .map_or(0, |index| index + 1);
        // A CIR function with no Param instructions has no arity metadata;
        // preserve the existing void/unused-parameter behavior for that
        // representation.  Once parameters are materialized, reject extras
        // rather than silently discarding caller arguments.
        if !parameter_indices.is_empty() && args.len() > expected_arguments {
            return Err(ExecuteError::UnexpectedArgumentCount {
                expected: expected_arguments,
                provided: args.len(),
            });
        }
        let mut values = std::collections::HashMap::<ValueId, i64>::new();
        let mut current = self.entry;
        let mut previous = None;
        let mut steps = 0usize;
        loop {
            let block = blocks[&current];
            for instruction in &block.instructions {
                if steps >= step_limit {
                    return Err(ExecuteError::StepLimitExceeded);
                }
                steps += 1;
                match instruction {
                    Instruction::Param { result, index } => {
                        let value = args
                            .get(*index as usize)
                            .copied()
                            .ok_or(ExecuteError::MissingArgument(*index))?;
                        values.insert(*result, value);
                    }
                    Instruction::ConstInt { result, value } => {
                        values.insert(*result, *value);
                    }
                    Instruction::ConstBool { result, value } => {
                        values.insert(*result, i64::from(*value));
                    }
                    Instruction::Add {
                        result,
                        left,
                        right,
                    }
                    | Instruction::Sub {
                        result,
                        left,
                        right,
                    }
                    | Instruction::Mul {
                        result,
                        left,
                        right,
                    }
                    | Instruction::Pow {
                        result,
                        left,
                        right,
                    }
                    | Instruction::Div {
                        result,
                        left,
                        right,
                    }
                    | Instruction::FloorDiv {
                        result,
                        left,
                        right,
                    }
                    | Instruction::Mod {
                        result,
                        left,
                        right,
                    } => {
                        let value = match instruction {
                            Instruction::Add { .. } => values[left]
                                .checked_add(values[right])
                                .ok_or(ExecuteError::ArithmeticOverflow)?,
                            Instruction::Sub { .. } => values[left]
                                .checked_sub(values[right])
                                .ok_or(ExecuteError::ArithmeticOverflow)?,
                            Instruction::Mul { .. } => values[left]
                                .checked_mul(values[right])
                                .ok_or(ExecuteError::ArithmeticOverflow)?,
                            Instruction::Pow { .. } => {
                                let exponent = u32::try_from(values[right])
                                    .map_err(|_| ExecuteError::NegativeExponent)?;
                                values[left]
                                    .checked_pow(exponent)
                                    .ok_or(ExecuteError::ArithmeticOverflow)?
                            }
                            Instruction::Div { .. } => {
                                if values[right] == 0 {
                                    return Err(ExecuteError::DivisionByZero);
                                }
                                values[left]
                                    .checked_div(values[right])
                                    .ok_or(ExecuteError::ArithmeticOverflow)?
                            }
                            Instruction::FloorDiv { .. } => {
                                if values[right] == 0 {
                                    return Err(ExecuteError::DivisionByZero);
                                }
                                values[left]
                                    .checked_div_euclid(values[right])
                                    .ok_or(ExecuteError::ArithmeticOverflow)?
                            }
                            Instruction::Mod { .. } => {
                                if values[right] == 0 {
                                    return Err(ExecuteError::DivisionByZero);
                                }
                                // Lucid follows Python-style integer
                                // arithmetic: `%` pairs with floor
                                // division, so derive the remainder from
                                // the checked Euclidean quotient. Rust's
                                // `rem`/`rem_euclid` cannot represent the
                                // divisor-sign rule for negative divisors.
                                let dividend = values[left];
                                let divisor = values[right];
                                let truncated = dividend
                                    .checked_div(divisor)
                                    .ok_or(ExecuteError::ArithmeticOverflow)?;
                                let truncated_remainder = dividend
                                    .checked_rem(divisor)
                                    .ok_or(ExecuteError::ArithmeticOverflow)?;
                                let quotient = if truncated_remainder != 0
                                    && (dividend < 0) != (divisor < 0)
                                {
                                    truncated
                                        .checked_sub(1)
                                        .ok_or(ExecuteError::ArithmeticOverflow)?
                                } else {
                                    truncated
                                };
                                let product = quotient
                                    .checked_mul(values[right])
                                    .ok_or(ExecuteError::ArithmeticOverflow)?;
                                values[left]
                                    .checked_sub(product)
                                    .ok_or(ExecuteError::ArithmeticOverflow)?
                            }
                            _ => return Err(ExecuteError::UnsupportedInstruction),
                        };
                        values.insert(*result, value);
                    }
                    Instruction::BitAnd {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, values[left] & values[right]);
                    }
                    Instruction::BitOr {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, values[left] | values[right]);
                    }
                    Instruction::BitXor {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, values[left] ^ values[right]);
                    }
                    Instruction::Shl {
                        result,
                        left,
                        right,
                    } => {
                        let shift = u32::try_from(values[right]).ok();
                        let value = shift
                            .and_then(|shift| values[left].checked_shl(shift))
                            .ok_or(ExecuteError::ArithmeticOverflow)?;
                        values.insert(*result, value);
                    }
                    Instruction::Shr {
                        result,
                        left,
                        right,
                    } => {
                        let shift = u32::try_from(values[right]).ok();
                        let value = shift
                            .and_then(|shift| values[left].checked_shr(shift))
                            .ok_or(ExecuteError::ArithmeticOverflow)?;
                        values.insert(*result, value);
                    }
                    Instruction::Neg { result, operand } => {
                        let value = values[operand]
                            .checked_neg()
                            .ok_or(ExecuteError::ArithmeticOverflow)?;
                        values.insert(*result, value);
                    }
                    Instruction::BitNot { result, operand } => {
                        values.insert(*result, !values[operand]);
                    }
                    Instruction::Not { result, operand } => {
                        values.insert(*result, i64::from(values[operand] == 0));
                    }
                    Instruction::And {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, i64::from(values[left] != 0 && values[right] != 0));
                    }
                    Instruction::Or {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, i64::from(values[left] != 0 || values[right] != 0));
                    }
                    Instruction::Phi { result, incomings } => {
                        let predecessor = previous.ok_or({
                            ExecuteError::Invalid(VerifyError::InvalidPhiIncoming {
                                block: block.id,
                                predecessor: block.id,
                            })
                        })?;
                        let (_, value) = incomings
                            .iter()
                            .find(|(incoming, _)| *incoming == predecessor)
                            .ok_or(ExecuteError::Invalid(VerifyError::InvalidPhiIncoming {
                                block: block.id,
                                predecessor,
                            }))?;
                        values.insert(*result, values[value]);
                    }
                    Instruction::CmpEq {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, i64::from(values[left] == values[right]));
                    }
                    Instruction::CmpNe {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, i64::from(values[left] != values[right]));
                    }
                    Instruction::CmpLe {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, i64::from(values[left] <= values[right]));
                    }
                    Instruction::CmpGt {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, i64::from(values[left] > values[right]));
                    }
                    Instruction::CmpGe {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, i64::from(values[left] >= values[right]));
                    }
                    Instruction::CmpLt {
                        result,
                        left,
                        right,
                    } => {
                        values.insert(*result, i64::from(values[left] < values[right]));
                    }
                }
            }
            if steps >= step_limit {
                return Err(ExecuteError::StepLimitExceeded);
            }
            steps += 1;
            match block.terminator {
                Terminator::Jump(target) => {
                    previous = Some(current);
                    current = target;
                }
                Terminator::Branch {
                    condition,
                    then_block,
                    else_block,
                } => {
                    previous = Some(current);
                    current = if values[&condition] != 0 {
                        then_block
                    } else {
                        else_block
                    };
                }
                Terminator::Return(value) => return Ok(value.map(|value| values[&value])),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn valid() -> Function {
        Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 1,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 2,
                    },
                    Instruction::Add {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        }
    }

    #[test]
    fn verifies_valid_function() {
        assert!(valid().verify().is_ok());
    }

    #[test]
    fn rejects_missing_block_target() {
        let mut function = valid();
        function.blocks[0].terminator = Terminator::Jump(BlockId(9));
        assert_eq!(
            function.verify(),
            Err(VerifyError::MissingBlock(BlockId(9)))
        );
    }

    #[test]
    fn rejects_undefined_value() {
        let mut function = valid();
        function.blocks[0].terminator = Terminator::Return(Some(ValueId(9)));
        assert_eq!(
            function.verify(),
            Err(VerifyError::UndefinedValue(ValueId(9)))
        );
    }

    #[test]
    fn rejects_use_before_definition_within_block() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Add {
                        result: ValueId(1),
                        left: ValueId(0),
                        right: ValueId(0),
                    },
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 1,
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(1))),
            }],
        };
        assert_eq!(
            function.verify(),
            Err(VerifyError::UseBeforeDefinition(ValueId(0)))
        );
    }

    #[test]
    fn rejects_value_defined_on_sibling_branch() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: vec![Instruction::ConstBool {
                        result: ValueId(0),
                        value: true,
                    }],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(1),
                        value: 7,
                    }],
                    terminator: Terminator::Return(None),
                },
                Block {
                    id: BlockId(2),
                    instructions: vec![],
                    terminator: Terminator::Return(Some(ValueId(1))),
                },
            ],
        };
        assert_eq!(
            function.verify(),
            Err(VerifyError::UseNotDominated {
                value: ValueId(1),
                block: BlockId(2)
            })
        );
    }

    #[test]
    fn rejects_duplicate_phi_incoming_edges() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: vec![Instruction::ConstBool {
                        result: ValueId(0),
                        value: true,
                    }],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(1),
                        value: 1,
                    }],
                    terminator: Terminator::Jump(BlockId(3)),
                },
                Block {
                    id: BlockId(2),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(2),
                        value: 2,
                    }],
                    terminator: Terminator::Jump(BlockId(3)),
                },
                Block {
                    id: BlockId(3),
                    instructions: vec![Instruction::Phi {
                        result: ValueId(3),
                        incomings: vec![
                            (BlockId(1), ValueId(1)),
                            (BlockId(1), ValueId(2)),
                            (BlockId(2), ValueId(2)),
                        ],
                    }],
                    terminator: Terminator::Return(Some(ValueId(3))),
                },
            ],
        };
        assert_eq!(
            function.verify(),
            Err(VerifyError::DuplicatePhiIncoming {
                block: BlockId(3),
                predecessor: BlockId(1)
            })
        );
    }

    #[test]
    fn rejects_missing_phi_incoming_edge() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: vec![Instruction::ConstBool {
                        result: ValueId(0),
                        value: true,
                    }],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(1),
                        value: 1,
                    }],
                    terminator: Terminator::Jump(BlockId(3)),
                },
                Block {
                    id: BlockId(2),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(2),
                        value: 2,
                    }],
                    terminator: Terminator::Jump(BlockId(3)),
                },
                Block {
                    id: BlockId(3),
                    instructions: vec![Instruction::Phi {
                        result: ValueId(3),
                        incomings: vec![(BlockId(1), ValueId(1))],
                    }],
                    terminator: Terminator::Return(Some(ValueId(3))),
                },
            ],
        };
        assert_eq!(
            function.verify(),
            Err(VerifyError::MissingPhiIncoming {
                block: BlockId(3),
                predecessor: BlockId(2)
            })
        );
    }

    #[test]
    fn rejects_phi_after_an_ordinary_instruction() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 1,
                    },
                    Instruction::Phi {
                        result: ValueId(1),
                        incomings: vec![],
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(1))),
            }],
        };
        assert_eq!(
            function.verify(),
            Err(VerifyError::PhiAfterNonPhi { block: BlockId(0) })
        );
    }

    #[test]
    fn verification_does_not_depend_on_block_vector_order() {
        let function = Function {
            entry: BlockId(1),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(1),
                        value: 9,
                    }],
                    terminator: Terminator::Return(Some(ValueId(1))),
                },
                Block {
                    id: BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(0),
                        value: 1,
                    }],
                    terminator: Terminator::Jump(BlockId(0)),
                },
            ],
        };
        assert!(function.verify().is_ok());
        assert_eq!(function.execute(), Ok(Some(9)));
    }

    #[test]
    fn arithmetic_overflow_is_reported_without_panicking() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: i64::MAX,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 1,
                    },
                    Instruction::Add {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        assert_eq!(function.execute(), Err(ExecuteError::ArithmeticOverflow));
    }

    #[test]
    fn executes_integer_control_flow() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(0),
                        value: 1,
                    }],
                    terminator: Terminator::Branch {
                        condition: ValueId(0),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(1),
                        value: 7,
                    }],
                    terminator: Terminator::Return(Some(ValueId(1))),
                },
                Block {
                    id: BlockId(2),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(2),
                        value: 9,
                    }],
                    terminator: Terminator::Return(Some(ValueId(2))),
                },
            ],
        };
        assert_eq!(function.execute(), Ok(Some(7)));
    }

    #[test]
    fn lowers_and_executes_parameter_counted_while_loop() {
        let module = lucid_syntax::parse(
            r#"while n > 0:
    n -= 1
return n
"#,
        )
        .expect("loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("canonical counted loop should lower");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(0)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"while n != 0:
    n -= 1
return n
"#,
        )
        .expect("not-equal loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("not-equal counted loop should lower");
        assert_eq!(function.execute_with_args(&[3]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"while n > 0:
    n -= 1
    continue
return n
"#,
        )
        .expect("continue loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("trailing continue should lower with the induction loop");
        assert_eq!(function.execute_with_args(&[3]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"while n > 0:
    n -= 1
    pass
    pass
return n
"#,
        )
        .expect("pass-pass loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("pass/pass tail should lower with the induction loop");
        assert_eq!(function.execute_with_args(&[3]), Ok(Some(0)));

        let module = lucid_syntax::parse("while n > 0:\n    n -= 1\n    pass\nreturn n\n")
            .expect("pass loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("trailing pass should lower with the induction loop");
        assert_eq!(function.execute_with_args(&[3]), Ok(Some(0)));

        let module =
            lucid_syntax::parse("while n > 0:\n    n -= 1\nif_broken:\n    pass\nreturn n\n")
                .expect("if_broken pass fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("if_broken pass should lower with the induction loop");
        assert_eq!(function.execute_with_args(&[3]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"while n > 0:
    n = n - 1
return n
"#,
        )
        .expect("ordinary-update loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("ordinary-update counted loop should lower");
        assert_eq!(function.execute_with_args(&[3]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"while n > 0:
    n += -1
return n
"#,
        )
        .expect("signed-update loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("signed-update counted loop should lower");
        assert_eq!(function.execute_with_args(&[3]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"while n > 0:
    n = -1 + n
return n
"#,
        )
        .expect("commuted-update loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("commuted-update counted loop should lower");
        assert_eq!(function.execute_with_args(&[3]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"value = -3
while value < 0:
    value += 1
return value
"#,
        )
        .expect("signed-initializer loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &[])
            .expect("signed-initializer counted loop should lower");
        assert_eq!(function.execute(), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"while n > -2:
    n -= 1
return n
"#,
        )
        .expect("signed-bound loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("signed-bound counted loop should lower");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(-2)));

        let module = lucid_syntax::parse(
            r#"while n > limit:
    n -= 1
return n
"#,
        )
        .expect("parameter-bound loop fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "limit".into()])
                .expect("parameter-bound counted loop should lower");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(Some(2)));
        assert_eq!(function.execute_with_args(&[1, 2]), Ok(Some(1)));

        let module = lucid_syntax::parse(
            r#"value = n
stop = limit
while value > stop:
    value -= 1
return value
"#,
        )
        .expect("local-bound counted loop fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "limit".into()])
                .expect("local-bound counted loop should lower");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(Some(2)));

        let module = lucid_syntax::parse(
            r#"while n > 0:
    n -= 1
"#,
        )
        .expect("void loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("void counted loop should lower");
        assert_eq!(function.execute_with_args(&[3]), Ok(None));

        let module = lucid_syntax::parse(
            r#"value = n
stop = limit
while value > stop:
    value -= 1
"#,
        )
        .expect("void local-bound counted loop fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "limit".into()])
                .expect("void local-bound counted loop should lower");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(None));

        let module = lucid_syntax::parse(
            r#"value = n
while value > 0:
    value -= 1
return value
"#,
        )
        .expect("local-init loop fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("local-init counted loop should lower");
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"total = 0
while n > 0:
    total += 1
    n -= 1
return total
"#,
        )
        .expect("while accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("while accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(4)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"total = 0
while n > 0:
    total += 1
    n -= 1
    pass
    pass
return total
"#,
        )
        .expect("while accumulator pass-pass fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("while accumulator pass/pass tail should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(4)));

        let module = lucid_syntax::parse(
            r#"total = seed
while n > 0:
    total = total + 2
    n = n - 1
return total
"#,
        )
        .expect("while parameter-seeded accumulator fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "seed".into()])
                .expect("parameter-seeded while accumulator should lower");
        assert_eq!(function.execute_with_args(&[3, 7]), Ok(Some(13)));

        let module = lucid_syntax::parse(
            r#"total = 0
while n > 0:
    total += -1
    n -= 1
return total
"#,
        )
        .expect("signed-literal while accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("signed-literal while accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(-4)));

        let module = lucid_syntax::parse(
            r#"total = -1
while n > 0:
    total += n
    n += -1
return total
"#,
        )
        .expect("signed-update while accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("signed-update while accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(9)));

        let module = lucid_syntax::parse(
            r#"total = -1
while n > 0:
    total += n
    n = -1 + n
return total
"#,
        )
        .expect("commuted-update while accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("commuted-update while accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(9)));

        let module = lucid_syntax::parse(
            r#"total = 0
while n > -3:
    total += n
    n -= 1
return total
"#,
        )
        .expect("signed-bound while accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("signed-bound while accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(-3)));

        let module = lucid_syntax::parse(
            r#"total = 0
while n > 0:
    total += n
    n -= 1
return total
"#,
        )
        .expect("while induction accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("induction accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = seed
while n > 0:
    total = total + n
    n = n - 1
return total
"#,
        )
        .expect("ordinary induction accumulator fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "seed".into()])
                .expect("ordinary induction accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[4, 10]), Ok(Some(20)));

        let module = lucid_syntax::parse(
            r#"total = 0
while n > 0:
    total = n + total
    n -= 1
return total
"#,
        )
        .expect("commuted induction accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("commuted induction accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = 0
while n > limit:
    total += n
    n -= 1
return total
"#,
        )
        .expect("parameter-bound while accumulator fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "limit".into()])
                .expect("parameter-bound while accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(Some(12)));
        assert_eq!(function.execute_with_args(&[1, 2]), Ok(Some(0)));

        let module = lucid_syntax::parse(
            r#"total = 0
value = n
while value > 0:
    total += value
    value -= 1
return total
"#,
        )
        .expect("local-induction while accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("local-induction while accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"value = n
total = 0
while value > 0:
    total += value
    value -= 1
return total
"#,
        )
        .expect("reordered local-induction accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("reordered local-induction accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[4]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = seed
let value = n
while value > limit:
    total = total + value
    value = value - 1
return total
"#,
        )
        .expect("parameter-bound local-induction accumulator fixture should parse");
        let function = Function::from_module_linear_with_params(
            &module,
            &["n".into(), "limit".into(), "seed".into()],
        )
        .expect("parameter-bound local-induction accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[5, 2, 10]), Ok(Some(22)));

        let module = lucid_syntax::parse(
            r#"total = 0
value = n
stop = limit
while value > stop:
    total += value
    value -= 1
return total
"#,
        )
        .expect("local-bound while accumulator fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "limit".into()])
                .expect("local-bound while accumulator should lower through CIR");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(Some(12)));

        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total += i
return total
"#,
        )
        .expect("range accumulation fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("range accumulation should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total = i + total
return total
"#,
        )
        .expect("commuted range accumulation fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("commuted range accumulation should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total += 2
return total
"#,
        )
        .expect("literal-step range accumulation fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("literal-step range accumulation should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total += -2
return total
"#,
        )
        .expect("signed-literal range accumulation fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("signed-literal range accumulation should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(-10)));

        let module = lucid_syntax::parse(
            r#"total = 0
limit = n
for i in range(limit):
    total += i
return total
"#,
        )
        .expect("range alias accumulation fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("range alias accumulation should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = 0
begin = seed
for i in range(begin, n):
    total += i
return total
"#,
        )
        .expect("range start alias accumulation fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "seed".into()])
                .expect("range start alias accumulation should lower");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(Some(9)));

        let module = lucid_syntax::parse(
            r#"begin = seed
total = 0
for i in range(begin, n):
    total += i
return total
"#,
        )
        .expect("range reordered start alias accumulation fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "seed".into()])
                .expect("range reordered start alias accumulation should lower");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(Some(9)));

        let module = lucid_syntax::parse(
            r#"total = 0
start = seed
begin = start
for i in range(begin, n):
    total += i
return total
"#,
        )
        .expect("range chained start alias accumulation fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "seed".into()])
                .expect("range chained start alias accumulation should lower");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(Some(9)));

        let module = lucid_syntax::parse(
            r#"total = 0
start = seed
middle = start
begin = middle
for i in range(begin, n):
    total += i
return total
"#,
        )
        .expect("range long chained start alias accumulation fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "seed".into()])
                .expect("range long chained start alias accumulation should lower");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(Some(9)));

        let module = lucid_syntax::parse(
            r#"total = 0
begin = seed
stop = limit
for i in range(begin, stop):
    total += i
return total
"#,
        )
        .expect("range two-alias accumulation fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["seed".into(), "limit".into()])
                .expect("range two-alias accumulation should lower");
        assert_eq!(function.execute_with_args(&[2, 6]), Ok(Some(14)));

        let module = lucid_syntax::parse(
            r#"begin = seed
stop = limit
total = 0
for i in range(begin, stop):
    total += i
return total
"#,
        )
        .expect("range reordered two-alias accumulation fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["seed".into(), "limit".into()])
                .expect("range reordered two-alias accumulation should lower");
        assert_eq!(function.execute_with_args(&[2, 6]), Ok(Some(14)));

        let module = lucid_syntax::parse(
            r#"total = 0
stop = limit
for i in range(n, stop, -1):
    total += i
return total
"#,
        )
        .expect("descending range alias accumulation fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "limit".into()])
                .expect("descending range alias accumulation should lower");
        assert_eq!(function.execute_with_args(&[5, 2]), Ok(Some(12)));

        let module = lucid_syntax::parse(
            r#"total = 0
unused = n
for i in range(n):
    total += i
return total
"#,
        )
        .expect("unused range alias fixture should parse");
        assert_eq!(
            Function::from_module_linear_with_params(&module, &["n".into()]),
            Err(LowerError::UnsupportedExpression)
        );

        let module = lucid_syntax::parse(
            r#"total = 0
begin = stop
stop = begin
for i in range(begin, n):
    total += i
return total
"#,
        )
        .expect("cyclic range alias fixture should parse");
        assert_eq!(
            Function::from_module_linear_with_params(&module, &["n".into()]),
            Err(LowerError::UnsupportedExpression)
        );

        let module =
            lucid_syntax::parse("total = 20\nfor i in range(n):\n    total -= i\nreturn total\n")
                .expect("range subtraction fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("range subtraction should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total += i
    continue
return total
"#,
        )
        .expect("range continue fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("trailing continue in range loop should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(n):
    total += i
    pass
    pass
return total
"#,
        )
        .expect("range pass-pass fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("trailing pass/pass in range loop should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            "total = 0\nfor i in range(n):\n    total += i\n    pass\nreturn total\n",
        )
        .expect("range pass fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("trailing pass in range loop should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            "total = 0\nfor i in range(n):\n    total += i\nif_broken:\n    pass\nreturn total\n",
        )
        .expect("range if_broken pass fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &["n".into()])
            .expect("if_broken pass in range loop should lower");
        assert_eq!(function.execute_with_args(&[5]), Ok(Some(10)));

        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(5, 1, -2):
    total += i
return total
"#,
        )
        .expect("descending range fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &[])
            .expect("descending range accumulation should lower");
        assert_eq!(function.execute(), Ok(Some(8)));

        let module = lucid_syntax::parse(
            r#"let total = 0
for i in range(4):
    total += i
return total
"#,
        )
        .expect("let range fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &[])
            .expect("let range accumulation should lower");
        assert_eq!(function.execute(), Ok(Some(6)));

        let module = lucid_syntax::parse(
            r#"total = 0
for i in range(4):
    total = total + i
return total
"#,
        )
        .expect("ordinary-assignment range fixture should parse");
        let function = Function::from_module_linear_with_params(&module, &[])
            .expect("ordinary-assignment range accumulation should lower");
        assert_eq!(function.execute(), Ok(Some(6)));

        let module = lucid_syntax::parse(
            r#"total = seed
for i in range(n):
    total += i
return total
"#,
        )
        .expect("parameter-seeded range fixture should parse");
        let function =
            Function::from_module_linear_with_params(&module, &["n".into(), "seed".into()])
                .expect("parameter-seeded range accumulation should lower");
        assert_eq!(function.execute_with_args(&[5, 7]), Ok(Some(17)));
    }

    #[test]
    fn custom_step_limit_bounds_cyclic_control_flow() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![],
                terminator: Terminator::Jump(BlockId(0)),
            }],
        };
        assert_eq!(
            function.execute_with_step_limit(3),
            Err(ExecuteError::StepLimitExceeded)
        );
    }

    #[test]
    fn step_limit_counts_instructions_and_terminators() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![Instruction::ConstInt {
                    result: ValueId(0),
                    value: 7,
                }],
                terminator: Terminator::Return(Some(ValueId(0))),
            }],
        };
        assert_eq!(
            function.execute_with_step_limit(1),
            Err(ExecuteError::StepLimitExceeded)
        );
        assert_eq!(function.execute_with_step_limit(2), Ok(Some(7)));
    }

    #[test]
    fn recoverable_operation_detection_is_centralized_in_cir() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 8,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 2,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        assert!(function.has_recoverable_operations());
        assert!(function.has_division_operations());
        let safe = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![Instruction::ConstInt {
                    result: ValueId(0),
                    value: 1,
                }],
                terminator: Terminator::Return(Some(ValueId(0))),
            }],
        };
        assert!(!safe.has_recoverable_operations());
        assert!(!safe.has_division_operations());
        let modulo = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 5,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 2,
                    },
                    Instruction::Mod {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        assert!(modulo.has_recoverable_operations());
        assert!(modulo.has_division_operations());
    }

    #[test]
    fn executes_integer_arithmetic_subset() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 6,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 4,
                    },
                    Instruction::Sub {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                    Instruction::Mul {
                        result: ValueId(3),
                        left: ValueId(2),
                        right: ValueId(1),
                    },
                    Instruction::Neg {
                        result: ValueId(4),
                        operand: ValueId(3),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(4))),
            }],
        };
        assert_eq!(function.execute(), Ok(Some(-8)));
    }

    #[test]
    fn remainder_matches_floor_division_for_negative_operands() {
        for (source, expected) in [("value = -5 % 2\n", 1), ("value = 5 % -2\n", -1)] {
            let module = lucid_syntax::parse(source).unwrap();
            assert_eq!(
                Function::from_module(&module).unwrap().execute(),
                Ok(Some(expected))
            );
        }
    }

    #[test]
    fn lowers_ast_arithmetic_to_verified_cir() {
        let module = lucid_syntax::parse("value = 2 + 3 * 4\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        let function = Function::from_expr(value).expect("arithmetic should lower");
        assert_eq!(function.execute(), Ok(Some(14)));
    }

    #[test]
    fn lowers_typed_hir_graph_without_ast_access() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(2)),
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(3)),
            },
            TypedExprNode {
                id: 2,
                kind: "binary".into(),
                detail: Some("Mul".into()),
                children: vec![0, 1],
                literal: None,
            },
        ];
        let function =
            Function::from_typed_initializers(&nodes, &[2]).expect("typed graph should lower");
        assert_eq!(function.execute(), Ok(Some(6)));
    }

    #[test]
    fn lowers_typed_hir_local_alias_chain_to_ssa_values() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("value".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(2)),
            },
            TypedExprNode {
                id: 2,
                kind: "binary".into(),
                detail: Some("Mul".into()),
                children: vec![0, 1],
                literal: None,
            },
            TypedExprNode {
                id: 3,
                kind: "name".into(),
                detail: Some("doubled".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 4,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(1)),
            },
            TypedExprNode {
                id: 5,
                kind: "binary".into(),
                detail: Some("Add".into()),
                children: vec![3, 4],
                literal: None,
            },
            TypedExprNode {
                id: 6,
                kind: "name".into(),
                detail: Some("result".into()),
                children: vec![],
                literal: None,
            },
        ];
        let function = Function::from_typed_function_body_with_locals(
            &nodes,
            6,
            &["value".into()],
            &[("doubled".into(), 2), ("result".into(), 5)],
        )
        .expect("local aliases should lower");
        assert_eq!(function.execute_with_args(&[20]), Ok(Some(41)));
    }

    #[test]
    fn executes_parameterized_cir_with_explicit_arguments() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::Param {
                        result: ValueId(0),
                        index: 0,
                    },
                    Instruction::Param {
                        result: ValueId(1),
                        index: 1,
                    },
                    Instruction::Add {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        assert_eq!(function.execute_with_args(&[20, 22]), Ok(Some(42)));
        assert_eq!(function.execute(), Err(ExecuteError::MissingArgument(0)));
        assert_eq!(
            function.execute_with_args(&[20, 22, 24]),
            Err(ExecuteError::UnexpectedArgumentCount {
                expected: 2,
                provided: 3,
            })
        );
        let cyclic = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![Instruction::Param {
                    result: ValueId(0),
                    index: 0,
                }],
                terminator: Terminator::Jump(BlockId(0)),
            }],
        };
        assert_eq!(
            cyclic.execute_with_args_and_step_limit(&[1], 3),
            Err(ExecuteError::StepLimitExceeded)
        );
    }

    #[test]
    fn folds_constant_if_in_typed_hir_graph() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Bool(true)),
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(11)),
            },
            TypedExprNode {
                id: 2,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(22)),
            },
            TypedExprNode {
                id: 3,
                kind: "if".into(),
                detail: None,
                children: vec![0, 1, 2],
                literal: None,
            },
        ];
        let function =
            Function::from_typed_initializers(&nodes, &[3]).expect("constant if should lower");
        assert_eq!(function.execute(), Ok(Some(11)));
    }

    #[test]
    fn lowers_dynamic_if_in_typed_hir_to_cfg() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("flag".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(11)),
            },
            TypedExprNode {
                id: 2,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(22)),
            },
            TypedExprNode {
                id: 3,
                kind: "if".into(),
                detail: None,
                children: vec![0, 1, 2],
                literal: None,
            },
        ];
        let function = Function::from_typed_function_body(&nodes, 3, &["flag".into()])
            .expect("dynamic typed conditional should lower to a diamond");
        assert_eq!(function.blocks.len(), 4);
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(22)));
    }

    #[test]
    fn lowers_typed_short_circuit_logic_to_cfg() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("flag".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Bool(true)),
            },
            TypedExprNode {
                id: 2,
                kind: "binary".into(),
                detail: Some("And".into()),
                children: vec![0, 1],
                literal: None,
            },
        ];
        let function = Function::from_typed_function_body(&nodes, 2, &["flag".into()])
            .expect("typed and should lower as short-circuit control flow");
        assert_eq!(function.blocks.len(), 4);
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(0)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(1)));
    }

    #[test]
    fn lowers_typed_match_chain_with_expression_results() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("value".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(10)),
            },
            TypedExprNode {
                id: 2,
                kind: "binary".into(),
                detail: Some("Add".into()),
                children: vec![0, 1],
                literal: None,
            },
            TypedExprNode {
                id: 3,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(10)),
            },
            TypedExprNode {
                id: 4,
                kind: "binary".into(),
                detail: Some("Mul".into()),
                children: vec![0, 3],
                literal: None,
            },
            TypedExprNode {
                id: 5,
                kind: "unary".into(),
                detail: Some("Neg".into()),
                children: vec![0],
                literal: None,
            },
            TypedExprNode {
                id: 6,
                kind: "match-chain".into(),
                detail: Some("literal-chain:i1,i2".into()),
                children: vec![0, 2, 4, 5],
                literal: None,
            },
        ];
        let function = Function::from_typed_function_body(&nodes, 6, &["value".into()])
            .expect("typed match-chain should lower nonliteral arm results");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(20)));
        assert_eq!(function.execute_with_args(&[7]), Ok(Some(-7)));
    }

    #[test]
    fn lowers_typed_optional_match_chain_with_expression_results() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("value".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(10)),
            },
            TypedExprNode {
                id: 2,
                kind: "binary".into(),
                detail: Some("Add".into()),
                children: vec![0, 1],
                literal: None,
            },
            TypedExprNode {
                id: 3,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(10)),
            },
            TypedExprNode {
                id: 4,
                kind: "binary".into(),
                detail: Some("Mul".into()),
                children: vec![0, 3],
                literal: None,
            },
            TypedExprNode {
                id: 5,
                kind: "optional-match-chain".into(),
                detail: Some("literal-chain:i1,i2".into()),
                children: vec![0, 2, 4],
                literal: None,
            },
        ];
        let function = Function::from_typed_function_body(&nodes, 5, &["value".into()])
            .expect("typed optional match-chain should lower nonliteral arm results");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(20)));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));
    }

    #[test]
    fn lowers_typed_void_match_chain() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("value".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "void-match-chain".into(),
                detail: Some("literal-chain:i1,i2".into()),
                children: vec![0],
                literal: None,
            },
        ];
        let function = Function::from_typed_function_body(&nodes, 1, &["value".into()])
            .expect("typed void match-chain should lower");
        assert_eq!(function.execute_with_args(&[1]), Ok(None));
        assert_eq!(function.execute_with_args(&[2]), Ok(None));
        assert_eq!(function.execute_with_args(&[7]), Ok(None));
    }

    #[test]
    fn lowers_typed_unary_plus_as_identity() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(9)),
            },
            TypedExprNode {
                id: 1,
                kind: "unary".into(),
                detail: Some("Pos".into()),
                children: vec![0],
                literal: None,
            },
        ];
        let function = Function::from_typed_initializers(&nodes, &[1])
            .expect("typed unary plus should lower as an identity");
        assert_eq!(function.execute(), Ok(Some(9)));
    }

    #[test]
    fn folds_typed_short_circuit_without_executing_dead_operand() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Bool(false)),
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(1)),
            },
            TypedExprNode {
                id: 2,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(0)),
            },
            TypedExprNode {
                id: 3,
                kind: "binary".into(),
                detail: Some("Div".into()),
                children: vec![1, 2],
                literal: None,
            },
            TypedExprNode {
                id: 4,
                kind: "binary".into(),
                detail: Some("And".into()),
                children: vec![0, 3],
                literal: None,
            },
        ];
        let function = Function::from_typed_initializers(&nodes, &[4])
            .expect("constant short-circuit should lower");
        assert_eq!(function.execute(), Ok(Some(0)));
    }

    #[test]
    fn lowers_typed_identity_comparisons() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(4)),
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(4)),
            },
            TypedExprNode {
                id: 2,
                kind: "binary".into(),
                detail: Some("Is".into()),
                children: vec![0, 1],
                literal: None,
            },
        ];
        let function = Function::from_typed_initializers(&nodes, &[2])
            .expect("typed identity comparison should lower");
        assert_eq!(function.execute(), Ok(Some(1)));
    }

    #[test]
    fn lowers_typed_statement_if_for_both_branches() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("flag".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(11)),
            },
            TypedExprNode {
                id: 2,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(22)),
            },
        ];
        let parameters = vec!["flag".to_string()];
        let function = Function::from_typed_statement_if(&nodes, 0, 1, 2, &parameters, &[])
            .expect("typed statement if should lower");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(22)));
    }

    #[test]
    fn typed_statement_if_preserves_parameter_reads_in_branches() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("flag".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "name".into(),
                detail: Some("value".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 2,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(1)),
            },
            TypedExprNode {
                id: 3,
                kind: "binary".into(),
                detail: Some("Add".into()),
                children: vec![1, 2],
                literal: None,
            },
            TypedExprNode {
                id: 4,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(10)),
            },
        ];
        let parameters = vec!["flag".to_string(), "value".to_string()];
        let function = Function::from_typed_statement_if(&nodes, 0, 3, 4, &parameters, &[])
            .expect("parameter-backed statement if should lower");
        assert_eq!(function.execute_with_args(&[1, 41]), Ok(Some(42)));
        assert_eq!(function.execute_with_args(&[0, 41]), Ok(Some(10)));
    }

    #[test]
    fn typed_statement_if_rejects_duplicate_parameter_names() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("flag".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(1)),
            },
            TypedExprNode {
                id: 2,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(2)),
            },
        ];
        let parameters = vec!["flag".to_string(), "flag".to_string()];
        assert_eq!(
            Function::from_typed_statement_if(&nodes, 0, 1, 2, &parameters, &[]),
            Err(LowerError::UnsupportedExpression)
        );
    }

    #[test]
    fn typed_statement_if_optional_has_void_fallthrough() {
        let nodes = vec![
            TypedExprNode {
                id: 0,
                kind: "name".into(),
                detail: Some("flag".into()),
                children: vec![],
                literal: None,
            },
            TypedExprNode {
                id: 1,
                kind: "literal".into(),
                detail: None,
                children: vec![],
                literal: Some(TypedLiteral::Int(7)),
            },
        ];
        let parameters = vec!["flag".to_string()];
        let function = Function::from_typed_statement_if_optional(&nodes, 0, 1, &parameters, &[])
            .expect("optional typed statement if should lower");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(7)));
        assert_eq!(function.execute_with_args(&[0]), Ok(None));
    }

    #[test]
    fn rejects_malformed_typed_hir_graph() {
        let nodes = vec![TypedExprNode {
            id: 0,
            kind: "binary".into(),
            detail: Some("Add".into()),
            children: vec![1, 1],
            literal: None,
        }];
        assert_eq!(
            Function::from_typed_initializers(&nodes, &[0]),
            Err(LowerError::UnsupportedExpression)
        );
    }

    #[test]
    fn module_lowering_owns_assignment_selection() {
        let module = lucid_syntax::parse("value = 6 * 7\n").unwrap();
        assert_eq!(
            Function::from_module(&module).unwrap().execute(),
            Ok(Some(42))
        );
        let empty = lucid_syntax::parse("pass\n").unwrap();
        assert_eq!(Function::from_module(&empty).unwrap().execute(), Ok(None));
        let module = lucid_syntax::parse("first = 1\nsecond = first + 2\n").unwrap();
        assert_eq!(
            Function::from_module(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module = lucid_syntax::parse("value = 4\nreturn value + 1\n").unwrap();
        assert_eq!(
            Function::from_module(&module).unwrap().execute(),
            Ok(Some(5))
        );
        let module = lucid_syntax::parse("while false:\n    pass\nvalue = 7\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("statically false loops should fold in linear lowering")
                .execute(),
            Ok(Some(7))
        );
        let module = lucid_syntax::parse("for item in []:\n    pass\nvalue = 8\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("empty literal loops should fold in linear lowering")
                .execute(),
            Ok(Some(8))
        );
        let module = lucid_syntax::parse("for item in []:\n    return 1\nvalue = 9\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("empty literal loops should ignore unreachable returns")
                .execute(),
            Ok(Some(9))
        );
        let module = lucid_syntax::parse(
            "value = 8\nfor item in []:\n    value = 1\nif_broken:\n    value = 2\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("empty literal loop if_broken should not run")
                .execute(),
            Ok(Some(8))
        );
        let module = lucid_syntax::parse(
            "value = 7\nwhile false:\n    value = 1\nif_broken:\n    value = 2\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("statically false while if_broken should not run")
                .execute(),
            Ok(Some(7))
        );
        let module =
            lucid_syntax::parse("for item in range(0, 1, -1):\n    pass\nvalue = 9\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("empty range loops should fold in linear lowering")
                .execute(),
            Ok(Some(9))
        );
        let module =
            lucid_syntax::parse("for item in range(0):\n    return 1\nvalue = 11\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("empty range loops should ignore unreachable returns")
                .execute(),
            Ok(Some(11))
        );
        let module = lucid_syntax::parse("for item in range(-1):\n    pass\nvalue = 10\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("negative-stop range loops should fold in linear lowering")
                .execute(),
            Ok(Some(10))
        );
        let module =
            lucid_syntax::parse("for item in [1, 2, 3]:\n    value = item\nvalue = value + 1\n")
                .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("constant list loops should unroll in linear lowering")
                .execute(),
            Ok(Some(4))
        );
        let module =
            lucid_syntax::parse("value = 0\nfor item in [1, 2, 3]:\n    value = value + item\n")
                .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("constant list accumulation should unroll in order")
                .execute(),
            Ok(Some(6))
        );
        let module =
            lucid_syntax::parse("value = 0\nfor item in {1, 2, 3}:\n    value = value + item\n")
                .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("constant set loops should unroll in order")
                .execute(),
            Ok(Some(6))
        );
        let module =
            lucid_syntax::parse("value = 0\nfor item in {1: 2, 3: 4}:\n    value = value + item\n")
                .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("constant dictionary loops should iterate keys")
                .execute(),
            Ok(Some(4))
        );
        let module = lucid_syntax::parse("for item in {1: 1 / 0}:\n    pass\nvalue = 4\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("dictionary value expressions should lower")
                .execute(),
            Err(ExecuteError::DivisionByZero)
        );
        let module =
            lucid_syntax::parse("value = 0\nfor _ in [1, 2, 3]:\n    value = value + 1\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("wildcard constant loops should discard the binding")
                .execute(),
            Ok(Some(3))
        );
        let module =
            lucid_syntax::parse("value = 0\nfor 2 in [1, 2, 3, 2]:\n    value = value + 1\n")
                .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("literal constant loop patterns should filter elements")
                .execute(),
            Ok(Some(2))
        );
        let module =
            lucid_syntax::parse("for 4 in [1, 2, 3]:\n    return 1\nvalue = 12\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("literal loop patterns should ignore unreachable returns")
                .execute(),
            Ok(Some(12))
        );
        let module = lucid_syntax::parse(
            "value = 0\nfor true in [false, true, true]:\n    value = value + 1\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("boolean literal loop patterns should filter elements")
                .execute(),
            Ok(Some(2))
        );
        let module =
            lucid_syntax::parse("for true in [false]:\n    return 1\nvalue = 13\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("boolean literal loop patterns should ignore unreachable returns")
                .execute(),
            Ok(Some(13))
        );
        let module =
            lucid_syntax::parse("value = 0\nfor item in range(1, 4):\n    value = value + item\n")
                .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("constant range loops should unroll in order")
                .execute(),
            Ok(Some(6))
        );
        let module = lucid_syntax::parse(
            "value = 0\nfor item in range(1 + 1, 2 * 3, 1 << 1):\n    value = value + item\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("arithmetic constant range bounds should unroll")
                .execute(),
            Ok(Some(6))
        );
        let module =
            lucid_syntax::parse("for item in range(2048):\n    pass\nvalue = 13\n").unwrap();
        assert!(
            Function::from_module(&module).is_err(),
            "oversized constant ranges must not be unrolled without a bound"
        );
        let module = lucid_syntax::parse("assert(true)\nvalue = 11\n").unwrap();
        assert_eq!(
            Function::from_module(&module)
                .expect("a statically true assertion should be removed")
                .execute(),
            Ok(Some(11))
        );
        let module = lucid_syntax::parse("assert(false)\nvalue = 12\n").unwrap();
        assert!(
            Function::from_module(&module).is_err(),
            "a statically false assertion must not be erased"
        );
        let module = lucid_syntax::parse("return\n").unwrap();
        assert_eq!(Function::from_module(&module).unwrap().execute(), Ok(None));
    }

    #[test]
    fn serializes_functions_deterministically() {
        let function = valid();
        assert_eq!(
            function.to_text(),
            "entry b0\nb0:\n  ConstInt { result: ValueId(0), value: 1 }\n  ConstInt { result: ValueId(1), value: 2 }\n  Add { result: ValueId(2), left: ValueId(0), right: ValueId(1) }\n  Return(Some(ValueId(2)))\n"
        );
    }

    #[test]
    fn division_reports_zero_denominator() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: 1,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: 0,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        assert_eq!(function.execute(), Err(ExecuteError::DivisionByZero));
    }

    #[test]
    fn lowers_remainder_and_reports_zero_denominator() {
        let module = lucid_syntax::parse("value = 17 % 5\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert_eq!(Function::from_expr(value).unwrap().execute(), Ok(Some(2)));
    }

    #[test]
    fn lowers_floor_division_and_bitwise_operations() {
        for (source, expected) in [
            ("value = -7 // 3\n", -3),
            ("value = 5 & 3\n", 1),
            ("value = 5 | 2\n", 7),
            ("value = 5 ^ 1\n", 4),
            ("value = 3 << 2\n", 12),
            ("value = 12 >> 2\n", 3),
        ] {
            let module = lucid_syntax::parse(source).unwrap();
            let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
                panic!("expected assignment");
            };
            assert_eq!(
                Function::from_expr(value).unwrap().execute(),
                Ok(Some(expected))
            );
        }
    }

    #[test]
    fn lowers_integer_power_and_rejects_negative_exponents() {
        let module = lucid_syntax::parse("value = 2 ** 10\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert_eq!(
            Function::from_expr(value).unwrap().execute(),
            Ok(Some(1024))
        );

        let module = lucid_syntax::parse("value = 2 ** -1\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert_eq!(
            Function::from_expr(value).unwrap().execute(),
            Err(ExecuteError::NegativeExponent)
        );
    }

    #[test]
    fn division_overflow_is_reported_without_panicking() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![Block {
                id: BlockId(0),
                instructions: vec![
                    Instruction::ConstInt {
                        result: ValueId(0),
                        value: i64::MIN,
                    },
                    Instruction::ConstInt {
                        result: ValueId(1),
                        value: -1,
                    },
                    Instruction::Div {
                        result: ValueId(2),
                        left: ValueId(0),
                        right: ValueId(1),
                    },
                ],
                terminator: Terminator::Return(Some(ValueId(2))),
            }],
        };
        assert_eq!(function.execute(), Err(ExecuteError::ArithmeticOverflow));
    }

    #[test]
    fn executes_boolean_equality_branch_condition() {
        let function = Function {
            entry: BlockId(0),
            blocks: vec![
                Block {
                    id: BlockId(0),
                    instructions: vec![
                        Instruction::ConstInt {
                            result: ValueId(0),
                            value: 4,
                        },
                        Instruction::ConstInt {
                            result: ValueId(1),
                            value: 4,
                        },
                        Instruction::CmpEq {
                            result: ValueId(2),
                            left: ValueId(0),
                            right: ValueId(1),
                        },
                    ],
                    terminator: Terminator::Branch {
                        condition: ValueId(2),
                        then_block: BlockId(1),
                        else_block: BlockId(2),
                    },
                },
                Block {
                    id: BlockId(1),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(3),
                        value: 1,
                    }],
                    terminator: Terminator::Return(Some(ValueId(3))),
                },
                Block {
                    id: BlockId(2),
                    instructions: vec![Instruction::ConstInt {
                        result: ValueId(4),
                        value: 0,
                    }],
                    terminator: Terminator::Return(Some(ValueId(4))),
                },
            ],
        };
        assert_eq!(function.execute(), Ok(Some(1)));
    }

    #[test]
    fn lowers_ordered_comparison() {
        let module = lucid_syntax::parse("value = 2 < 3\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        let function = Function::from_expr(value).expect("comparison should lower");
        assert_eq!(function.execute(), Ok(Some(1)));
    }

    #[test]
    fn lowers_all_ordered_comparisons() {
        for (source, expected) in [
            ("value = 2 <= 2\n", 1),
            ("value = 3 > 2\n", 1),
            ("value = 3 >= 4\n", 0),
        ] {
            let module = lucid_syntax::parse(source).unwrap();
            let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
                panic!("expected assignment");
            };
            assert_eq!(
                Function::from_expr(value).unwrap().execute(),
                Ok(Some(expected))
            );
        }
    }

    #[test]
    fn lowers_inequality() {
        let module = lucid_syntax::parse("value = 2 != 3\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert_eq!(Function::from_expr(value).unwrap().execute(), Ok(Some(1)));

        let module = lucid_syntax::parse("value = 2 !== 3\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert_eq!(Function::from_expr(value).unwrap().execute(), Ok(Some(1)));
    }

    #[test]
    fn lowers_unary_negation() {
        let module = lucid_syntax::parse("value = -5\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        let function = Function::from_expr(value).expect("negation should lower");
        assert_eq!(function.execute(), Ok(Some(-5)));
    }

    #[test]
    fn lowers_unary_plus_as_integer_identity() {
        let module = lucid_syntax::parse("value = +5\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert_eq!(Function::from_expr(value).unwrap().execute(), Ok(Some(5)));
    }

    #[test]
    fn lowers_unary_inversion() {
        let module = lucid_syntax::parse("value = ~5\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert_eq!(Function::from_expr(value).unwrap().execute(), Ok(Some(-6)));
    }

    #[test]
    fn lowers_logical_negation_to_canonical_boolean() {
        for (source, expected) in [("value = not 0\n", 1), ("value = not 4\n", 0)] {
            let module = lucid_syntax::parse(source).unwrap();
            let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
                panic!("expected assignment");
            };
            assert_eq!(
                Function::from_expr(value).unwrap().execute(),
                Ok(Some(expected))
            );
        }
    }

    #[test]
    fn lowers_constant_conditional_without_evaluating_dead_branch() {
        let module = lucid_syntax::parse("value = 1 if true else (2 // 0)\n").unwrap();
        assert_eq!(
            Function::from_module(&module).unwrap().execute(),
            Ok(Some(1))
        );
    }

    #[test]
    fn lowers_dynamic_conditional_to_selective_cfg_execution() {
        for (source, expected) in [
            ("value = 1 if 1 else (2 // 0)\n", 1),
            ("value = 1 if 0 else 7\n", 7),
            ("value = 8 if 3 is 3 else 9\n", 8),
        ] {
            let module = lucid_syntax::parse(source).unwrap();
            let function = Function::from_module(&module).expect("conditional should lower");
            assert_eq!(function.execute(), Ok(Some(expected)));
        }
    }

    #[test]
    fn lowers_logical_and_or_to_canonical_boolean() {
        for (source, expected) in [
            ("value = 1 and 2\n", 1),
            ("value = 1 and 0\n", 0),
            ("value = 0 or 3\n", 1),
        ] {
            let module = lucid_syntax::parse(source).unwrap();
            let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
                panic!("expected assignment");
            };
            assert_eq!(
                Function::from_expr(value).unwrap().execute(),
                Ok(Some(expected))
            );
        }
    }

    #[test]
    fn lowers_identity_operators_for_primitive_values() {
        for (source, expected) in [("value = 3 is 3\n", 1), ("value = 3 is not 4\n", 1)] {
            let module = lucid_syntax::parse(source).unwrap();
            assert_eq!(
                Function::from_module(&module).unwrap().execute(),
                Ok(Some(expected))
            );
        }
    }

    #[test]
    fn logical_lowering_short_circuits_dead_right_operands() {
        for (source, expected) in [
            ("value = 0 and (2 // 0)\n", 0),
            ("value = 1 or (2 // 0)\n", 1),
            ("value = \"\" and (2 // 0)\n", 0),
            ("value = none and (2 // 0)\n", 0),
            ("value = not \"x\" and (2 // 0)\n", 0),
            ("value = 1 > 2 and (2 // 0)\n", 0),
            ("value = (false or false) and (2 // 0)\n", 0),
            ("value = \"\" == \"x\" and (2 // 0)\n", 0),
            ("value = \"x\" or (2 // 0)\n", 1),
            ("value = 9223372036854775808 and 2\n", 1),
        ] {
            let module = lucid_syntax::parse(source).unwrap();
            assert_eq!(
                Function::from_module(&module).unwrap().execute(),
                Ok(Some(expected))
            );
        }
    }

    #[test]
    fn nested_logical_constants_short_circuit_inside_arithmetic() {
        for (source, expected) in [
            ("value = 2 + (0 and (2 // 0))\n", 2),
            ("value = 2 + (1 or (2 // 0))\n", 3),
        ] {
            let module = lucid_syntax::parse(source).unwrap();
            assert_eq!(
                Function::from_module(&module).unwrap().execute(),
                Ok(Some(expected))
            );
        }
    }

    #[test]
    fn module_lowering_accepts_initialized_variable_definitions() {
        let module = lucid_syntax::parse("value: int = 6 * 7\n").unwrap();
        assert_eq!(
            Function::from_module(&module).unwrap().execute(),
            Ok(Some(42))
        );
    }

    #[test]
    fn module_lowering_unwraps_exported_initializers() {
        let module = lucid_syntax::parse("export value: int = 9\n").unwrap();
        assert_eq!(
            Function::from_module(&module).unwrap().execute(),
            Ok(Some(9))
        );
    }

    #[test]
    fn linear_module_lowering_resolves_bindings_and_returns_last_value() {
        let module = lucid_syntax::parse("x = 6\ny = x * 7\n").unwrap();
        let function = Function::from_module_linear(&module).unwrap();
        assert_eq!(function.execute(), Ok(Some(42)));
    }

    #[test]
    fn linear_module_lowering_accepts_value_return_statements() {
        let module = lucid_syntax::parse("x = 6\nreturn x * 7\n").unwrap();
        let function = Function::from_module_linear(&module).unwrap();
        assert_eq!(function.execute(), Ok(Some(42)));
    }

    #[test]
    fn linear_module_lowering_supports_augmented_bindings() {
        let module = lucid_syntax::parse("x = 5\nx += 3\n").unwrap();
        let function = Function::from_module_linear(&module).unwrap();
        assert_eq!(function.execute(), Ok(Some(8)));
        let module = lucid_syntax::parse("x = 5\nx **= 2\n").unwrap();
        let function = Function::from_module_linear(&module).unwrap();
        assert_eq!(function.execute(), Ok(Some(25)));
    }

    #[test]
    fn linear_module_lowering_selects_constant_if_branch() {
        let module =
            lucid_syntax::parse("if true:\n    x = 4\nelse:\n    x = 9\ny = x + 1\n").unwrap();
        let function = Function::from_module_linear(&module).unwrap();
        assert_eq!(function.execute(), Ok(Some(5)));
        let module = lucid_syntax::parse("if 0:\n    x = 4\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(9))
        );
        let module = lucid_syntax::parse("if not false:\n    x = 4\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module = lucid_syntax::parse("if not \"\":\n    x = 4\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module =
            lucid_syntax::parse("if true and false:\n    x = 4\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(9))
        );
        let module =
            lucid_syntax::parse("if false and unknown:\n    x = 4\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(9))
        );
        let module = lucid_syntax::parse("if \"\":\n    x = 4\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(9))
        );
        let module = lucid_syntax::parse("if 0.0:\n    x = 4\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(9))
        );
        let module = lucid_syntax::parse("if 3 === 3:\n    x = 4\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module = lucid_syntax::parse("if 3 !== 3:\n    x = 4\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(9))
        );
    }

    #[test]
    fn linear_module_lowering_rejects_dynamic_control_flow_instead_of_dropping_it() {
        let module = lucid_syntax::parse("if input:\n    x = 1\nelse:\n    x = 2\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module),
            Err(LowerError::UnsupportedExpression)
        );
        let module =
            lucid_syntax::parse("flag = true\nif flag:\n    x = 3\nelse:\n    x = 4\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module = lucid_syntax::parse(
            "flag = false\nif flag:\n    x = 3\nelif 1 < 2:\n    x = 4\nelse:\n    x = 5\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module = lucid_syntax::parse(
            "flag = false\nother = true\nif flag:\n    x = 3\nelif other:\n    x = 4\nelse:\n    x = 5\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module = lucid_syntax::parse(
            "flag = false\nother = true\nif flag:\n    x = 3\nelif not other:\n    x = 4\nelse:\n    x = 5\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(5))
        );
        let module = lucid_syntax::parse(
            "flag = false\nif flag:\n    x = 3\nelif not true:\n    x = 4\nelse:\n    x = 5\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(5))
        );
        let module =
            lucid_syntax::parse("flag = 1 < 2\nif flag:\n    x = 3\nelse:\n    x = 4\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module =
            lucid_syntax::parse("flag = true == false\nif flag:\n    x = 3\nelse:\n    x = 4\n")
                .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module =
            lucid_syntax::parse("flag = 1 + 1 < 3\nif flag:\n    x = 3\nelse:\n    x = 4\n")
                .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module =
            lucid_syntax::parse("flag = not (1 < 2)\nif flag:\n    x = 3\nelse:\n    x = 4\n")
                .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module =
            lucid_syntax::parse("flag = (1 << 2) < 5\nif flag:\n    x = 3\nelse:\n    x = 4\n")
                .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module =
            lucid_syntax::parse("flag = (4 // 2) == 2\nif flag:\n    x = 3\nelse:\n    x = 4\n")
                .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module =
            lucid_syntax::parse("flag = 1 + 1\nif flag:\n    x = 3\nelse:\n    x = 4\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module = lucid_syntax::parse(
            "flag = 1 < 2\nif flag:\n    y = 2\n    x = y + 1\nelse:\n    y = 8\n    x = y + 1\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module = lucid_syntax::parse(
            "left = 1 < 2\nright = 2 > 3\nif left and right:\n    x = 3\nelse:\n    x = 4\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module = lucid_syntax::parse(
            "flag = 1 < 2\nif not flag or false:\n    x = 3\nelse:\n    x = 4\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module =
            lucid_syntax::parse("keep_going = false\nwhile keep_going:\n    x = 1\nx = 6\n")
                .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(6))
        );
        let module = lucid_syntax::parse("checked = true\nassert(checked)\nx = 7\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(7))
        );
        let module = lucid_syntax::parse("if 2 < 3:\n    x = 5\nelse:\n    x = 6\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(5))
        );
        let module = lucid_syntax::parse(
            "if 2 > 3:\n    x = 5\nelif 4 == 4:\n    x = 7\nelse:\n    x = 6\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(7))
        );
        let module = lucid_syntax::parse(
            "if false:\n    x = 5\nelif 4 == 4:\n    x = 7\nelse:\n    x = 6\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(7))
        );
        let module =
            lucid_syntax::parse("if 0:\n    x = 5\nelif 4 == 4:\n    x = 7\nelse:\n    x = 6\n")
                .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(7))
        );
    }

    #[test]
    fn dynamic_if_lowering_builds_verified_phi_merge() {
        for (source, expected) in [
            ("if 1 < 2:\n    x = 7 + 3\nelse:\n    x = 9\n", 10),
            ("if false:\n    x = 7\nelse:\n    x = 9 * 2\n", 18),
        ] {
            let module = lucid_syntax::parse(source).unwrap();
            let function = Function::from_module_if(&module).unwrap();
            assert_eq!(function.execute(), Ok(Some(expected)));
        }
        let module = lucid_syntax::parse("if 1:\n    x: int = 2\nelse:\n    x: int = 3\n").unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(2))
        );
        let module = lucid_syntax::parse(
            "if 1 < 2:\n    y = 4\n    x = y + 2\nelse:\n    y = 8\n    x = y + 2\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(6))
        );
        let module =
            lucid_syntax::parse("if 1 < 2:\n    export x = 2\nelse:\n    export x = 3\n").unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(2))
        );
        let module = lucid_syntax::parse(
            "flag = true\nif flag:\n    x = 2\n    assert(true)\nelse:\n    x = 3\n    while false:\n        x = 9\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(2))
        );
        let module = lucid_syntax::parse(
            "flag = true\nif flag:\n    x = 2\n    assert(x)\nelse:\n    x = 3\n    assert(x)\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(2))
        );
        let module =
            lucid_syntax::parse("flag = false\nif flag:\n    x = 2\nelse:\n    y = 3\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module =
            lucid_syntax::parse("flag = true\nvalue = 1\nif flag:\n    value = 2\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(2))
        );
        let module =
            lucid_syntax::parse("flag = false\nvalue = 1\nif flag:\n    value = 2\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(1))
        );
        let module = lucid_syntax::parse("flag = true\nif flag:\n    value = 2\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(2))
        );
        let module = lucid_syntax::parse("flag = true\nvalue = 1\nif flag:\n    pass\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(1))
        );
        let module = lucid_syntax::parse(
            "flag = true\nvalue = 1\nif flag:\n    pass\nelse:\n    value = 2\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(1))
        );
        let module = lucid_syntax::parse(
            "flag = false\nvalue = 1\nif flag:\n    value = 2\nelse:\n    pass\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(1))
        );
        let module = lucid_syntax::parse(
            "flag = false\nif flag:\n    value = 1\nelif true:\n    value = 2\nelse:\n    value = 3\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(2))
        );
        let module = lucid_syntax::parse(
            "flag = false\nif flag:\n    value = 1\nelif false:\n    value = 2\nelse:\n    value = 3\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module = lucid_syntax::parse(
            "flag = false\nvalue = 4\nif flag:\n    value = 1\nelif false:\n    value = 2\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(4))
        );
        let module = lucid_syntax::parse(
            "flag = true\nif flag:\n    value = 1\nelif true:\n    value = 2\nelse:\n    value = 3\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(1))
        );
        let module = lucid_syntax::parse(
            "other = true\nif flag:\n    value = 1\nelif other:\n    value = 2\nelse:\n    value = 3\n",
        )
        .unwrap();
        let function = Function::from_module_linear_with_params(&module, &["flag".into()])
            .expect("binding-known elif should select the false-side branch");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(2)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(1)));
        let module = lucid_syntax::parse(
            "other = false\nif flag:\n    value = 1\nelif other:\n    value = 2\nelse:\n    value = 3\n",
        )
        .unwrap();
        let function = Function::from_module_linear_with_params(&module, &["flag".into()])
            .expect("binding-known false elif should select else branch");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(3)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(1)));
        let module = lucid_syntax::parse(
            "if flag:\n    value = 1\nelse:\n    value = 2\nvalue = value + 1\n",
        )
        .unwrap();
        let function = Function::from_module_linear_with_params(&module, &["flag".into()])
            .expect("post-diamond continuation should lower through CIR");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(3)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(2)));
        let module = lucid_syntax::parse(
            "seed = 10\nif flag:\n    value = seed + 1\nelse:\n    value = seed + 2\nvalue = value * 2\n",
        )
        .unwrap();
        let function = Function::from_module_linear_with_params(&module, &["flag".into()])
            .expect("post-diamond continuation should preserve prefix bindings");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(24)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(22)));
        let module =
            lucid_syntax::parse("if flag:\n    left = 1\nelse:\n    right = 2\nvalue = 3\n")
                .unwrap();
        let function = Function::from_module_linear_with_params(&module, &["flag".into()])
            .expect("post-diamond continuation should not require an unused phi");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(3)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(3)));
        let module =
            lucid_syntax::parse("if flag:\n    left = 1 / 0\nelse:\n    right = 2\nvalue = 3\n")
                .unwrap();
        let function = Function::from_module_linear_with_params(&module, &["flag".into()])
            .expect("unused branch locals should still execute conditionally");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(3)));
        assert_eq!(
            function.execute_with_args(&[1]),
            Err(ExecuteError::DivisionByZero)
        );
        let module = lucid_syntax::parse("if flag:\n    left = 1\nvalue = 3\n").unwrap();
        let function = Function::from_module_linear_with_params(&module, &["flag".into()])
            .expect("one-sided unused branch locals should allow a suffix result");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(3)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(3)));
        let module = lucid_syntax::parse("if flag:\n    left = 1 / 0\nvalue = 3\n").unwrap();
        let function = Function::from_module_linear_with_params(&module, &["flag".into()])
            .expect("one-sided branch errors should remain conditional before suffix");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(3)));
        assert_eq!(
            function.execute_with_args(&[1]),
            Err(ExecuteError::DivisionByZero)
        );
        let module =
            lucid_syntax::parse("value = 1\nif flag:\n    value = 2\nvalue = value + 1\n").unwrap();
        let function = Function::from_module_linear_with_params(&module, &["flag".into()])
            .expect("one-sided reassignment should merge with the incoming value");
        assert_eq!(function.execute_with_args(&[0]), Ok(Some(2)));
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(3)));
        let module = lucid_syntax::parse(
            "if first:\n    value = 10\nelif second:\n    value = 20\nelse:\n    value = 30\nvalue = value + 1\n",
        )
        .unwrap();
        let function =
            Function::from_module_linear_with_params(&module, &["first".into(), "second".into()])
                .expect("dynamic elif continuation should lower through CIR");
        assert_eq!(function.execute_with_args(&[1, 1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[0, 1]), Ok(Some(21)));
        assert_eq!(function.execute_with_args(&[0, 0]), Ok(Some(31)));
        let module = lucid_syntax::parse(
            "value = 30\nif first:\n    value = 10\nelif second:\n    value = 20\nvalue = value + 1\n",
        )
        .unwrap();
        let function =
            Function::from_module_linear_with_params(&module, &["first".into(), "second".into()])
                .expect("dynamic elif continuation should preserve initialized fallback");
        assert_eq!(function.execute_with_args(&[1, 1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[0, 1]), Ok(Some(21)));
        assert_eq!(function.execute_with_args(&[0, 0]), Ok(Some(31)));
        let module = lucid_syntax::parse(
            "if first:\n    value = 10\nelif false:\n    value = 99\nelif second:\n    value = 20\nelse:\n    value = 30\nvalue = value + 1\n",
        )
        .unwrap();
        let function =
            Function::from_module_linear_with_params(&module, &["first".into(), "second".into()])
                .expect("static-false elifs should not block dynamic elif continuations");
        assert_eq!(function.execute_with_args(&[1, 1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[0, 1]), Ok(Some(21)));
        assert_eq!(function.execute_with_args(&[0, 0]), Ok(Some(31)));
        let module = lucid_syntax::parse(
            "if first:\n    value = 10\nelif second:\n    value = 20\nelif third:\n    value = 30\nelse:\n    value = 40\nvalue = value + 1\n",
        )
        .unwrap();
        let function = Function::from_module_linear_with_params(
            &module,
            &["first".into(), "second".into(), "third".into()],
        )
        .expect("multiple dynamic elif continuations should lower through CIR");
        assert_eq!(function.execute_with_args(&[1, 1, 1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[0, 1, 1]), Ok(Some(21)));
        assert_eq!(function.execute_with_args(&[0, 0, 1]), Ok(Some(31)));
        assert_eq!(function.execute_with_args(&[0, 0, 0]), Ok(Some(41)));
        let module = lucid_syntax::parse(
            "if first:\n    left = 10\nelif second:\n    middle = 20\nelif third:\n    right = 30\nelse:\n    fallback = 40\nvalue = 99\n",
        )
        .unwrap();
        let function = Function::from_module_linear_with_params(
            &module,
            &["first".into(), "second".into(), "third".into()],
        )
        .expect("unused dynamic elif branch locals should not require a phi");
        assert_eq!(function.execute_with_args(&[1, 1, 1]), Ok(Some(99)));
        assert_eq!(function.execute_with_args(&[0, 1, 1]), Ok(Some(99)));
        assert_eq!(function.execute_with_args(&[0, 0, 1]), Ok(Some(99)));
        assert_eq!(function.execute_with_args(&[0, 0, 0]), Ok(Some(99)));
        let module = lucid_syntax::parse(
            "if first:\n    left = 1 / 0\nelif second:\n    middle = 20\nelse:\n    fallback = 40\nvalue = 99\n",
        )
        .unwrap();
        let function =
            Function::from_module_linear_with_params(&module, &["first".into(), "second".into()])
                .expect("unused dynamic elif branch errors should remain conditional");
        assert_eq!(function.execute_with_args(&[0, 1]), Ok(Some(99)));
        assert_eq!(
            function.execute_with_args(&[1, 1]),
            Err(ExecuteError::DivisionByZero)
        );
        let module = lucid_syntax::parse(
            "flag = true\nif flag:\n    x = 2\n    return\nelse:\n    y = 3\n    return\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(2))
        );
        let module = lucid_syntax::parse(
            "flag = false\nif flag:\n    x = 2\n    return\nelse:\n    y = 3\n    return\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module =
            lucid_syntax::parse("flag = true\nif flag:\n    return\nelse:\n    y = 3\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module),
            Err(LowerError::NoLowerableAssignment)
        );
        let module = lucid_syntax::parse(
            "flag = false\nchecked = true\nkeep_going = false\nif flag:\n    x = 2\n    assert(checked)\nelse:\n    x = 3\n    while keep_going:\n        x = 9\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(3))
        );
        let module = lucid_syntax::parse(
            "flag = true\nif flag:\n    x = 2\n    for item in []:\n        x = 9\nelse:\n    x = 3\n    pass\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(2))
        );
        let module =
            lucid_syntax::parse("if 1 < 0:\n    x = 1\nelif true:\n    x = 6\nelse:\n    x = 9\n")
                .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(6))
        );
        let module =
            lucid_syntax::parse("if 1 < 0:\n    x = 1\nelif 0:\n    x = 6\nelse:\n    x = 9\n")
                .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(9))
        );
        let module =
            lucid_syntax::parse("if 1 < 0:\n    x = 1\nelif 0.5:\n    x = 6\nelse:\n    x = 9\n")
                .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(6))
        );
        let module = lucid_syntax::parse(
            "if 1 < 0:\n    x = 1\nelif 0.5 < 1.0:\n    x = 6\nelse:\n    x = 9\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(6))
        );
        let module = lucid_syntax::parse("if 1 < 0:\n    x = 1\nelif false:\n    x = 6\nelif true:\n    x = 8\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(8))
        );
        let module = lucid_syntax::parse(
            "if 1 < 0:\n    x = 1\nelif false or true:\n    x = 8\nelse:\n    x = 9\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(8))
        );
        let module =
            lucid_syntax::parse("if 1 < 0:\n    x = 1\nelif 2 < 3:\n    x = 8\nelse:\n    x = 9\n")
                .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(8))
        );
        let module = lucid_syntax::parse(
            "if 1 < 0:\n    x = 1\nelif true == false:\n    x = 8\nelse:\n    x = 9\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(9))
        );
        let module = lucid_syntax::parse(
            "if 1 < 0:\n    x = 1\nelif true is true:\n    x = 8\nelse:\n    x = 9\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(8))
        );
        let module = lucid_syntax::parse(
            "if 1 < 0:\n    x = 1\nelif 1 is 1:\n    x = 8\nelse:\n    x = 9\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(8))
        );
        let module = lucid_syntax::parse("if 1 < 0:\n    x = 1\nelif 9223372036854775808 == 9223372036854775808:\n    x = 8\nelse:\n    x = 9\n").unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(8))
        );
        let module = lucid_syntax::parse(
            "if 1 < 0:\n    x = 1\nelif none is none:\n    x = 8\nelse:\n    x = 9\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(8))
        );
        let module = lucid_syntax::parse(
            "if 1 < 0:\n    x = 1\nelif not \"\":\n    x = 8\nelse:\n    x = 9\n",
        )
        .unwrap();
        assert_eq!(
            Function::from_module_if(&module).unwrap().execute(),
            Ok(Some(8))
        );
        let module = lucid_syntax::parse("if not (1 < 2):\n    x = 1\nelse:\n    x = 6\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(6))
        );
        let module =
            lucid_syntax::parse("if 1 < 2:\n    x = 1 if false else 6\nelse:\n    x = 0\n")
                .unwrap();
        assert_eq!(
            Function::from_module_linear(&module).unwrap().execute(),
            Ok(Some(6))
        );
    }

    #[test]
    fn rejects_unsupported_ast_forms_explicitly() {
        let module = lucid_syntax::parse("value = [1, 2]\n").unwrap();
        let lucid_syntax::Stmt::Assignment { value, .. } = &module.statements[0] else {
            panic!("expected assignment");
        };
        assert_eq!(
            Function::from_expr(value),
            Err(LowerError::UnsupportedExpression)
        );
        let module = lucid_syntax::parse("for item in [1, 2]:\n    return item\n").unwrap();
        assert_eq!(
            Function::from_module_linear(&module),
            Err(LowerError::UnsupportedExpression),
            "linear unrolling must not erase an early loop return"
        );
    }

    #[test]
    fn lowers_parameterized_literal_match_to_decision_chain() {
        let function = Function::from_parameterized_literal_match(
            1,
            0,
            &[
                (TypedLiteral::Int(1), TypedLiteral::Int(11)),
                (TypedLiteral::Int(2), TypedLiteral::Int(22)),
            ],
            TypedLiteral::Int(33),
        )
        .expect("literal match should lower");
        assert_eq!(function.execute_with_args(&[1]), Ok(Some(11)));
        assert_eq!(function.execute_with_args(&[2]), Ok(Some(22)));
        assert_eq!(function.execute_with_args(&[9]), Ok(Some(33)));
        assert!(function.verify().is_ok());
    }

    #[test]
    fn lowers_declaration_only_and_empty_modules_to_void_cir() {
        let empty = lucid_syntax::parse("").unwrap();
        let empty_function = Function::from_module(&empty).expect("empty module should lower");
        assert_eq!(empty_function.execute(), Ok(None));

        let declarations =
            lucid_syntax::parse("pass\n\nimport metrics\n\nclass Marker:\n    pass\n").unwrap();
        let function = Function::from_module(&declarations).expect("declarations should lower");
        assert_eq!(function.execute(), Ok(None));
        assert!(matches!(
            function.blocks[0].terminator,
            Terminator::Return(None)
        ));
    }

    #[test]
    fn constant_range_evaluator_enforces_direction_and_bound() {
        fn expression(source: &str) -> lucid_syntax::Expr {
            let module = lucid_syntax::parse(&format!("value = {source}\n")).unwrap();
            match module.statements.into_iter().next().unwrap() {
                lucid_syntax::Stmt::Assignment { value, .. } => value,
                _ => panic!("expected assignment"),
            }
        }
        let empty = expression("range(0, 1, -1)");
        assert_eq!(const_range_values(&empty), Some(Vec::new()));
        let descending = expression("range(3, 0, -1)");
        assert_eq!(const_range_values(&descending), Some(vec![3, 2, 1]));
        let arithmetic = expression("range(1 + 1, 2 * 3, 1 << 1)");
        assert_eq!(const_range_values(&arithmetic), Some(vec![2, 4]));
        let powered = expression("range(2 ** 2, 10, 2 ** 1)");
        assert_eq!(const_range_values(&powered), Some(vec![4, 6, 8]));
        let quotient = expression("range(-5 // 2, 2, 2)");
        assert_eq!(const_range_values(&quotient), Some(vec![-3, -1, 1]));
        let inverted = expression("range(~3, 2, 2)");
        assert_eq!(const_range_values(&inverted), Some(vec![-4, -2, 0]));
        let divide_by_zero = expression("range(0, 3, 1 // 0)");
        assert_eq!(const_range_values(&divide_by_zero), None);
        let oversized = expression("range(2048)");
        assert_eq!(const_range_values(&oversized), None);
    }
}
