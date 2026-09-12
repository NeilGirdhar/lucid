use lucid_syntax::ast::*;
use lucid_syntax::token::Span;
use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq)]
pub enum Type {
    Int,
    LiteralInt(i64),
    LiteralBool(bool),
    LiteralFloat(f64),
    LiteralStr(String),
    Float,
    Bool,
    Str,
    None,
    Never,
    Class {
        name: String,
        type_args: Vec<Type>,
        parent: Option<String>,
        traits: Vec<String>,
        interfaces: Vec<String>,
        fields: HashMap<String, Type>,
        is_sealed: bool,
    },
    Interface {
        name: String,
        type_args: Vec<Type>,
        methods: HashSet<String>,
    },
    Trait {
        name: String,
        type_args: Vec<Type>,
        methods: HashSet<String>,
    },
    Record {
        fields: Vec<(Option<String>, Type)>,
        is_open: bool,
    },
    Function {
        params: Vec<Type>,
        return_type: Box<Type>,
    },
    Future(Box<Type>),
    Union(Vec<Type>),
    Intersection(Vec<Type>),
    Negation(Box<Type>),
    Shape(Vec<Option<i64>>),
    View {
        mutability: MutabilityView,
        inner: Box<Type>,
    },
    TypeVar(String),
}

fn interface_extends(source: &str, target: &str, env: &TypeEnvironment) -> bool {
    env.interface_bases
        .get(source)
        .into_iter()
        .flatten()
        .any(|base| base == target || interface_extends(base, target, env))
}

fn trait_extends(source: &str, target: &str, env: &TypeEnvironment) -> bool {
    env.trait_bases
        .get(source)
        .into_iter()
        .flatten()
        .any(|base| base == target || trait_extends(base, target, env))
}

impl Type {
    /// Produce the canonical semantic form used by later HIR/CIR passes.
    /// Nested unions/intersections are flattened, duplicate members are
    /// removed, and commutative members receive a deterministic order.  The
    /// existing checker can keep constructing types incrementally; consumers
    /// call this boundary before interning or comparing types.
    pub fn canonical(&self) -> Self {
        match self {
            Type::Union(parts) => canonical_commutative(parts, true),
            Type::Intersection(parts) => canonical_commutative(parts, false),
            Type::View { mutability, inner } => Type::View {
                mutability: mutability.clone(),
                inner: Box::new(inner.canonical()),
            },
            Type::Function {
                params,
                return_type,
            } => Type::Function {
                params: params.iter().map(Type::canonical).collect(),
                return_type: Box::new(return_type.canonical()),
            },
            Type::Future(inner) => Type::Future(Box::new(inner.canonical())),
            Type::Negation(inner) => Type::Negation(Box::new(inner.canonical())),
            Type::Record { fields, is_open } => Type::Record {
                fields: fields
                    .iter()
                    .map(|(name, ty)| (name.clone(), ty.canonical()))
                    .collect(),
                is_open: *is_open,
            },
            Type::Class {
                name,
                type_args,
                parent,
                traits,
                interfaces,
                fields,
                is_sealed,
            } => {
                let mut traits = traits.clone();
                traits.sort();
                traits.dedup();
                let mut interfaces = interfaces.clone();
                interfaces.sort();
                interfaces.dedup();
                Type::Class {
                    name: name.clone(),
                    type_args: type_args.iter().map(Type::canonical).collect(),
                    parent: parent.clone(),
                    traits,
                    interfaces,
                    fields: fields
                        .iter()
                        .map(|(k, v)| (k.clone(), v.canonical()))
                        .collect(),
                    is_sealed: *is_sealed,
                }
            }
            Type::Interface {
                name,
                type_args,
                methods,
            } => Type::Interface {
                name: name.clone(),
                type_args: type_args.iter().map(Type::canonical).collect(),
                methods: methods.clone(),
            },
            Type::Trait {
                name,
                type_args,
                methods,
            } => Type::Trait {
                name: name.clone(),
                type_args: type_args.iter().map(Type::canonical).collect(),
                methods: methods.clone(),
            },
            other => other.clone(),
        }
    }

    /// Render a canonical type without relying on hash-map or hash-set
    /// iteration order. This is used in snapshots and typed-HIR payloads.
    pub fn canonical_string(&self) -> String {
        match self {
            Type::Int => "int".into(),
            Type::LiteralInt(value) => format!("LiteralInt({value})"),
            Type::LiteralBool(value) => format!("LiteralBool({value})"),
            Type::LiteralFloat(value) => format!("LiteralFloat({value})"),
            Type::LiteralStr(value) => format!("LiteralStr({value:?})"),
            Type::Float => "float".into(),
            Type::Bool => "bool".into(),
            Type::Str => "str".into(),
            Type::None => "none".into(),
            Type::Never => "never".into(),
            Type::TypeVar(name) => format!("var({name})"),
            Type::Class {
                name,
                type_args,
                parent,
                traits,
                interfaces,
                fields,
                is_sealed,
            } => {
                let mut field_names = fields.keys().cloned().collect::<Vec<_>>();
                field_names.sort();
                let fields = field_names
                    .into_iter()
                    .map(|name| format!("{name}:{}", fields[&name].canonical_string()))
                    .collect::<Vec<_>>()
                    .join(",");
                let mut traits = traits.to_vec();
                traits.sort();
                traits.dedup();
                let mut interfaces = interfaces.to_vec();
                interfaces.sort();
                interfaces.dedup();
                format!(
                    "class({name};args=[{}];parent={};traits={};interfaces={};fields=[{fields}];sealed={is_sealed})",
                    type_args
                        .iter()
                        .map(Type::canonical_string)
                        .collect::<Vec<_>>()
                        .join(","),
                    parent.as_deref().unwrap_or(""),
                    traits.join(","),
                    interfaces.join(",")
                )
            }
            Type::Interface {
                name,
                type_args,
                methods,
            } => {
                let mut methods = methods.iter().cloned().collect::<Vec<_>>();
                methods.sort();
                format!(
                    "interface({name};args=[{}];methods=[{}])",
                    type_args
                        .iter()
                        .map(Type::canonical_string)
                        .collect::<Vec<_>>()
                        .join(","),
                    methods.join(",")
                )
            }
            Type::Trait {
                name,
                type_args,
                methods,
            } => {
                let mut methods = methods.iter().cloned().collect::<Vec<_>>();
                methods.sort();
                format!(
                    "trait({name};args=[{}];methods=[{}])",
                    type_args
                        .iter()
                        .map(Type::canonical_string)
                        .collect::<Vec<_>>()
                        .join(","),
                    methods.join(",")
                )
            }
            Type::Record { fields, is_open } => format!(
                "record({}{})",
                fields
                    .iter()
                    .map(|(name, ty)| format!(
                        "{}:{}",
                        name.as_deref().unwrap_or(""),
                        ty.canonical_string()
                    ))
                    .collect::<Vec<_>>()
                    .join(","),
                if *is_open { ",..." } else { "" }
            ),
            Type::Function {
                params,
                return_type,
            } => format!(
                "fn({})->{}",
                params
                    .iter()
                    .map(Type::canonical_string)
                    .collect::<Vec<_>>()
                    .join(","),
                return_type.canonical_string()
            ),
            Type::Future(inner) => format!("future({})", inner.canonical_string()),
            Type::Union(parts) => format!(
                "union({})",
                parts
                    .iter()
                    .map(Type::canonical_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Type::Intersection(parts) => format!(
                "intersection({})",
                parts
                    .iter()
                    .map(Type::canonical_string)
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Type::Negation(inner) => format!("not({})", inner.canonical_string()),
            Type::Shape(dimensions) => format!(
                "shape({})",
                dimensions
                    .iter()
                    .map(|dimension| dimension.map_or("_".into(), |value| value.to_string()))
                    .collect::<Vec<_>>()
                    .join(",")
            ),
            Type::View { mutability, inner } => {
                format!("view({mutability:?};{})", inner.canonical_string())
            }
        }
    }

    pub fn make_union(types: Vec<Type>) -> Self {
        let mut flattened = Vec::new();
        for t in types {
            match t {
                Type::Union(sub_types) => flattened.extend(sub_types),
                Type::Never => {} // Never is union identity (Never | X = X)
                other => flattened.push(other),
            }
        }

        // Deduplicate
        let mut unique: Vec<Type> = Vec::new();
        for t in flattened {
            if !unique.contains(&t) {
                unique.push(t);
            }
        }

        if unique.is_empty() {
            Type::Never
        } else if unique.len() == 1 {
            // The length check guarantees a value; keep this semantic
            // constructor total even if its representation changes later.
            unique.into_iter().next().unwrap_or(Type::Never)
        } else {
            Type::Union(unique)
        }
    }

    pub fn is_subtype_of(&self, target: &Type, env: &TypeEnvironment) -> bool {
        if self == target {
            return true;
        }

        // Literal types retain exact identity for equality above, then widen
        // to their primitive for ordinary assignment, trait, and union
        // compatibility checks.
        match self {
            Type::LiteralInt(_) => return Type::Int.is_subtype_of(target, env),
            Type::LiteralBool(_) => return Type::Bool.is_subtype_of(target, env),
            Type::LiteralFloat(_) => return Type::Float.is_subtype_of(target, env),
            Type::LiteralStr(_) => return Type::Str.is_subtype_of(target, env),
            _ => {}
        }

        // `object` is Lucid's checked top type: every runtime value can be
        // viewed as object, while operations still require narrowing or an
        // explicit `trust` at the boundary.
        if matches!(target, Type::Class { name, .. } if name == "object") {
            return true;
        }

        // Never is bottom: subtype of everything
        if matches!(self, Type::Never) {
            return true;
        }
        if let Type::LiteralInt(_) = self {
            if matches!(target, Type::Int) {
                return true;
            }
        }
        if let Type::LiteralBool(_) = self {
            if matches!(target, Type::Bool) {
                return true;
            }
        }
        if let Type::LiteralFloat(_) = self {
            if matches!(target, Type::Float) {
                return true;
            }
        }
        if let Type::LiteralStr(_) = self {
            if matches!(target, Type::Str) {
                return true;
            }
        }

        // Any is top and bottom (gradual typing)
        if matches!(self, Type::TypeVar(s) if s == "Any")
            || matches!(target, Type::TypeVar(s) if s == "Any")
        {
            return true;
        }

        // Union subtype: A | B <: Target iff A <: Target and B <: Target
        if let Type::Union(variants) = self {
            return variants.iter().all(|v| v.is_subtype_of(target, env));
        }

        // Target is Union: Self <: A | B if Self <: A or Self <: B
        if let Type::Union(target_variants) = target {
            return target_variants.iter().any(|tv| self.is_subtype_of(tv, env));
        }

        // An intersection satisfies every constituent obligation. A value
        // accepted by an intersection target must satisfy each constituent.
        if let Type::Intersection(variants) = self {
            return variants.iter().all(|v| v.is_subtype_of(target, env));
        }
        if let Type::Intersection(target_variants) = target {
            return target_variants.iter().all(|tv| self.is_subtype_of(tv, env));
        }

        // Negation is useful for closed, concrete checks (for example
        // `not int`). Unknown/gradual types remain conservative.
        if let Type::Negation(inner) = target {
            return !self.is_subtype_of(inner, env)
                && !matches!(self, Type::TypeVar(name) if name == "Any");
        }

        // Anonymous records are structural: a value may carry additional
        // fields, but every field required by the target shape must exist at
        // the declared position/name and satisfy its type.
        if let (
            Type::Record { fields: source, .. },
            Type::Record {
                fields: target_fields,
                ..
            },
        ) = (self, target)
        {
            return target_fields
                .iter()
                .enumerate()
                .all(|(index, (name, expected))| {
                    let found = match name {
                        Some(name) => source
                            .iter()
                            .find(|(source_name, _)| source_name.as_deref() == Some(name)),
                        None => source.get(index),
                    };
                    found.is_some_and(|(_, actual)| actual.is_subtype_of(expected, env))
                });
        }
        if let (
            Type::Class {
                name: class_name,
                fields,
                ..
            },
            Type::Record {
                fields: target_fields,
                ..
            },
        ) = (self, target)
        {
            fn class_field_type(
                class_name: &str,
                own_fields: &HashMap<String, Type>,
                field_name: &str,
                env: &TypeEnvironment,
            ) -> Option<Type> {
                if let Some(field) = own_fields.get(field_name) {
                    return Some(field.clone());
                }
                let parent = env.classes.get(class_name).and_then(|class| match class {
                    Type::Class { parent, .. } => parent.as_deref(),
                    _ => None,
                })?;
                let parent_fields = env.classes.get(parent).and_then(|class| match class {
                    Type::Class { fields, .. } => Some(fields),
                    _ => None,
                })?;
                class_field_type(parent, parent_fields, field_name, env)
            }
            return target_fields.iter().all(|(name, expected)| {
                name.as_ref()
                    .and_then(|name| class_field_type(class_name, fields, name, env))
                    .is_some_and(|actual| actual.is_subtype_of(expected, env))
            });
        }

        if let (Type::Shape(left), Type::Shape(right)) = (self, target) {
            return left.len() == right.len()
                && left.iter().zip(right).all(|(a, b)| match (a, b) {
                    (Some(x), Some(y)) => x == y,
                    // An unknown source dimension cannot promise an exact
                    // target dimension. A wildcard target, however, accepts
                    // either a known or unknown source dimension.
                    (_, None) => true,
                    (None, Some(_)) => false,
                });
        }
        if matches!(self, Type::Shape(_))
            && matches!(target, Type::Trait { name, .. } if name == "Shape")
        {
            return true;
        }
        if matches!(self, Type::Shape(_)) {
            if let Type::View {
                mutability: MutabilityView::Immutable | MutabilityView::ReadOnly,
                inner,
            } = target
            {
                if let Type::Class {
                    name, type_args, ..
                } = inner.as_ref()
                {
                    let element_is_int = type_args.is_empty()
                        || matches!(type_args.as_slice(), [Type::Int])
                        || matches!(type_args.as_slice(), [Type::TypeVar(name)] if name == "Any");
                    if name == "list" && element_is_int {
                        return true;
                    }
                }
            }
        }
        if matches!(self, Type::Function { .. })
            && matches!(target, Type::Trait { name, .. } if name == "Callable")
        {
            return true;
        }

        if let (
            Type::Interface {
                name: source_name,
                type_args: source_args,
                ..
            },
            Type::Interface {
                name: target_name,
                type_args: target_args,
                ..
            },
        ) = (self, target)
        {
            if source_name == target_name {
                if source_args.is_empty() || target_args.is_empty() {
                    return true;
                }
                if source_args.len() != target_args.len() {
                    return false;
                }
                let variances = env.interface_variance.get(source_name);
                return source_args.iter().zip(target_args.iter()).enumerate().all(
                    |(index, (source, expected))| match variances.and_then(|items| items.get(index))
                    {
                        Some(Variance::Covariant) => source.is_subtype_of(expected, env),
                        Some(Variance::Contravariant) => expected.is_subtype_of(source, env),
                        _ => source == expected,
                    },
                );
            }
            if interface_extends(source_name, target_name, env) {
                return true;
            }
        }
        if let (
            Type::Trait {
                name: source_name,
                type_args: source_args,
                ..
            },
            Type::Trait {
                name: target_name,
                type_args: target_args,
                ..
            },
        ) = (self, target)
        {
            if source_name == target_name {
                if source_args.is_empty() || target_args.is_empty() {
                    return true;
                }
                if source_args.len() != target_args.len() {
                    return false;
                }
                let variances = env.trait_variance.get(source_name);
                return source_args.iter().zip(target_args.iter()).enumerate().all(
                    |(index, (source, expected))| match variances.and_then(|items| items.get(index))
                    {
                        Some(Variance::Covariant) => source.is_subtype_of(expected, env),
                        Some(Variance::Contravariant) => expected.is_subtype_of(source, env),
                        _ => source == expected,
                    },
                );
            }
            if trait_extends(source_name, target_name, env) {
                return true;
            }
        }

        // Mutability View Subtyping:
        // T  <: &T (mutable subtype of read-only view)
        // !T <: &T (immutable subtype of read-only view)
        match (self, target) {
            (
                Type::View {
                    mutability: MutabilityView::Immutable,
                    inner: i1,
                },
                Type::View {
                    mutability: MutabilityView::ReadOnly,
                    inner: i2,
                },
            ) => {
                return i1.is_subtype_of(i2, env);
            }
            (
                Type::Class { name: n1, .. },
                Type::View {
                    mutability: MutabilityView::ReadOnly,
                    inner: i2,
                },
            ) => {
                let bare_class = Type::Class {
                    name: n1.clone(),
                    type_args: Vec::new(),
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                };
                return bare_class.is_subtype_of(i2, env);
            }
            _ => {}
        }

        // Class inheritance and interface implementation subtyping
        if let Type::Class {
            name: c_name,
            parent,
            traits,
            interfaces,
            ..
        } = self
        {
            if let Type::Class {
                name: target_name,
                type_args: target_args,
                ..
            } = target
            {
                if c_name == target_name {
                    // Compare instantiated arguments according to the
                    // declaration-site variance. Unparameterized types keep
                    // nominal identity; parameterized types cannot collapse
                    // merely because their class names match.
                    let Type::Class {
                        type_args: source_args,
                        ..
                    } = self
                    else {
                        return false;
                    };
                    if source_args.is_empty() || target_args.is_empty() {
                        return true;
                    }
                    if source_args.len() != target_args.len() {
                        return false;
                    }
                    let variances = env.class_variance.get(c_name);
                    return source_args.iter().zip(target_args.iter()).enumerate().all(
                        |(index, (source, expected))| match variances
                            .and_then(|items| items.get(index))
                        {
                            Some(Variance::Covariant) => source.is_subtype_of(expected, env),
                            Some(Variance::Contravariant) => expected.is_subtype_of(source, env),
                            _ => source == expected,
                        },
                    );
                }
                if let Some(p) = parent {
                    if p == target_name {
                        return true;
                    }
                    if let Some(parent_type) = env.classes.get(p) {
                        if parent_type.is_subtype_of(target, env) {
                            return true;
                        }
                    }
                }
            }

            if let Type::Interface {
                name: target_iface, ..
            } = target
            {
                if interfaces.iter().any(|interface| {
                    interface == target_iface || interface_extends(interface, target_iface, env)
                }) {
                    return true;
                }
                for t in traits {
                    if trait_extends(t, target_iface, env) {
                        return true;
                    }
                }
            }

            if let Type::Trait {
                name: target_trait, ..
            } = target
            {
                if traits.iter().any(|trait_name| {
                    trait_name == target_trait || trait_extends(trait_name, target_trait, env)
                }) {
                    return true;
                }
                // Built-in containers satisfy the capability traits directly;
                // these relationships are semantic and do not require
                // synthetic user declarations in the environment.
                let builtin_traits: &[&str] = match c_name.as_str() {
                    "str" => &["Sized", "Container"],
                    "list" => &[
                        "Sized",
                        "Container",
                        "Collection",
                        "Sequence",
                        "Iterable",
                        "Reversible",
                    ],
                    "range" => &[
                        "Sized",
                        "Container",
                        "Collection",
                        "Sequence",
                        "Iterable",
                        "Reversible",
                    ],
                    "set" => &["Sized", "Container", "Collection", "Iterable", "Set"],
                    "dict" => &["Sized", "Container", "Collection", "Iterable"],
                    "Bytes" => &["Sized", "Container", "Buffer"],
                    "ByteArray" | "MemoryView" => &["Sized", "Container", "Buffer"],
                    _ => &[],
                };
                if builtin_traits.contains(&target_trait.as_str()) {
                    return true;
                }
            }
        }

        false
    }
}

fn substitute_type(ty: &Type, substitutions: &HashMap<String, Type>) -> Type {
    match ty {
        Type::TypeVar(name) => substitutions
            .get(name)
            .cloned()
            .unwrap_or_else(|| ty.clone()),
        Type::Class {
            name,
            type_args,
            parent,
            traits,
            interfaces,
            fields,
            is_sealed,
        } if type_args.is_empty() && substitutions.contains_key(name) => {
            substitutions[name].clone()
        }
        Type::Class {
            name,
            type_args,
            parent,
            traits,
            interfaces,
            fields,
            is_sealed,
        } => Type::Class {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|arg| substitute_type(arg, substitutions))
                .collect(),
            parent: parent.clone(),
            traits: traits.clone(),
            interfaces: interfaces.clone(),
            fields: fields
                .iter()
                .map(|(name, ty)| (name.clone(), substitute_type(ty, substitutions)))
                .collect(),
            is_sealed: *is_sealed,
        },
        Type::Interface {
            name,
            type_args,
            methods,
        } => Type::Interface {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|arg| substitute_type(arg, substitutions))
                .collect(),
            methods: methods.clone(),
        },
        Type::Trait {
            name,
            type_args,
            methods,
        } => Type::Trait {
            name: name.clone(),
            type_args: type_args
                .iter()
                .map(|arg| substitute_type(arg, substitutions))
                .collect(),
            methods: methods.clone(),
        },
        Type::Record { fields, is_open } => Type::Record {
            fields: fields
                .iter()
                .map(|(name, ty)| (name.clone(), substitute_type(ty, substitutions)))
                .collect(),
            is_open: *is_open,
        },
        Type::Function {
            params,
            return_type,
        } => Type::Function {
            params: params
                .iter()
                .map(|arg| substitute_type(arg, substitutions))
                .collect(),
            return_type: Box::new(substitute_type(return_type, substitutions)),
        },
        Type::Future(inner) => Type::Future(Box::new(substitute_type(inner, substitutions))),
        Type::Union(parts) => Type::make_union(
            parts
                .iter()
                .map(|part| substitute_type(part, substitutions))
                .collect(),
        ),
        Type::Intersection(parts) => Type::Intersection(
            parts
                .iter()
                .map(|part| substitute_type(part, substitutions))
                .collect(),
        ),
        Type::Negation(inner) => Type::Negation(Box::new(substitute_type(inner, substitutions))),
        Type::View { mutability, inner } => Type::View {
            mutability: mutability.clone(),
            inner: Box::new(substitute_type(inner, substitutions)),
        },
        Type::Int
        | Type::Float
        | Type::Bool
        | Type::Str
        | Type::None
        | Type::Never
        | Type::LiteralInt(_)
        | Type::LiteralBool(_)
        | Type::LiteralFloat(_)
        | Type::LiteralStr(_)
        | Type::Shape(_) => ty.clone(),
    }
}

fn infer_type_arguments(
    pattern: &Type,
    actual: &Type,
    generic_names: &HashSet<String>,
    substitutions: &mut HashMap<String, Type>,
) -> bool {
    match (pattern, actual) {
        (
            Type::Class {
                name, type_args, ..
            },
            actual,
        ) if type_args.is_empty() && generic_names.contains(name) => {
            if let Some(existing) = substitutions.get(name) {
                existing == actual
            } else {
                substitutions.insert(name.clone(), actual.clone());
                true
            }
        }
        (
            Type::Class {
                name: pattern_name,
                type_args: pattern_args,
                ..
            },
            Type::Class {
                name: actual_name,
                type_args: actual_args,
                ..
            },
        ) if pattern_name == actual_name => {
            pattern_args
                .iter()
                .zip(actual_args.iter())
                .all(|(pattern_arg, actual_arg)| {
                    infer_type_arguments(pattern_arg, actual_arg, generic_names, substitutions)
                })
        }
        (
            Type::View {
                inner: pattern_inner,
                ..
            },
            Type::View {
                inner: actual_inner,
                ..
            },
        ) => infer_type_arguments(pattern_inner, actual_inner, generic_names, substitutions),
        (
            Type::Function {
                params: pattern_params,
                return_type: pattern_return,
            },
            Type::Function {
                params: actual_params,
                return_type: actual_return,
            },
        ) => {
            pattern_params
                .iter()
                .zip(actual_params.iter())
                .all(|(pattern_param, actual_param)| {
                    infer_type_arguments(pattern_param, actual_param, generic_names, substitutions)
                })
                && infer_type_arguments(pattern_return, actual_return, generic_names, substitutions)
        }
        _ => true,
    }
}

fn canonical_commutative(parts: &[Type], union: bool) -> Type {
    let mut flat = Vec::new();
    for part in parts {
        let normalized = part.canonical();
        match (&normalized, union) {
            (Type::Union(inner), true) | (Type::Intersection(inner), false) => {
                flat.extend(inner.iter().cloned());
            }
            _ => flat.push(normalized),
        }
    }
    flat.sort_by_key(Type::canonical_string);
    flat.dedup();
    if union {
        Type::make_union(flat)
    } else if flat.is_empty() {
        Type::Never
    } else if flat.len() == 1 {
        flat.into_iter().next().unwrap_or(Type::Never)
    } else {
        Type::Intersection(flat)
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct TypeError {
    pub message: String,
    pub span: Span,
}

#[derive(Debug, Clone, Default)]
pub struct TypeEnvironment {
    pub classes: HashMap<String, Type>,
    pub interfaces: HashMap<String, Type>,
    pub traits: HashMap<String, Type>,
    pub type_aliases: HashMap<String, Type>,
    pub type_alias_params: HashMap<String, Vec<String>>,
    pub type_alias_bounds: HashMap<String, Vec<Option<Type>>>,
    pub functions: HashMap<String, Type>,
    pub function_type_params: HashMap<String, Vec<TypeParam>>,
    pub function_arity: HashMap<String, (usize, Option<usize>)>,
    pub function_param_names: HashMap<String, Vec<String>>,
    pub function_positional_only: HashMap<String, HashSet<String>>,
    pub function_keyword_only: HashMap<String, HashSet<String>>,
    pub function_required_params: HashMap<String, Vec<String>>,
    pub function_overloads: HashMap<String, Vec<Type>>,
    pub dispatch_functions: HashSet<String>,
    pub overloaded_functions: HashSet<String>,
    /// Top-level functions decorated with `contextmanager`; calls to these
    /// produce a managed resource even though their source return annotation
    /// describes the yielded value rather than the desugared wrapper.
    pub contextmanager_functions: HashSet<String>,
    pub variables: HashMap<String, (Type, MutabilityView)>,
    /// Local bindings known to hold an exact freshly constructed class value.
    /// This lets `is` reject dead subclass/trait tests without treating every
    /// class-typed value as exact.
    pub exact_variables: HashMap<String, String>,
    /// Names declared with `final`; a later assignment is a static error.
    pub final_variables: HashSet<String>,
    /// Final instance fields, keyed by declaring class name.
    pub final_fields: HashMap<String, HashSet<String>>,
    /// Final instance/class methods, keyed by declaring class name.
    pub final_methods: HashMap<String, HashSet<String>>,
    pub final_classes: HashSet<String>,
    pub class_members: HashMap<String, HashSet<String>>,
    pub class_methods: HashMap<(String, String), Type>,
    pub class_method_params: HashMap<(String, String), Vec<String>>,
    pub class_getters: HashMap<(String, String), Type>,
    /// Callable and property signatures supplied by reusable trait bodies.
    /// Classes that list a trait as a base resolve these through the same
    /// lookup path as inherited class members.
    pub trait_methods: HashMap<(String, String), Type>,
    pub trait_method_params: HashMap<(String, String), Vec<String>>,
    pub trait_getters: HashMap<(String, String), Type>,
    pub trait_fields: HashMap<(String, String), Type>,
    pub trait_bases: HashMap<String, Vec<String>>,
    pub interface_methods: HashMap<(String, String), Type>,
    pub interface_method_params: HashMap<(String, String), Vec<String>>,
    pub interface_getters: HashMap<(String, String), Type>,
    pub interface_fields: HashMap<(String, String), Type>,
    pub interface_bases: HashMap<String, Vec<String>>,
    pub class_vars: HashMap<String, HashMap<String, Type>>,
    pub class_field_order: HashMap<String, Vec<String>>,
    pub class_constructor_arity: HashMap<String, usize>,
    pub class_constructor_required: HashMap<String, usize>,
    pub class_parents: HashMap<String, String>,
    /// Definition-site variance for each class type parameter.
    pub class_variance: HashMap<String, Vec<Variance>>,
    pub class_type_params: HashMap<String, Vec<String>>,
    pub interface_variance: HashMap<String, Vec<Variance>>,
    pub trait_variance: HashMap<String, Vec<Variance>>,
    pub class_bounds: HashMap<String, Vec<Option<Type>>>,
    pub interface_bounds: HashMap<String, Vec<Option<Type>>>,
    pub trait_bounds: HashMap<String, Vec<Option<Type>>>,
    pub obligations: HashMap<String, HashSet<String>>,
    pub sealed_subclasses: HashMap<String, Vec<String>>,
    pub current_return_type: Option<Type>,
    pub current_class: Option<String>,
    /// Whether the current body is a class factory, the only context where
    /// the `construct(...)` expression is legal.
    pub current_factory: bool,
    /// Nesting depth of loop bodies while checking statements.  `break` and
    /// `continue` are only valid when this is non-zero.
    pub loop_depth: usize,
    /// Local names introduced by import statements, used to reject duplicate
    /// import bindings before project-level resolution runs.
    pub import_bindings: HashSet<String>,
}

#[derive(Clone)]
pub struct TypeChecker {
    pub env: TypeEnvironment,
}

impl Default for TypeChecker {
    fn default() -> Self {
        Self::new()
    }
}

impl TypeChecker {
    fn removed_builtin_message(name: &str) -> Option<&'static str> {
        match name {
            "tuple" => Some("tuple is not supported; use a class or !list instead"),
            "property" => Some("property is not supported; use getter/setter member syntax instead"),
            "staticmethod" => Some(
                "staticmethod is not supported; a function that needs no self or cls stays a function",
            ),
            "classmethod" => Some("classmethod is a declaration modifier, not a decorator or builtin"),
            "NotImplemented" => Some(
                "NotImplemented is not supported; multiple dispatch replaces reflected operator negotiation",
            ),
            "isinstance" => Some("isinstance() is not supported; use `value is Type` instead"),
            "issubclass" => Some("issubclass() is not supported; use declaration-kind `is` checks instead"),
            "frozenset" => Some("frozenset() is not supported; use an immutable set literal !{...} instead"),
            "eval" => Some("eval() is not supported; Lucid only runs code the checker can see"),
            "exec" => Some("exec() is not supported; Lucid only runs code the checker can see"),
            "__import__" => Some("__import__() is not supported; use static import declarations instead"),
            "vars" => Some("vars() is not supported; use fields(...) for documented reflection"),
            "dir" => Some("dir() is not supported; use fields(...) for documented reflection"),
            "next" => Some("next() is not a bare builtin; call cursor.next() on an iterator instead"),
            "ascii" => Some("ascii() is not a bare builtin; use string.ascii(...) instead"),
            "filter" => Some("filter() is not supported; use a comprehension with an if clause instead"),
            "globals" => Some("globals() is not supported; use locals() for the visible bindings"),
            "compile" => Some("compile() is not a bare builtin"),
            "delattr" => Some("delattr() is not supported; declared fields are fixed"),
            "open" => Some("open() is not a bare builtin; use Path.open(...) instead"),
            "bin" => Some("bin() is not a bare builtin; use str.bin(...) instead"),
            "oct" => Some("oct() is not a bare builtin; use str.oct(...) instead"),
            "hex" => Some("hex() is not a bare builtin; use str.hex(...) instead"),
            "chr" => Some("chr() is not a bare builtin; use string.chr(...) instead"),
            "ord" => Some("ord() is not a bare builtin; use string.ord(...) instead"),
            _ => None,
        }
    }

    fn check_call_argument_order(args: &[Arg]) -> Result<(), TypeError> {
        let mut keyword_section_started = false;
        for argument in args {
            if argument.name.is_some() || argument.is_dict_spread {
                keyword_section_started = true;
            } else if argument.is_gather_spread {
                if keyword_section_started {
                    return Err(TypeError {
                        message: "positional argument follows named argument".into(),
                        span: argument.span,
                    });
                }
                keyword_section_started = true;
            } else if keyword_section_started {
                return Err(TypeError {
                    message: "positional argument follows named argument".into(),
                    span: argument.span,
                });
            }
        }
        Ok(())
    }

    pub fn new() -> Self {
        let mut env = TypeEnvironment::default();
        // Register builtins
        env.classes.insert(
            "object".to_string(),
            Type::Class {
                name: "object".into(),
                type_args: Vec::new(),
                parent: None,
                traits: Vec::new(),
                interfaces: Vec::new(),
                fields: HashMap::new(),
                is_sealed: false,
            },
        );
        env.classes.insert("int".to_string(), Type::Int);
        env.classes.insert("float".to_string(), Type::Float);
        env.classes.insert("bool".to_string(), Type::Bool);
        env.classes.insert("str".to_string(), Type::Str);
        env.classes.insert(
            "complex".to_string(),
            Type::Class {
                name: "complex".into(),
                type_args: Vec::new(),
                parent: None,
                traits: Vec::new(),
                interfaces: Vec::new(),
                fields: HashMap::new(),
                is_sealed: true,
            },
        );
        env.classes.insert("none".to_string(), Type::None);
        env.traits.insert(
            "Callable".into(),
            Type::Trait {
                name: "Callable".into(),
                type_args: Vec::new(),
                methods: HashSet::new(),
            },
        );
        env.variables.insert(
            "Callable".into(),
            (
                Type::Trait {
                    name: "Callable".into(),
                    type_args: Vec::new(),
                    methods: HashSet::new(),
                },
                MutabilityView::ReadOnly,
            ),
        );
        for name in [
            "Eq",
            "Ord",
            "Hashable",
            "Sized",
            "Iterable",
            "Iterator",
            "Reversible",
            "Set",
            "Container",
            "Collection",
            "Sequence",
            "Buffer",
            "Shape",
            "Truthy",
        ] {
            let method_names: &[&str] = match name {
                "Eq" => &["__eq__"],
                "Ord" => &["__eq__", "__lt__", "__le__", "__gt__", "__ge__"],
                "Hashable" => &["__hash__"],
                "Sized" => &["__len__"],
                "Iterable" => &["__iter__"],
                "Iterator" => &["next"],
                "Reversible" => &["__reversed__"],
                "Set" => &[
                    "__iter__",
                    "__len__",
                    "__contains__",
                    "__and__",
                    "__or__",
                    "__sub__",
                    "__xor__",
                    "isdisjoint",
                ],
                "Container" => &["__contains__"],
                "Collection" => &["__iter__", "__len__", "__contains__"],
                "Sequence" => &["__iter__", "__len__", "__contains__", "__getitem__"],
                "Buffer" => &["__buffer__"],
                "Shape" => &[],
                "Truthy" => &["__bool__"],
                _ => &[],
            };
            let trait_type = Type::Trait {
                name: name.into(),
                type_args: Vec::new(),
                methods: method_names.iter().map(|method| (*method).into()).collect(),
            };
            env.traits.insert(name.into(), trait_type.clone());
            env.variables
                .insert(name.into(), (trait_type, MutabilityView::ReadOnly));
        }
        for name in ["Bytes", "ByteArray", "MemoryView"] {
            env.classes.insert(
                name.to_string(),
                Type::Class {
                    name: name.to_string(),
                    type_args: Vec::new(),
                    parent: None,
                    traits: Vec::new(),
                    interfaces: vec!["Buffer".into()],
                    fields: HashMap::new(),
                    is_sealed: true,
                },
            );
        }
        let any = Type::TypeVar("Any".into());
        let list_any = Type::Class {
            name: "list".into(),
            type_args: vec![any.clone()],
            parent: None,
            traits: Vec::new(),
            interfaces: Vec::new(),
            fields: HashMap::new(),
            is_sealed: false,
        };
        let dict_any = Type::Class {
            name: "dict".into(),
            type_args: vec![Type::Str, any.clone()],
            parent: None,
            traits: Vec::new(),
            interfaces: Vec::new(),
            fields: HashMap::new(),
            is_sealed: false,
        };
        // Built-in collection methods participate in the same closed-member
        // lookup as user-defined class methods.  Keeping these signatures in
        // the checker prevents expression statements such as `items.append`
        // from becoming an unchecked escape hatch.
        let list_methods = env.class_members.entry("list".into()).or_default();
        list_methods.extend(
            ["append", "extend", "pop", "clear", "insert", "remove"]
                .into_iter()
                .map(str::to_string),
        );
        env.class_methods.insert(
            ("list".into(), "append".into()),
            Type::Function {
                params: vec![any.clone()],
                return_type: Box::new(Type::None),
            },
        );
        env.class_methods.insert(
            ("list".into(), "extend".into()),
            Type::Function {
                params: vec![list_any.clone()],
                return_type: Box::new(Type::None),
            },
        );
        for method in ["pop", "clear", "insert", "remove"] {
            env.class_methods.insert(
                ("list".into(), method.into()),
                Type::Function {
                    params: vec![any.clone()],
                    return_type: Box::new(Type::TypeVar("Any".into())),
                },
            );
        }
        let dict_methods = env.class_members.entry("dict".into()).or_default();
        dict_methods.extend(
            ["get", "keys", "values", "items", "clear", "pop"]
                .into_iter()
                .map(str::to_string),
        );
        for (method, arity) in [
            ("get", 2usize),
            ("pop", 1),
            ("keys", 0),
            ("values", 0),
            ("items", 0),
            ("clear", 0),
        ] {
            env.class_methods.insert(
                ("dict".into(), method.into()),
                Type::Function {
                    params: vec![any.clone(); arity],
                    return_type: Box::new(Type::TypeVar("Any".into())),
                },
            );
        }
        let set_methods = env.class_members.entry("set".into()).or_default();
        set_methods.extend(
            ["add", "remove", "discard", "clear", "pop"]
                .into_iter()
                .map(str::to_string),
        );
        for method in ["add", "remove", "discard", "clear", "pop"] {
            env.class_methods.insert(
                ("set".into(), method.into()),
                Type::Function {
                    params: vec![any.clone()],
                    return_type: Box::new(Type::TypeVar("Any".into())),
                },
            );
        }
        let special_types = [
            (
                "Arguments",
                vec![("vpargs", list_any.clone()), ("kwargs", dict_any.clone())],
            ),
            (
                "Parameters",
                vec![
                    ("pargs", list_any.clone()),
                    ("vpargs", list_any.clone()),
                    ("kwargs", dict_any.clone()),
                ],
            ),
            (
                "SourceLocation",
                vec![
                    ("file", Type::Str),
                    ("line", Type::Int),
                    ("column", Type::Int),
                ],
            ),
            ("VarName", vec![("value", Type::Str)]),
            ("DottedPath", vec![("parts", list_any.clone())]),
            ("Module", vec![("name", Type::Str)]),
        ];
        for (name, fields) in special_types {
            env.classes.insert(
                name.into(),
                Type::Class {
                    name: name.into(),
                    type_args: Vec::new(),
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: fields
                        .into_iter()
                        .map(|(field, ty)| (field.to_string(), ty))
                        .collect(),
                    is_sealed: true,
                },
            );
        }

        env.variables.insert(
            "print".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::None),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "freeze".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::TypeVar("T".to_string())),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "Sentinel".to_string(),
            (
                Type::Function {
                    params: Vec::new(),
                    return_type: Box::new(Type::TypeVar("Sentinel".to_string())),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "Cell".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::TypeVar("Cell".to_string())),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "range".to_string(),
            (
                Type::Function {
                    params: vec![Type::Int],
                    return_type: Box::new(Type::Class {
                        name: "range".into(),
                        type_args: vec![],
                        parent: None,
                        traits: vec![],
                        interfaces: vec![],
                        fields: HashMap::new(),
                        is_sealed: false,
                    }),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "len".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::Int),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "min".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::TypeVar("T".to_string())),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "max".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::TypeVar("T".to_string())),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "sum".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::Int),
                },
                MutabilityView::ReadOnly,
            ),
        );
        // `help` is intentionally permissive: the interactive/documentation
        // service may be called with no subject or with any value.
        env.variables.insert(
            "help".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("Any".into())],
                    return_type: Box::new(Type::None),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "slice".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("Any".into())],
                    return_type: Box::new(Type::Class {
                        name: "slice".into(),
                        type_args: Vec::new(),
                        parent: None,
                        traits: Vec::new(),
                        interfaces: Vec::new(),
                        fields: HashMap::new(),
                        is_sealed: true,
                    }),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "read_file".to_string(),
            (
                Type::Function {
                    params: vec![Type::Str],
                    return_type: Box::new(Type::Str),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "write_file".to_string(),
            (
                Type::Function {
                    params: vec![Type::Str, Type::Str],
                    return_type: Box::new(Type::None),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "env_var".to_string(),
            (
                Type::Function {
                    params: vec![Type::Str],
                    return_type: Box::new(Type::Str),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "str".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::Str),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "int".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::Int),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "float".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::Float),
                },
                MutabilityView::ReadOnly,
            ),
        );
        for (name, result) in [("bytearray", "ByteArray"), ("memoryview", "MemoryView")] {
            env.variables.insert(
                name.to_string(),
                (
                    Type::Function {
                        params: vec![Type::TypeVar("T".into())],
                        return_type: Box::new(Type::Class {
                            name: result.into(),
                            type_args: Vec::new(),
                            parent: None,
                            traits: Vec::new(),
                            interfaces: vec!["Buffer".into()],
                            fields: HashMap::new(),
                            is_sealed: true,
                        }),
                    },
                    MutabilityView::ReadOnly,
                ),
            );
        }
        env.variables.insert(
            "bytes".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".into())],
                    return_type: Box::new(Type::Class {
                        name: "Bytes".into(),
                        type_args: Vec::new(),
                        parent: None,
                        traits: Vec::new(),
                        interfaces: vec!["Buffer".into(), "Sized".into(), "Container".into()],
                        fields: HashMap::new(),
                        is_sealed: true,
                    }),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "bool".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::Bool),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "list".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::Class {
                        name: "list".into(),
                        type_args: vec![Type::TypeVar("Any".to_string())],
                        parent: None,
                        traits: vec![],
                        interfaces: vec![],
                        fields: HashMap::new(),
                        is_sealed: false,
                    }),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "abs".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::TypeVar("T".to_string())),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "round".to_string(),
            (
                Type::Function {
                    params: vec![Type::TypeVar("T".to_string())],
                    return_type: Box::new(Type::TypeVar("T".to_string())),
                },
                MutabilityView::ReadOnly,
            ),
        );
        env.variables.insert(
            "time".to_string(),
            (
                Type::Function {
                    params: vec![],
                    return_type: Box::new(Type::Float),
                },
                MutabilityView::ReadOnly,
            ),
        );
        // The remaining standard-library surface is intentionally typed
        // through Any at this boundary.  Individual operations still perform
        // their runtime checks, while names remain visible to the checker as
        // ordinary first-class builtins instead of undefined variables.
        for name in [
            "dict",
            "set",
            "enumerate",
            "map",
            "reversed",
            "iter",
            "locals",
            "format",
            "hash",
            "repr",
            "getattr",
            "setattr",
            "hasattr",
            "zip",
            "any",
            "all",
            "sorted",
            "pow",
            "cos",
            "sin",
            "tan",
            "sqrt",
            "floor",
            "ceil",
            "monotonic",
        ] {
            env.variables.entry(name.to_string()).or_insert((
                Type::Function {
                    params: vec![Type::TypeVar("Any".into())],
                    return_type: Box::new(Type::TypeVar("Any".into())),
                },
                MutabilityView::ReadOnly,
            ));
        }

        // Give the remaining builtins explicit arity and argument contracts.
        // Their result types may stay dynamic, but malformed calls must not
        // cross the static boundary and fail only inside a backend.
        let any = Type::TypeVar("Any".into());
        let builtin_contracts = [
            (
                "dict",
                0,
                Some(1),
                vec![any.clone()],
                Type::Class {
                    name: "dict".into(),
                    type_args: vec![any.clone(), any.clone()],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                },
            ),
            (
                "set",
                0,
                Some(1),
                vec![any.clone()],
                Type::Class {
                    name: "set".into(),
                    type_args: vec![any.clone()],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                },
            ),
            (
                "enumerate",
                1,
                Some(2),
                vec![any.clone(), Type::Int],
                any.clone(),
            ),
            ("reversed", 1, Some(1), vec![any.clone()], any.clone()),
            ("iter", 1, Some(1), vec![any.clone()], any.clone()),
            ("locals", 0, Some(0), Vec::new(), any.clone()),
            (
                "format",
                1,
                Some(2),
                vec![any.clone(), Type::Str],
                Type::Str,
            ),
            ("hash", 1, Some(1), vec![any.clone()], Type::Int),
            ("repr", 1, Some(1), vec![any.clone()], Type::Str),
            (
                "fields",
                1,
                Some(1),
                vec![any.clone()],
                Type::Class {
                    name: "list".into(),
                    type_args: vec![Type::Str],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                },
            ),
            (
                "getattr",
                2,
                Some(3),
                vec![any.clone(), Type::Str, any.clone()],
                any.clone(),
            ),
            (
                "setattr",
                3,
                Some(3),
                vec![any.clone(), Type::Str, any.clone()],
                Type::None,
            ),
            (
                "hasattr",
                2,
                Some(2),
                vec![any.clone(), Type::Str],
                Type::Bool,
            ),
            ("zip", 1, None, vec![any.clone()], any.clone()),
            ("any", 1, Some(1), vec![any.clone()], Type::Bool),
            ("all", 1, Some(1), vec![any.clone()], Type::Bool),
            ("abs", 1, Some(1), vec![any.clone()], any.clone()),
            (
                "round",
                1,
                Some(2),
                vec![any.clone(), Type::Int],
                any.clone(),
            ),
            (
                "sum",
                1,
                Some(2),
                vec![any.clone(), any.clone()],
                any.clone(),
            ),
            (
                "sorted",
                1,
                Some(1),
                vec![any.clone()],
                Type::Class {
                    name: "list".into(),
                    type_args: vec![any.clone()],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                },
            ),
            ("min", 1, None, vec![any.clone()], any.clone()),
            ("max", 1, None, vec![any.clone()], any.clone()),
            (
                "map",
                2,
                Some(2),
                vec![any.clone(), any.clone()],
                any.clone(),
            ),
            (
                "pow",
                2,
                Some(3),
                vec![any.clone(), any.clone(), any.clone()],
                any.clone(),
            ),
        ];
        for (name, required, maximum, params, return_type) in builtin_contracts {
            env.function_arity.insert(name.into(), (required, maximum));
            env.variables.insert(
                name.into(),
                (
                    Type::Function {
                        params,
                        return_type: Box::new(return_type),
                    },
                    MutabilityView::ReadOnly,
                ),
            );
        }
        for (name, required, maximum) in [
            ("freeze", 1, Some(1)),
            ("print", 0, None),
            ("help", 0, Some(1)),
            ("Sentinel", 0, Some(0)),
            ("Cell", 1, Some(1)),
            ("range", 1, Some(3)),
            ("slice", 1, Some(3)),
            ("len", 1, Some(1)),
            ("read_file", 1, Some(1)),
            ("write_file", 2, Some(2)),
            ("env_var", 1, Some(2)),
            ("str", 1, Some(1)),
            ("int", 1, Some(1)),
            ("float", 1, Some(1)),
            ("complex", 0, Some(2)),
            ("bool", 1, Some(1)),
            ("bytes", 1, Some(1)),
            ("bytearray", 1, Some(1)),
            ("memoryview", 1, Some(1)),
            ("list", 0, Some(1)),
            ("set", 0, Some(1)),
            ("time", 0, Some(0)),
        ] {
            env.function_arity.insert(name.into(), (required, maximum));
        }

        Self { env }
    }

    pub fn check_module(&mut self, module: &Module) -> Result<(), TypeError> {
        // Record modifiers before resolving bases so inheritance constraints do
        // not depend on source declaration order.
        for stmt in &module.statements {
            self.collect_class_modifiers(stmt);
            self.collect_class_members(stmt);
        }
        // Pass 1: Register class, interface, trait, and alias declarations
        for stmt in &module.statements {
            self.collect_declaration(stmt)?;
        }

        // Resolve class parents after every declaration is known. This makes
        // forward references and trait-first base lists behave like ordinary
        // class inheritance, and lets us reject cycles before member lookup or
        // exhaustiveness analysis can recurse through them.
        self.resolve_class_parents_and_check_cycles(module)?;

        // Pass 2: Verify single-inheritance and trait constraints
        for stmt in &module.statements {
            self.verify_structure(stmt)?;
        }

        // Pass 3: Check function bodies and statements
        for stmt in &module.statements {
            self.check_statement(stmt)?;
        }

        Ok(())
    }

    fn resolve_class_parents_and_check_cycles(&mut self, module: &Module) -> Result<(), TypeError> {
        let mut parents = HashMap::new();
        let mut spans = HashMap::new();
        fn collect(
            stmt: &Stmt,
            parents: &mut HashMap<String, String>,
            spans: &mut HashMap<String, Span>,
            classes: &HashMap<String, Type>,
        ) {
            match stmt {
                Stmt::Export(inner) => collect(inner, parents, spans, classes),
                Stmt::ClassDef {
                    name, bases, span, ..
                } => {
                    spans.insert(name.clone(), *span);
                    if let Some(parent) = bases.iter().find_map(|base| match base {
                        TypeExpr::Named {
                            name: base_name, ..
                        } if classes.contains_key(base_name) => Some(base_name.clone()),
                        _ => None,
                    }) {
                        parents.insert(name.clone(), parent);
                    }
                }
                _ => {}
            }
        }
        for stmt in &module.statements {
            collect(stmt, &mut parents, &mut spans, &self.env.classes);
        }
        self.env.class_parents = parents.clone();
        for (class, parent) in &parents {
            if let Some(Type::Class {
                parent: stored_parent,
                ..
            }) = self.env.classes.get_mut(class)
            {
                *stored_parent = Some(parent.clone());
            }
        }

        fn visit(
            node: &str,
            parents: &HashMap<String, String>,
            visiting: &mut HashSet<String>,
            visited: &mut HashSet<String>,
        ) -> bool {
            if visited.contains(node) {
                return false;
            }
            if !visiting.insert(node.to_owned()) {
                return true;
            }
            if let Some(parent) = parents.get(node) {
                if visit(parent, parents, visiting, visited) {
                    return true;
                }
            }
            visiting.remove(node);
            visited.insert(node.to_owned());
            false
        }

        let mut visiting = HashSet::new();
        let mut visited = HashSet::new();
        for class in parents.keys() {
            if visit(class, &parents, &mut visiting, &mut visited) {
                let span = spans.get(class).copied().unwrap_or_default();
                return Err(TypeError {
                    message: format!("cyclic class inheritance involving '{class}'"),
                    span,
                });
            }
        }
        Ok(())
    }

    fn collect_class_modifiers(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Export(inner) => self.collect_class_modifiers(inner),
            Stmt::ClassDef {
                name,
                is_final: true,
                ..
            } => {
                self.env.final_classes.insert(name.clone());
            }
            _ => {}
        }
    }

    fn collect_class_members(&mut self, stmt: &Stmt) {
        match stmt {
            Stmt::Export(inner) => self.collect_class_members(inner),
            Stmt::ClassDef { name, body, .. } => {
                let names = self.env.class_members.entry(name.clone()).or_default();
                for member in body {
                    match member {
                        ClassMember::Method(m) | ClassMember::ClassMethod(m) => {
                            names.insert(m.name.clone());
                        }
                        ClassMember::Factory(f) => {
                            names.insert(f.name.clone());
                        }
                        ClassMember::Getter(g) => {
                            names.insert(g.name.clone());
                        }
                        ClassMember::Setter(s) => {
                            names.insert(s.name.clone());
                        }
                        ClassMember::Field(f) | ClassMember::ClassVar(f) => {
                            names.insert(f.name.clone());
                        }
                        _ => {}
                    }
                }
            }
            Stmt::TraitDef { name, body, .. } => {
                let names = self.env.class_members.entry(name.clone()).or_default();
                for member in body {
                    match member {
                        TraitMember::Method(m) | TraitMember::ClassMethod(m) => {
                            names.insert(m.name.clone());
                        }
                        TraitMember::Getter(g) => {
                            names.insert(g.name.clone());
                        }
                        TraitMember::Setter(s) => {
                            names.insert(s.name.clone());
                        }
                        TraitMember::Field(f) => {
                            names.insert(f.name.clone());
                        }
                        TraitMember::Pass(_) | TraitMember::Ellipsis(_) => {}
                    }
                }
            }
            _ => {}
        }
    }

    fn collect_declaration(&mut self, stmt: &Stmt) -> Result<(), TypeError> {
        match stmt {
            Stmt::Export(inner) => self.collect_declaration(inner),
            Stmt::ClassDef {
                name,
                type_params,
                bases,
                without_traits,
                body,
                is_sealed,
                is_final,
                span,
                ..
            } => {
                let mut parent_class = None;
                let mut traits = Vec::new();
                let mut interfaces = Vec::new();

                for base_expr in bases {
                    if let TypeExpr::Named {
                        name: base_name, ..
                    } = base_expr
                    {
                        if self.env.classes.contains_key(base_name) {
                            if let Some(existing_parent) = parent_class.as_ref() {
                                return Err(TypeError {
                                    message: format!(
                                        "class '{name}' has multiple class parents ('{existing_parent}' and '{base_name}'). Lucid permits at most one class parent."
                                    ),
                                    span: *span,
                                });
                            }
                            parent_class = Some(base_name.clone());
                        } else if self.env.interfaces.contains_key(base_name) {
                            interfaces.push(base_name.clone());
                        } else if self.env.traits.contains_key(base_name) {
                            traits.push(base_name.clone());
                        } else {
                            // Forward-referenced or external
                            interfaces.push(base_name.clone());
                        }
                    }
                }

                // Every class gets structural equality, ordering, and
                // hashing by default.  `without` is the explicit opt-out
                // required by the language specification.
                for generated in ["Eq", "Ord", "Hashable"] {
                    if !without_traits.iter().any(|name| name == generated) {
                        traits.push(generated.to_string());
                    }
                }

                if let Some(ref p) = parent_class {
                    self.env
                        .sealed_subclasses
                        .entry(p.clone())
                        .or_default()
                        .push(name.clone());
                }

                let mut fields = HashMap::new();
                let mut final_fields = HashSet::new();
                let mut final_methods = HashSet::new();
                let mut member_names = HashSet::new();
                let mut class_vars = HashMap::new();
                let mut field_order = Vec::new();
                for member in body {
                    if let Some((member_name, member_span)) = Self::member_name_and_span(member) {
                        Self::reject_removed_member(member_name, member_span)?;
                    }
                    match member {
                        ClassMember::Field(f) => {
                            field_order.push(f.name.clone());
                            member_names.insert(f.name.clone());
                            if let Ok(ft) = self.resolve_type_expr(&f.type_annotation) {
                                fields.insert(f.name.clone(), ft);
                            }
                            if f.is_final {
                                final_fields.insert(f.name.clone());
                            }
                        }
                        ClassMember::ClassVar(f) => {
                            member_names.insert(f.name.clone());
                            if let Ok(ft) = self.resolve_type_expr(&f.type_annotation) {
                                class_vars.insert(f.name.clone(), ft);
                            }
                        }
                        ClassMember::Method(m) | ClassMember::ClassMethod(m) => {
                            member_names.insert(m.name.clone());
                            if m.is_final {
                                final_methods.insert(m.name.clone());
                            }
                        }
                        ClassMember::Factory(f) => {
                            member_names.insert(f.name.clone());
                        }
                        ClassMember::Getter(g) => {
                            member_names.insert(g.name.clone());
                        }
                        ClassMember::Setter(s) => {
                            member_names.insert(s.name.clone());
                        }
                        _ => {}
                    }
                }
                self.env.final_fields.insert(name.clone(), final_fields);
                self.env.final_methods.insert(name.clone(), final_methods);
                self.env.class_members.insert(name.clone(), member_names);
                self.env.class_vars.insert(name.clone(), class_vars);
                self.env.class_field_order.insert(name.clone(), field_order);
                self.env.class_constructor_arity.insert(
                    name.clone(),
                    body.iter()
                        .filter(|member| matches!(member, ClassMember::Field(_)))
                        .count(),
                );
                self.env.class_constructor_required.insert(
                    name.clone(),
                    body.iter()
                        .filter(|member| {
                            matches!(member, ClassMember::Field(field) if field.default.is_none())
                        })
                        .count(),
                );
                for member in body {
                    if let ClassMember::Method(method) | ClassMember::ClassMethod(method) = member
                    {
                        Self::reject_removed_decorators(method)?;
                    }
                    let (method_name, params, return_type, is_async) = match member {
                        ClassMember::Method(method) | ClassMember::ClassMethod(method) => (
                            &method.name,
                            &method.params,
                            method.return_type.as_ref(),
                            method.is_async,
                        ),
                        ClassMember::Factory(factory) => (
                            &factory.name,
                            &factory.params,
                            factory.return_type.as_ref(),
                            false,
                        ),
                        ClassMember::Getter(getter) => {
                            let return_type = getter
                                .return_type
                                .as_ref()
                                .map(|annotation| self.resolve_type_expr(annotation))
                                .transpose()?
                                .unwrap_or(Type::None);
                            self.env
                                .class_getters
                                .insert((name.clone(), getter.name.clone()), return_type);
                            continue;
                        }
                        ClassMember::Setter(setter) => {
                            let parameter_type = setter
                                .param
                                .type_annotation
                                .as_ref()
                                .map(|annotation| self.resolve_type_expr(annotation))
                                .transpose()?
                                .unwrap_or(Type::TypeVar("Any".into()));
                            self.env.class_methods.insert(
                                (name.clone(), setter.name.clone()),
                                Type::Function {
                                    params: vec![parameter_type],
                                    return_type: Box::new(Type::None),
                                },
                            );
                            continue;
                        }
                        _ => continue,
                    };
                    let parameter_types = params
                        .iter()
                        .filter(|param| !matches!(param.name.as_str(), "self" | "cls"))
                        .map(|param| {
                            param
                                .type_annotation
                                .as_ref()
                                .map(|annotation| self.resolve_type_expr(annotation))
                                .transpose()
                                .map(|ty| ty.unwrap_or(Type::TypeVar("Any".into())))
                        })
                        .collect::<Result<Vec<_>, _>>()?;
                    let return_type = return_type
                        .map(|annotation| self.resolve_type_expr(annotation))
                        .transpose()?
                        .unwrap_or(Type::None);
                    let return_type = if is_async {
                        Type::Future(Box::new(return_type))
                    } else {
                        return_type
                    };
                    self.env.class_methods.insert(
                        (name.clone(), method_name.clone()),
                        Type::Function {
                            params: parameter_types,
                            return_type: Box::new(return_type),
                        },
                    );
                    self.env.class_method_params.insert(
                        (name.clone(), method_name.clone()),
                        params.iter().map(|param| param.name.clone()).collect(),
                    );
                }
                self.env.class_variance.insert(
                    name.clone(),
                    type_params
                        .iter()
                        .map(|param| param.variance.clone())
                        .collect(),
                );
                self.env.class_type_params.insert(
                    name.clone(),
                    type_params.iter().map(|param| param.name.clone()).collect(),
                );
                self.env.class_bounds.insert(
                    name.clone(),
                    type_params
                        .iter()
                        .map(|param| {
                            param
                                .bound
                                .as_ref()
                                .and_then(|bound| self.resolve_type_expr(bound).ok())
                        })
                        .collect(),
                );
                if *is_final {
                    self.env.final_classes.insert(name.clone());
                }

                let class_type = Type::Class {
                    name: name.clone(),
                    type_args: Vec::new(),
                    parent: parent_class,
                    traits,
                    interfaces,
                    fields,
                    is_sealed: *is_sealed,
                };

                self.env.classes.insert(name.clone(), class_type);
                Ok(())
            }
            Stmt::InterfaceDef {
                name,
                type_params,
                bases,
                body,
                ..
            } => {
                let mut required = HashSet::new();
                self.env.interface_bases.insert(
                    name.clone(),
                    bases
                        .iter()
                        .filter_map(|base| match base {
                            TypeExpr::Named { name, .. } => Some(name.clone()),
                            _ => None,
                        })
                        .collect(),
                );
                for member in body {
                    if let Some((member_name, member_span)) =
                        Self::interface_member_name_and_span(member)
                    {
                        Self::reject_removed_member(member_name, member_span)?;
                    }
                    match member {
                        InterfaceMember::MethodSig {
                            name: member_name,
                            params,
                            return_type,
                            ..
                        }
                        | InterfaceMember::ClassMethodSig {
                            name: member_name,
                            params,
                            return_type,
                            ..
                        }
                        | InterfaceMember::FactorySig {
                            name: member_name,
                            params,
                            return_type,
                            ..
                        } => {
                            required.insert(member_name.clone());
                            let parameter_types = params
                                .iter()
                                .filter(|param| !matches!(param.name.as_str(), "self" | "cls"))
                                .map(|param| {
                                    param
                                        .type_annotation
                                        .as_ref()
                                        .map(|annotation| self.resolve_type_expr(annotation))
                                        .transpose()
                                        .map(|ty| ty.unwrap_or(Type::TypeVar("Any".into())))
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            let return_type = return_type
                                .as_ref()
                                .map(|annotation| self.resolve_type_expr(annotation))
                                .transpose()?
                                .unwrap_or(Type::None);
                            self.env.interface_methods.insert(
                                (name.clone(), member_name.clone()),
                                Type::Function {
                                    params: parameter_types,
                                    return_type: Box::new(return_type),
                                },
                            );
                            self.env.interface_method_params.insert(
                                (name.clone(), member_name.clone()),
                                params.iter().map(|param| param.name.clone()).collect(),
                            );
                        }
                        InterfaceMember::GetterSig {
                            name: member_name,
                            return_type,
                            ..
                        } => {
                            required.insert(member_name.clone());
                            let return_type = return_type
                                .as_ref()
                                .map(|annotation| self.resolve_type_expr(annotation))
                                .transpose()?
                                .unwrap_or(Type::None);
                            self.env
                                .interface_getters
                                .insert((name.clone(), member_name.clone()), return_type);
                        }
                        InterfaceMember::SetterSig {
                            name: member_name,
                            param_type,
                            ..
                        } => {
                            required.insert(member_name.clone());
                            let parameter_type = self.resolve_type_expr(param_type)?;
                            self.env.interface_methods.insert(
                                (name.clone(), member_name.clone()),
                                Type::Function {
                                    params: vec![parameter_type],
                                    return_type: Box::new(Type::None),
                                },
                            );
                        }
                        InterfaceMember::FieldSig {
                            name: member_name,
                            type_annotation,
                            is_final,
                            span,
                        } => {
                            if *is_final {
                                return Err(TypeError {
                                    message: format!(
                                        "final member '{member_name}' must have an implementation"
                                    ),
                                    span: *span,
                                });
                            }
                            required.insert(member_name.clone());
                            let field_type = self.resolve_type_expr(type_annotation)?;
                            self.env
                                .interface_fields
                                .insert((name.clone(), member_name.clone()), field_type);
                        }
                        InterfaceMember::AssociatedTypeSig { name, .. } => {
                            required.insert(name.clone());
                        }
                        InterfaceMember::Pass(_) | InterfaceMember::Ellipsis(_) => {}
                    }
                }
                self.env.obligations.insert(name.clone(), required);
                self.env.interface_variance.insert(
                    name.clone(),
                    type_params
                        .iter()
                        .map(|param| param.variance.clone())
                        .collect(),
                );
                self.env.interface_bounds.insert(
                    name.clone(),
                    type_params
                        .iter()
                        .map(|param| {
                            param
                                .bound
                                .as_ref()
                                .and_then(|bound| self.resolve_type_expr(bound).ok())
                        })
                        .collect(),
                );
                let iface_type = Type::Interface {
                    name: name.clone(),
                    type_args: Vec::new(),
                    methods: HashSet::new(),
                };
                self.env.interfaces.insert(name.clone(), iface_type);
                Ok(())
            }
            Stmt::TraitDef {
                name,
                type_params,
                bases,
                body,
                ..
            } => {
                let mut required = HashSet::new();
                let base_names = bases
                    .iter()
                    .filter_map(|base| match base {
                        TypeExpr::Named { name, .. } => Some(name.clone()),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                self.env
                    .trait_bases
                    .insert(name.clone(), base_names.clone());
                for base_name in &base_names {
                    if let Some(base_obligations) = self.env.obligations.get(base_name) {
                        required.extend(base_obligations.iter().cloned());
                    }
                }
                for member in body {
                    if let Some((member_name, member_span)) =
                        Self::trait_member_name_and_span(member)
                    {
                        Self::reject_removed_member(member_name, member_span)?;
                    }
                    match member {
                        TraitMember::Field(field) if field.is_final => {
                            return Err(TypeError {
                                message: format!(
                                    "final member '{}' must have an implementation",
                                    field.name
                                ),
                                span: field.span,
                            });
                        }
                        TraitMember::Method(method) | TraitMember::ClassMethod(method)
                            if method.body.is_empty() =>
                        {
                            required.insert(method.name.clone());
                        }
                        TraitMember::Getter(getter) if getter.body.is_empty() => {
                            required.insert(getter.name.clone());
                        }
                        TraitMember::Setter(setter) if setter.body.is_empty() => {
                            required.insert(setter.name.clone());
                        }
                        TraitMember::Field(field) if field.default.is_none() => {
                            required.insert(field.name.clone());
                        }
                        _ => {}
                    }
                }
                self.env.obligations.insert(name.clone(), required);
                for member in body {
                    if let TraitMember::Method(method) | TraitMember::ClassMethod(method) = member
                    {
                        Self::reject_removed_decorators(method)?;
                    }
                    match member {
                        TraitMember::Method(method) | TraitMember::ClassMethod(method) => {
                            let parameter_types = method
                                .params
                                .iter()
                                .filter(|param| !matches!(param.name.as_str(), "self" | "cls"))
                                .map(|param| {
                                    param
                                        .type_annotation
                                        .as_ref()
                                        .map(|annotation| self.resolve_type_expr(annotation))
                                        .transpose()
                                        .map(|ty| ty.unwrap_or(Type::TypeVar("Any".into())))
                                })
                                .collect::<Result<Vec<_>, _>>()?;
                            let return_type = method
                                .return_type
                                .as_ref()
                                .map(|annotation| self.resolve_type_expr(annotation))
                                .transpose()?
                                .unwrap_or(Type::None);
                            let return_type = if method.is_async {
                                Type::Future(Box::new(return_type))
                            } else {
                                return_type
                            };
                            self.env.trait_methods.insert(
                                (name.clone(), method.name.clone()),
                                Type::Function {
                                    params: parameter_types,
                                    return_type: Box::new(return_type),
                                },
                            );
                            self.env.trait_method_params.insert(
                                (name.clone(), method.name.clone()),
                                method
                                    .params
                                    .iter()
                                    .map(|param| param.name.clone())
                                    .collect(),
                            );
                        }
                        TraitMember::Getter(getter) => {
                            let return_type = getter
                                .return_type
                                .as_ref()
                                .map(|annotation| self.resolve_type_expr(annotation))
                                .transpose()?
                                .unwrap_or(Type::None);
                            self.env
                                .trait_getters
                                .insert((name.clone(), getter.name.clone()), return_type);
                        }
                        TraitMember::Setter(setter) => {
                            let parameter_type = setter
                                .param
                                .type_annotation
                                .as_ref()
                                .map(|annotation| self.resolve_type_expr(annotation))
                                .transpose()?
                                .unwrap_or(Type::TypeVar("Any".into()));
                            self.env.trait_methods.insert(
                                (name.clone(), setter.name.clone()),
                                Type::Function {
                                    params: vec![parameter_type],
                                    return_type: Box::new(Type::None),
                                },
                            );
                        }
                        TraitMember::Field(field) => {
                            let field_type = self.resolve_type_expr(&field.type_annotation)?;
                            self.env
                                .trait_fields
                                .insert((name.clone(), field.name.clone()), field_type);
                        }
                        TraitMember::Pass(_) | TraitMember::Ellipsis(_) => {}
                    }
                }
                self.env.trait_variance.insert(
                    name.clone(),
                    type_params
                        .iter()
                        .map(|param| param.variance.clone())
                        .collect(),
                );
                self.env.trait_bounds.insert(
                    name.clone(),
                    type_params
                        .iter()
                        .map(|param| {
                            param
                                .bound
                                .as_ref()
                                .and_then(|bound| self.resolve_type_expr(bound).ok())
                        })
                        .collect(),
                );
                let trait_type = Type::Trait {
                    name: name.clone(),
                    type_args: Vec::new(),
                    methods: HashSet::new(),
                };
                self.env.traits.insert(name.clone(), trait_type);
                Ok(())
            }
            Stmt::TypeAlias {
                name,
                type_params,
                value,
                ..
            } => {
                self.env.type_alias_params.insert(
                    name.clone(),
                    type_params.iter().map(|param| param.name.clone()).collect(),
                );
                self.env.type_alias_bounds.insert(
                    name.clone(),
                    type_params
                        .iter()
                        .map(|param| {
                            param
                                .bound
                                .as_ref()
                                .and_then(|bound| self.resolve_type_expr(bound).ok())
                        })
                        .collect(),
                );
                match value {
                    TypeAliasValue::Direct(texpr) => {
                        let t = self.resolve_type_expr(texpr)?;
                        self.env.type_aliases.insert(name.clone(), t);
                    }
                    TypeAliasValue::Match { .. } => {
                        if let TypeAliasValue::Match { arms, .. } = value {
                            let resolved = arms
                                .iter()
                                .map(|(_, result)| self.resolve_type_expr(result))
                                .collect::<Result<Vec<_>, _>>()?;
                            self.env
                                .type_aliases
                                .insert(name.clone(), Type::make_union(resolved));
                        }
                    }
                }
                Ok(())
            }
            Stmt::Function(func) => {
                Self::reject_removed_decorators(func)?;
                if let Some((gather_index, gather)) = func
                    .params
                    .iter()
                    .enumerate()
                    .find(|(_, param)| param.is_gather)
                {
                    if gather_index + 1 != func.params.len()
                        || func.params.iter().skip(gather_index + 1).any(|param| param.is_gather)
                    {
                        return Err(TypeError {
                            message: format!(
                                "gather parameter '{}' must be the function's final parameter",
                                gather.name
                            ),
                            span: gather.span,
                        });
                    }
                }
                if func.decorators.iter().any(
                    |decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager"),
                ) {
                    self.env.contextmanager_functions.insert(func.name.clone());
                }
                if self.env.functions.contains_key(&func.name) {
                    if !func.is_dispatch || !self.env.dispatch_functions.contains(&func.name) {
                        return Err(TypeError {
                            message: format!(
                                "duplicate function '{}' requires dispatch on every overload",
                                func.name
                            ),
                            span: func.span,
                        });
                    }
                    self.env.overloaded_functions.insert(func.name.clone());
                }
                if func.is_dispatch {
                    self.env.dispatch_functions.insert(func.name.clone());
                }
                let mut param_types = Vec::new();
                for p in &func.params {
                    let pt = if let Some(ref t) = p.type_annotation {
                        self.resolve_type_expr(t)?
                    } else {
                        Type::TypeVar("Any".to_string())
                    };
                    param_types.push(pt);
                }
                let ret = if let Some(ref r) = func.return_type {
                    self.resolve_type_expr(r)?
                } else {
                    self.infer_unannotated_function_return(func, &param_types)
                        .unwrap_or(Type::None)
                };
                let fn_return = if func.is_async {
                    Type::Future(Box::new(ret.clone()))
                } else {
                    ret.clone()
                };
                let fn_type = Type::Function {
                    params: param_types,
                    return_type: Box::new(fn_return),
                };
                self.env
                    .function_overloads
                    .entry(func.name.clone())
                    .or_default()
                    .push(fn_type.clone());
                self.env
                    .functions
                    .insert(func.name.clone(), fn_type.clone());
                self.env
                    .variables
                    .insert(func.name.clone(), (fn_type, MutabilityView::Immutable));
                self.env
                    .function_type_params
                    .insert(func.name.clone(), func.type_params.clone());
                let required = func
                    .params
                    .iter()
                    .filter(|param| {
                        param.default.is_none()
                            && !param.is_variadic_positional
                            && !param.is_variadic_keyword
                            && !param.is_gather
                    })
                    .count();
                let maximum = (!func.params.iter().any(|param| {
                    param.is_variadic_positional || param.is_variadic_keyword || param.is_gather
                }))
                .then_some(func.params.len());
                self.env
                    .function_arity
                    .insert(func.name.clone(), (required, maximum));
                self.env.function_param_names.insert(
                    func.name.clone(),
                    func.params.iter().map(|param| param.name.clone()).collect(),
                );
                self.env.function_positional_only.insert(
                    func.name.clone(),
                    func.params
                        .iter()
                        .filter(|param| param.is_positional_only)
                        .map(|param| param.name.clone())
                        .collect(),
                );
                self.env.function_keyword_only.insert(
                    func.name.clone(),
                    func.params
                        .iter()
                        .filter(|param| param.is_keyword_only)
                        .map(|param| param.name.clone())
                        .collect(),
                );
                self.env.function_required_params.insert(
                    func.name.clone(),
                    func.params
                        .iter()
                        .filter(|param| {
                            param.default.is_none()
                                && !param.is_variadic_positional
                                && !param.is_variadic_keyword
                                && !param.is_gather
                        })
                        .map(|param| param.name.clone())
                        .collect(),
                );
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn infer_unannotated_function_return(
        &self,
        func: &FunctionDef,
        param_types: &[Type],
    ) -> Option<Type> {
        let mut checker = self.clone();
        checker.env.current_return_type = Some(Type::TypeVar("Any".into()));
        for (param, parameter_type) in func.params.iter().zip(param_types.iter()) {
            checker.env.variables.insert(
                param.name.clone(),
                (parameter_type.clone(), MutabilityView::Mutable),
            );
            if let Some(pattern) = &param.pattern {
                checker.bind_match_pattern_types(pattern, parameter_type);
            }
        }
        let return_types = checker.infer_return_types_in_statements(&func.body)?;
        if return_types.is_empty() {
            None
        } else {
            Some(Type::make_union(return_types))
        }
    }

    fn infer_return_types_in_statements(&mut self, statements: &[Stmt]) -> Option<Vec<Type>> {
        let mut return_types = Vec::new();
        for statement in statements {
            match statement {
                Stmt::Return {
                    value: Some(value), ..
                } => return_types.push(self.type_of_expr(value).ok()?),
                Stmt::Return { value: None, .. } => return_types.push(Type::None),
                Stmt::If {
                    condition,
                    then_branch,
                    elif_branches,
                    else_branch,
                    ..
                } => {
                    if !self.is_truthy_type(&self.type_of_expr(condition).ok()?) {
                        return None;
                    }
                    let mut branch_checker = self.clone();
                    return_types
                        .extend(branch_checker.infer_return_types_in_statements(then_branch)?);
                    for (elif_condition, branch) in elif_branches {
                        if !self
                            .type_of_expr(elif_condition)
                            .ok()?
                            .is_subtype_of(&Type::Bool, &self.env)
                        {
                            return None;
                        }
                        let mut branch_checker = self.clone();
                        return_types
                            .extend(branch_checker.infer_return_types_in_statements(branch)?);
                    }
                    if let Some(else_branch) = else_branch {
                        let mut branch_checker = self.clone();
                        return_types
                            .extend(branch_checker.infer_return_types_in_statements(else_branch)?);
                    }
                }
                _ => {
                    if self.check_statement(statement).is_err() {
                        return None;
                    }
                }
            }
        }
        Some(return_types)
    }

    fn reject_removed_member(name: &str, span: Span) -> Result<(), TypeError> {
        let message = match name {
            "__delitem__" => {
                "__delitem__ is not supported; use an explicit removal method instead"
            }
            "__getattr__" => "__getattr__ is not supported; declare visible members instead",
            "__getattribute__" => {
                "__getattribute__ is not supported; attribute reads use visible members"
            }
            "__setattr__" => "__setattr__ is not supported; use declared fields or setters",
            "__del__" => "__del__ is not supported; use context managers for cleanup",
            "__mro_entries__" => "__mro_entries__ is not supported; class bases are explicit",
            "__prepare__" => "__prepare__ is not supported; class bodies use normal scope",
            "__instancecheck__" => {
                "__instancecheck__ is not supported; type checks are not programmable"
            }
            "__subclasscheck__" => {
                "__subclasscheck__ is not supported; type checks are not programmable"
            }
            "__get__" => "__get__ is not supported; Lucid does not include descriptors",
            "__set__" => "__set__ is not supported; use declared fields or setters",
            "__delete__" => "__delete__ is not supported; Lucid does not include descriptors",
            "__set_name__" => {
                "__set_name__ is not supported; use caller-captured values for assignment names"
            }
            _ => return Ok(()),
        };
        Err(TypeError {
            message: message.into(),
            span,
        })
    }

    fn reject_removed_decorators(function: &FunctionDef) -> Result<(), TypeError> {
        for decorator in &function.decorators {
            let (name, span) = match decorator {
                Expr::Ident { name, span } => (name.as_str(), *span),
                Expr::Attribute {
                    value,
                    attr,
                    span,
                } if matches!(&**value, Expr::Ident { name, .. } if name == "typing") => {
                    (attr.as_str(), *span)
                }
                _ => continue,
            };
            let message = match name {
                "staticmethod" => {
                    "staticmethod is not supported; use a module-level function instead"
                }
                "classmethod" => {
                    "classmethod is not supported as a decorator; use the classmethod member modifier instead"
                }
                "property" => "property is not supported; use getter or setter syntax instead",
                "overload" => {
                    "overload is not supported as a decorator; use dispatch definitions instead"
                }
                _ => continue,
            };
            return Err(TypeError {
                message: message.into(),
                span,
            });
        }
        Ok(())
    }

    fn reject_bare_skip_value(expr: &Expr, context: &str) -> Result<(), TypeError> {
        if let Expr::Skip(span) = expr {
            return Err(TypeError {
                message: format!("skip cannot be used as a {context}; it only elides call arguments and collection entries"),
                span: *span,
            });
        }
        Ok(())
    }

    fn member_name_and_span(member: &ClassMember) -> Option<(&str, Span)> {
        match member {
            ClassMember::Field(field) | ClassMember::ClassVar(field) => {
                Some((field.name.as_str(), field.span))
            }
            ClassMember::Method(function) | ClassMember::ClassMethod(function) => {
                Some((function.name.as_str(), function.span))
            }
            ClassMember::Factory(factory) => Some((factory.name.as_str(), factory.span)),
            ClassMember::Getter(getter) => Some((getter.name.as_str(), getter.span)),
            ClassMember::Setter(setter) => Some((setter.name.as_str(), setter.span)),
            ClassMember::TypeAlias { name, span, .. } => Some((name.as_str(), *span)),
            ClassMember::Pass(_) | ClassMember::Ellipsis(_) => None,
        }
    }

    fn interface_member_name_and_span(member: &InterfaceMember) -> Option<(&str, Span)> {
        match member {
            InterfaceMember::MethodSig { name, span, .. }
            | InterfaceMember::GetterSig { name, span, .. }
            | InterfaceMember::SetterSig { name, span, .. }
            | InterfaceMember::ClassMethodSig { name, span, .. }
            | InterfaceMember::FactorySig { name, span, .. }
            | InterfaceMember::FieldSig { name, span, .. }
            | InterfaceMember::AssociatedTypeSig { name, span, .. } => Some((name.as_str(), *span)),
            InterfaceMember::Pass(_) | InterfaceMember::Ellipsis(_) => None,
        }
    }

    fn trait_member_name_and_span(member: &TraitMember) -> Option<(&str, Span)> {
        match member {
            TraitMember::Method(function) | TraitMember::ClassMethod(function) => {
                Some((function.name.as_str(), function.span))
            }
            TraitMember::Getter(getter) => Some((getter.name.as_str(), getter.span)),
            TraitMember::Setter(setter) => Some((setter.name.as_str(), setter.span)),
            TraitMember::Field(field) => Some((field.name.as_str(), field.span)),
            TraitMember::Pass(_) | TraitMember::Ellipsis(_) => None,
        }
    }

    fn verify_structure(&mut self, stmt: &Stmt) -> Result<(), TypeError> {
        match stmt {
            Stmt::Export(inner) => self.verify_structure(inner),
            Stmt::ClassDef {
                name,
                bases,
                body,
                span,
                ..
            } => {
                let mut class_parents = 0;
                for b in bases {
                    if let TypeExpr::Named { name: b_name, .. } = b {
                        if self.env.classes.contains_key(b_name) {
                            class_parents += 1;
                        }
                    }
                }

                if class_parents > 1 {
                    return Err(TypeError {
                        message: format!(
                            "class '{name}' inherits from multiple classes. Lucid requires single inheritance for classes (use traits for reusable behavior)."
                        ),
                        span: *span,
                    });
                }
                for base in bases {
                    if let TypeExpr::Named { name: parent, .. } = base {
                        // `final` is collected before base resolution, so this
                        // also catches a parent declared later in the file.
                        if self.env.final_classes.contains(parent) {
                            return Err(TypeError {
                                message: format!(
                                    "class '{parent}' is final and cannot be inherited"
                                ),
                                span: *span,
                            });
                        }
                    }
                }
                let inherited = bases
                    .iter()
                    .find_map(|base| match base {
                        TypeExpr::Named { name: parent, .. }
                            if self.env.classes.contains_key(parent)
                                || self.env.class_members.contains_key(parent) =>
                        {
                            Some(self.inherited_member_names(parent))
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                let final_inherited = bases
                    .iter()
                    .find_map(|base| match base {
                        TypeExpr::Named { name: parent, .. }
                            if self.env.classes.contains_key(parent)
                                || self.env.class_members.contains_key(parent) =>
                        {
                            Some(self.inherited_final_method_names(parent))
                        }
                        _ => None,
                    })
                    .unwrap_or_default();
                // The modifier lives on the member declaration, so inspect
                // the original AST rather than reducing it to names.
                for member in body {
                    let (member_name, explicitly_override) = match member {
                        ClassMember::Method(m) | ClassMember::ClassMethod(m) => {
                            if m.is_final && m.body.is_empty() {
                                return Err(TypeError {
                                    message: format!(
                                        "final method '{name}.{}' must have a body",
                                        m.name
                                    ),
                                    span: m.span,
                                });
                            }
                            (m.name.as_str(), m.is_override)
                        }
                        _ => continue,
                    };
                    if inherited.contains(member_name) && !explicitly_override {
                        return Err(TypeError {
                            message: format!(
                                "member '{member_name}' overrides an inherited member; write 'override' explicitly"
                            ),
                            span: *span,
                        });
                    }
                    if final_inherited.contains(member_name) {
                        return Err(TypeError {
                            message: format!(
                                "member '{member_name}' is final and cannot be overridden"
                            ),
                            span: *span,
                        });
                    }
                    if explicitly_override && !inherited.contains(member_name) {
                        return Err(TypeError {
                            message: format!(
                                "member '{member_name}' is marked 'override' but no inherited member exists"
                            ),
                            span: *span,
                        });
                    }
                }
                let mut required = HashSet::new();
                for base in bases {
                    if let TypeExpr::Named {
                        name: base_name, ..
                    } = base
                    {
                        if self.env.interfaces.contains_key(base_name) {
                            required.extend(self.interface_obligation_names(base_name));
                        } else if self.env.traits.contains_key(base_name) {
                            required.extend(self.trait_obligation_names(base_name));
                        }
                    }
                }
                let mut provided = self
                    .env
                    .class_members
                    .get(name)
                    .cloned()
                    .unwrap_or_default();
                for base in bases {
                    if let TypeExpr::Named {
                        name: base_name, ..
                    } = base
                    {
                        if let Some(members) = self.env.class_members.get(base_name) {
                            let required_for_base = self.env.obligations.get(base_name);
                            provided.extend(
                                members
                                    .iter()
                                    .filter(|member| {
                                        required_for_base
                                            .is_none_or(|required| !required.contains(*member))
                                    })
                                    .cloned(),
                            );
                        }
                    }
                }
                if let Some(missing) = required.iter().find(|member| !provided.contains(*member)) {
                    return Err(TypeError {
                        message: format!(
                            "class '{name}' does not implement required member '{missing}'"
                        ),
                        span: *span,
                    });
                }
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn inherited_member_names(&self, class_name: &str) -> HashSet<String> {
        let mut names = self
            .env
            .class_members
            .get(class_name)
            .cloned()
            .unwrap_or_default();
        if let Some(parent) = self
            .env
            .classes
            .get(class_name)
            .and_then(|ty| match ty {
                Type::Class { parent, .. } => parent.clone(),
                _ => None,
            })
            .or_else(|| self.env.class_parents.get(class_name).cloned())
        {
            names.extend(self.inherited_member_names(&parent));
        }
        names
    }

    fn inherited_final_method_names(&self, class_name: &str) -> HashSet<String> {
        let mut names = self
            .env
            .final_methods
            .get(class_name)
            .cloned()
            .unwrap_or_default();
        if let Some(parent) = self
            .env
            .classes
            .get(class_name)
            .and_then(|ty| match ty {
                Type::Class { parent, .. } => parent.clone(),
                _ => None,
            })
            .or_else(|| self.env.class_parents.get(class_name).cloned())
        {
            names.extend(self.inherited_final_method_names(&parent));
        }
        names
    }

    fn trait_obligation_names(&self, trait_name: &str) -> HashSet<String> {
        let mut names = self
            .env
            .obligations
            .get(trait_name)
            .cloned()
            .unwrap_or_default();
        if let Some(bases) = self.env.trait_bases.get(trait_name) {
            for base in bases {
                names.extend(self.trait_obligation_names(base));
            }
        }
        names
    }

    fn trait_method_type(&self, trait_name: &str, name: &str) -> Option<Type> {
        if let Some(method) = self
            .env
            .trait_methods
            .get(&(trait_name.to_string(), name.to_string()))
        {
            return Some(method.clone());
        }
        self.env
            .trait_bases
            .get(trait_name)
            .into_iter()
            .flatten()
            .find_map(|base| self.trait_method_type(base, name))
    }

    fn trait_getter_type(&self, trait_name: &str, name: &str) -> Option<Type> {
        if let Some(getter) = self
            .env
            .trait_getters
            .get(&(trait_name.to_string(), name.to_string()))
        {
            return Some(getter.clone());
        }
        self.env
            .trait_bases
            .get(trait_name)
            .into_iter()
            .flatten()
            .find_map(|base| self.trait_getter_type(base, name))
    }

    fn trait_field_type(&self, trait_name: &str, name: &str) -> Option<Type> {
        if let Some(field) = self
            .env
            .trait_fields
            .get(&(trait_name.to_string(), name.to_string()))
        {
            return Some(field.clone());
        }
        self.env
            .trait_bases
            .get(trait_name)
            .into_iter()
            .flatten()
            .find_map(|base| self.trait_field_type(base, name))
    }

    fn trait_method_params(&self, trait_name: &str, name: &str) -> Option<Vec<String>> {
        if let Some(params) = self
            .env
            .trait_method_params
            .get(&(trait_name.to_string(), name.to_string()))
        {
            return Some(
                params
                    .iter()
                    .filter(|param| !matches!(param.as_str(), "self" | "cls"))
                    .cloned()
                    .collect(),
            );
        }
        self.env
            .trait_bases
            .get(trait_name)
            .into_iter()
            .flatten()
            .find_map(|base| self.trait_method_params(base, name))
    }

    fn interface_obligation_names(&self, interface_name: &str) -> HashSet<String> {
        let mut names = self
            .env
            .obligations
            .get(interface_name)
            .cloned()
            .unwrap_or_default();
        if let Some(bases) = self.env.interface_bases.get(interface_name) {
            for base in bases {
                names.extend(self.interface_obligation_names(base));
            }
        }
        names
    }

    fn interface_method_type(&self, interface_name: &str, name: &str) -> Option<Type> {
        if let Some(method) = self
            .env
            .interface_methods
            .get(&(interface_name.to_string(), name.to_string()))
        {
            return Some(method.clone());
        }
        self.env
            .interface_bases
            .get(interface_name)
            .into_iter()
            .flatten()
            .find_map(|base| self.interface_method_type(base, name))
    }

    fn interface_getter_type(&self, interface_name: &str, name: &str) -> Option<Type> {
        if let Some(getter) = self
            .env
            .interface_getters
            .get(&(interface_name.to_string(), name.to_string()))
        {
            return Some(getter.clone());
        }
        self.env
            .interface_bases
            .get(interface_name)
            .into_iter()
            .flatten()
            .find_map(|base| self.interface_getter_type(base, name))
    }

    fn interface_field_type(&self, interface_name: &str, name: &str) -> Option<Type> {
        if let Some(field) = self
            .env
            .interface_fields
            .get(&(interface_name.to_string(), name.to_string()))
        {
            return Some(field.clone());
        }
        self.env
            .interface_bases
            .get(interface_name)
            .into_iter()
            .flatten()
            .find_map(|base| self.interface_field_type(base, name))
    }

    fn interface_method_params(&self, interface_name: &str, name: &str) -> Option<Vec<String>> {
        if let Some(params) = self
            .env
            .interface_method_params
            .get(&(interface_name.to_string(), name.to_string()))
        {
            return Some(
                params
                    .iter()
                    .filter(|param| !matches!(param.as_str(), "self" | "cls"))
                    .cloned()
                    .collect(),
            );
        }
        self.env
            .interface_bases
            .get(interface_name)
            .into_iter()
            .flatten()
            .find_map(|base| self.interface_method_params(base, name))
    }

    fn class_var_type(&self, class_name: &str, name: &str) -> Option<Type> {
        if let Some(value) = self
            .env
            .class_vars
            .get(class_name)
            .and_then(|vars| vars.get(name))
        {
            return Some(value.clone());
        }
        let parent = self.env.classes.get(class_name).and_then(|ty| match ty {
            Type::Class { parent, .. } => parent.as_deref(),
            _ => None,
        })?;
        self.class_var_type(parent, name)
    }

    fn class_field_type(&self, class_name: &str, name: &str) -> Option<Type> {
        let class = self.env.classes.get(class_name)?;
        let Type::Class { fields, parent, .. } = class else {
            return None;
        };
        fields.get(name).cloned().or_else(|| {
            parent
                .as_deref()
                .and_then(|parent| self.class_field_type(parent, name))
        })
    }

    fn class_method_type(&self, class_name: &str, name: &str) -> Option<Type> {
        if let Some(method) = self
            .env
            .class_methods
            .get(&(class_name.to_string(), name.to_string()))
        {
            return Some(method.clone());
        }
        if let Some(Type::Class { traits, .. }) = self.env.classes.get(class_name) {
            for trait_name in traits {
                if let Some(method) = self.trait_method_type(trait_name, name) {
                    return Some(method.clone());
                }
            }
        }
        if let Some(Type::Class { interfaces, .. }) = self.env.classes.get(class_name) {
            for interface_name in interfaces {
                if let Some(method) = self.interface_method_type(interface_name, name) {
                    return Some(method);
                }
            }
        }
        let parent = self
            .env
            .classes
            .get(class_name)
            .and_then(|class| match class {
                Type::Class { parent, .. } => parent.as_deref(),
                _ => None,
            })?;
        self.class_method_type(parent, name)
    }

    /// Return the value type yielded by a statically known iterable.  Built-in
    /// containers expose their element type directly; user iterators expose it
    /// through the declared return type of `next()`.
    fn iterable_element_type(&self, iterable: &Type) -> Type {
        match iterable {
            Type::View { inner, .. } => self.iterable_element_type(inner),
            Type::Class {
                name, type_args, ..
            } if matches!(name.as_str(), "list" | "set" | "dict") => type_args
                .first()
                .cloned()
                .unwrap_or(Type::TypeVar("Any".into())),
            Type::Class { name, .. } if name == "range" => Type::Int,
            Type::Shape(_) => Type::Int,
            Type::Record { fields, .. } => Type::make_union(
                fields
                    .iter()
                    .map(|(_, field_type)| field_type.clone())
                    .collect(),
            ),
            Type::Class { name, .. } => self
                .class_method_type(name, "next")
                .and_then(|method| match method {
                    Type::Function { return_type, .. } => Some(*return_type),
                    _ => None,
                })
                .unwrap_or(Type::TypeVar("Any".into())),
            _ => Type::TypeVar("Any".into()),
        }
    }

    fn is_iterable_type(&self, iterable: &Type) -> bool {
        let base = match iterable {
            Type::View { inner, .. } => inner.as_ref(),
            other => other,
        };
        if matches!(base, Type::Str) || matches!(base, Type::Class { name, .. } if name == "str") {
            return false;
        }
        if matches!(base, Type::TypeVar(name) if name == "Any") {
            return true;
        }
        matches!(
            base,
            Type::Class { name, .. }
                if matches!(name.as_str(), "list" | "set" | "dict" | "range")
        ) || matches!(base, Type::Shape(_) | Type::Record { .. })
            || matches!(base, Type::Class { name, .. }
                if self.env.class_members.get(name).is_some_and(|members| members.contains("__iter__")))
    }

    fn is_reversible_type(&self, value: &Type) -> bool {
        let base = match value {
            Type::View { inner, .. } => inner.as_ref(),
            other => other,
        };
        if matches!(base, Type::Shape(_)) {
            return true;
        }
        if matches!(base, Type::Str) || matches!(base, Type::Class { name, .. } if name == "str") {
            return false;
        }
        matches!(
            base,
            Type::Class { name, .. }
                if matches!(name.as_str(), "list" | "set" | "range")
                    || self.env.class_members.get(name).is_some_and(|members| members.contains("__reversed__"))
        )
    }

    fn is_buffer_type(&self, value: &Type) -> bool {
        let base = match value {
            Type::View { inner, .. } => inner.as_ref(),
            other => other,
        };
        if matches!(base, Type::TypeVar(name) if name == "Any") {
            return true;
        }
        if base.is_subtype_of(
            &Type::Trait {
                name: "Buffer".into(),
                type_args: Vec::new(),
                methods: HashSet::new(),
            },
            &self.env,
        ) {
            return true;
        }
        matches!(base, Type::Class { name, .. }
            if matches!(name.as_str(), "Bytes" | "ByteArray" | "MemoryView")
                || self.env.class_members.get(name).is_some_and(|members| members.contains("__buffer__")))
    }

    fn is_truthy_type(&self, value: &Type) -> bool {
        let base = match value {
            Type::View { inner, .. } => inner.as_ref(),
            other => other,
        };
        if base.is_subtype_of(&Type::Bool, &self.env)
            || matches!(base, Type::TypeVar(name) if name == "Any")
            || base.is_subtype_of(
                &Type::Trait {
                    name: "Truthy".into(),
                    type_args: Vec::new(),
                    methods: HashSet::new(),
                },
                &self.env,
            )
        {
            return true;
        }
        matches!(base, Type::Class { name, .. }
            if matches!(
                self.class_method_type(name, "__bool__"),
                Some(Type::Function { return_type, .. }) if return_type.is_subtype_of(&Type::Bool, &self.env)
            ))
    }

    fn is_byte_conversion_input(&self, value: &Type) -> bool {
        let base = match value {
            Type::View { inner, .. } => inner.as_ref(),
            other => other,
        };
        if matches!(base, Type::Str | Type::LiteralStr(_))
            || matches!(base, Type::Class { name, .. } if name == "str")
        {
            return true;
        }
        if matches!(base, Type::Class { name, type_args, .. }
            if name == "list"
                && type_args
                    .first()
                    .is_none_or(|item| item.is_subtype_of(&Type::Int, &self.env)))
        {
            return true;
        }
        if matches!(base, Type::Shape(_)) {
            return true;
        }
        self.is_buffer_type(base)
    }

    fn class_getter_type(&self, class_name: &str, name: &str) -> Option<Type> {
        if let Some(getter) = self
            .env
            .class_getters
            .get(&(class_name.to_string(), name.to_string()))
        {
            return Some(getter.clone());
        }
        if let Some(Type::Class { traits, .. }) = self.env.classes.get(class_name) {
            for trait_name in traits {
                if let Some(getter) = self.trait_getter_type(trait_name, name) {
                    return Some(getter.clone());
                }
            }
        }
        if let Some(Type::Class { interfaces, .. }) = self.env.classes.get(class_name) {
            for interface_name in interfaces {
                if let Some(getter) = self.interface_getter_type(interface_name, name) {
                    return Some(getter);
                }
            }
        }
        let parent = self
            .env
            .classes
            .get(class_name)
            .and_then(|class| match class {
                Type::Class { parent, .. } => parent.as_deref(),
                _ => None,
            })?;
        self.class_getter_type(parent, name)
    }

    fn class_method_params(&self, class_name: &str, name: &str) -> Option<Vec<String>> {
        if let Some(params) = self
            .env
            .class_method_params
            .get(&(class_name.to_string(), name.to_string()))
        {
            return Some(
                params
                    .iter()
                    .filter(|param| !matches!(param.as_str(), "self" | "cls"))
                    .cloned()
                    .collect(),
            );
        }
        if let Some(Type::Class { traits, .. }) = self.env.classes.get(class_name) {
            for trait_name in traits {
                if let Some(params) = self.trait_method_params(trait_name, name) {
                    return Some(params);
                }
            }
        }
        if let Some(Type::Class { interfaces, .. }) = self.env.classes.get(class_name) {
            for interface_name in interfaces {
                if let Some(params) = self.interface_method_params(interface_name, name) {
                    return Some(params);
                }
            }
        }
        let parent = self
            .env
            .classes
            .get(class_name)
            .and_then(|class| match class {
                Type::Class { parent, .. } => parent.as_deref(),
                _ => None,
            })?;
        self.class_method_params(parent, name)
    }

    fn class_constructor_field_count(&self, class_name: &str) -> usize {
        let own = self
            .env
            .class_constructor_arity
            .get(class_name)
            .copied()
            .unwrap_or(0);
        let parent = self
            .env
            .classes
            .get(class_name)
            .and_then(|class| match class {
                Type::Class { parent, .. } => parent.as_deref(),
                _ => None,
            });
        own + parent.map_or(0, |parent| self.class_constructor_field_count(parent))
    }

    fn class_constructor_required_count(&self, class_name: &str) -> usize {
        let own = self
            .env
            .class_constructor_required
            .get(class_name)
            .copied()
            .unwrap_or(0);
        let parent = self
            .env
            .classes
            .get(class_name)
            .and_then(|class| match class {
                Type::Class { parent, .. } => parent.as_deref(),
                _ => None,
            });
        own + parent.map_or(0, |parent| self.class_constructor_required_count(parent))
    }

    fn class_constructor_field_names(&self, class_name: &str) -> Vec<String> {
        let own = self
            .env
            .class_field_order
            .get(class_name)
            .cloned()
            .unwrap_or_default();
        let parent = self.env.classes.get(class_name).and_then(|ty| match ty {
            Type::Class { parent, .. } => parent.as_deref(),
            _ => None,
        });
        let mut names = parent
            .map(|parent| self.class_constructor_field_names(parent))
            .unwrap_or_default();
        names.extend(own);
        names
    }

    pub fn check_statement(&mut self, stmt: &Stmt) -> Result<(), TypeError> {
        if let Stmt::Export(inner) = stmt {
            let private_name = match inner.as_ref() {
                Stmt::ClassDef { name, .. }
                | Stmt::InterfaceDef { name, .. }
                | Stmt::TraitDef { name, .. }
                | Stmt::TypeAlias { name, .. }
                | Stmt::Function(FunctionDef { name, .. }) => Some(name),
                Stmt::VarDef {
                    pattern: Pattern::Ident(name, _),
                    ..
                }
                | Stmt::Assignment {
                    target: Expr::Ident { name, .. },
                    ..
                } => Some(name),
                _ => None,
            };
            if private_name.is_some_and(|name| name == "__all__") {
                let span = match inner.as_ref() {
                    Stmt::ClassDef { span, .. }
                    | Stmt::InterfaceDef { span, .. }
                    | Stmt::TraitDef { span, .. }
                    | Stmt::TypeAlias { span, .. }
                    | Stmt::VarDef { span, .. }
                    | Stmt::Assignment { span, .. } => *span,
                    Stmt::Function(FunctionDef { span, .. }) => *span,
                    _ => Span::default(),
                };
                return Err(TypeError {
                    message: "__all__ is not supported; Lucid uses leading '_' for module privacy"
                        .into(),
                    span,
                });
            }
            if let Some(name) = private_name.filter(|name| name.starts_with('_')) {
                let span = match inner.as_ref() {
                    Stmt::ClassDef { span, .. }
                    | Stmt::InterfaceDef { span, .. }
                    | Stmt::TraitDef { span, .. }
                    | Stmt::TypeAlias { span, .. }
                    | Stmt::VarDef { span, .. }
                    | Stmt::Assignment { span, .. } => *span,
                    Stmt::Function(FunctionDef { span, .. }) => *span,
                    _ => Span::default(),
                };
                return Err(TypeError {
                    message: format!("cannot export private name '{name}'"),
                    span,
                });
            }
            return self.check_statement(inner);
        }
        if let Some((name, span)) = Self::reserved_module_binding(stmt) {
            if name == "__all__" {
                return Err(TypeError {
                    message:
                        "__all__ is not supported; Lucid uses leading '_' for module privacy"
                            .into(),
                    span,
                });
            }
        }
        match stmt {
            Stmt::ClassDef {
                name, body, span, ..
            } => {
                let class_type = self
                    .env
                    .classes
                    .get(name)
                    .cloned()
                    .unwrap_or(Type::TypeVar(name.clone()));
                let saved_vars = self.env.variables.clone();
                let saved_exact_vars = self.env.exact_variables.clone();
                let saved_return = self.env.current_return_type.take();
                let saved_class = self.env.current_class.take();
                let mut class_vars = saved_vars.clone();
                class_vars.insert(
                    "Self".into(),
                    (class_type.clone(), MutabilityView::Immutable),
                );
                class_vars.insert("self".into(), (class_type.clone(), MutabilityView::Mutable));
                if let Type::Class { fields, .. } = &class_type {
                    for (field, ty) in fields {
                        class_vars.insert(field.clone(), (ty.clone(), MutabilityView::Mutable));
                    }
                }
                self.env.variables = class_vars;
                self.env.current_class = Some(name.clone());
                let result: Result<(), TypeError> = (|| {
                    for member in body {
                        match member {
                            ClassMember::Method(func) | ClassMember::ClassMethod(func) => {
                                self.check_member_function(func)?;
                            }
                            ClassMember::Factory(factory) => {
                                let previous_factory = self.env.current_factory;
                                self.env.current_factory = true;
                                let result = self.check_member_parts(
                                    &factory.params,
                                    factory.return_type.as_ref(),
                                    &factory.body,
                                    factory.span,
                                );
                                self.env.current_factory = previous_factory;
                                result?;
                            }
                            ClassMember::Getter(getter) => {
                                self.check_member_parts(
                                    &[],
                                    getter.return_type.as_ref(),
                                    &getter.body,
                                    getter.span,
                                )?;
                            }
                            ClassMember::Setter(setter) => {
                                self.check_member_parts(
                                    &[
                                        Param {
                                            name: "self".into(),
                                            pattern: None,
                                            type_annotation: None,
                                            default: None,
                                            is_positional_only: false,
                                            is_keyword_only: false,
                                            is_variadic_positional: false,
                                            is_variadic_keyword: false,
                                            is_gather: false,
                                            span: setter.span,
                                        },
                                        setter.param.clone(),
                                    ],
                                    None,
                                    &setter.body,
                                    setter.span,
                                )?;
                            }
                            _ => {}
                        }
                    }
                    Ok(())
                })();
                self.env.variables = saved_vars;
                self.env.exact_variables = saved_exact_vars;
                self.env.current_return_type = saved_return;
                self.env.current_class = saved_class;
                result.map_err(|mut error| {
                    if error.span == Span::default() {
                        error.span = *span;
                    }
                    error
                })
            }
            Stmt::ImplementDef {
                interface,
                target,
                body,
                span,
            } => {
                let target_name = match target {
                    TypeExpr::Named { name, .. } => name,
                    _ => {
                        return Err(TypeError {
                            message: "implementation target must be a named class".into(),
                            span: *span,
                        });
                    }
                };
                if !self.env.classes.contains_key(target_name) {
                    return Err(TypeError {
                        message: format!("cannot implement for unknown class '{target_name}'"),
                        span: *span,
                    });
                }
                let interface_name = match interface {
                    TypeExpr::Named { name, .. } => name,
                    _ => {
                        return Err(TypeError {
                            message: "implemented obligation must be a named trait or interface"
                                .into(),
                            span: *span,
                        });
                    }
                };
                let is_trait = self.env.traits.contains_key(interface_name);
                let is_interface = self.env.interfaces.contains_key(interface_name);
                if !is_trait && !is_interface {
                    return Err(TypeError {
                        message: format!("unknown trait or interface '{interface_name}'"),
                        span: *span,
                    });
                }
                if let Some(Type::Class {
                    traits, interfaces, ..
                }) = self.env.classes.get_mut(target_name)
                {
                    if is_trait {
                        if !traits.contains(interface_name) {
                            traits.push(interface_name.clone());
                        }
                    } else if !interfaces.contains(interface_name) {
                        interfaces.push(interface_name.clone());
                    }
                }
                let implementation_names: Vec<String> =
                    body.iter().map(|function| function.name.clone()).collect();
                self.env
                    .class_members
                    .entry(target_name.clone())
                    .or_default()
                    .extend(implementation_names);
                let saved_class = self.env.current_class.take();
                let saved_vars = self.env.variables.clone();
                let saved_exact_vars = self.env.exact_variables.clone();
                self.env.current_class = Some(target_name.clone());
                if let Some(class_type) = self.env.classes.get(target_name).cloned() {
                    self.env
                        .variables
                        .insert("self".into(), (class_type.clone(), MutabilityView::Mutable));
                    if let Type::Class { fields, .. } = class_type {
                        for (field, ty) in fields {
                            self.env
                                .variables
                                .insert(field, (ty, MutabilityView::Mutable));
                        }
                    }
                }
                let mut result = Ok(());
                for function in body {
                    // An implementation declaration contributes the same
                    // callable metadata as a method declared inside the
                    // class.  Keep the receiver out of the callable's
                    // argument list, but retain the source parameter names
                    // for named-argument validation.
                    let parameter_types = function
                        .params
                        .iter()
                        .filter(|param| !matches!(param.name.as_str(), "self" | "cls"))
                        .map(|param| {
                            param
                                .type_annotation
                                .as_ref()
                                .map(|annotation| self.resolve_type_expr(annotation))
                                .transpose()
                                .map(|ty| ty.unwrap_or(Type::TypeVar("Any".into())))
                        })
                        .collect::<Result<Vec<_>, _>>();
                    let return_type = function
                        .return_type
                        .as_ref()
                        .map(|annotation| self.resolve_type_expr(annotation))
                        .transpose()
                        .map(|ty| ty.unwrap_or(Type::None));
                    match (parameter_types, return_type) {
                        (Ok(params), Ok(return_type)) => {
                            let return_type = if function.is_async {
                                Type::Future(Box::new(return_type))
                            } else {
                                return_type
                            };
                            self.env.class_methods.insert(
                                (target_name.clone(), function.name.clone()),
                                Type::Function {
                                    params,
                                    return_type: Box::new(return_type),
                                },
                            );
                            self.env.class_method_params.insert(
                                (target_name.clone(), function.name.clone()),
                                function
                                    .params
                                    .iter()
                                    .map(|param| param.name.clone())
                                    .collect(),
                            );
                        }
                        (Err(error), _) | (_, Err(error)) => {
                            result = Err(error);
                            break;
                        }
                    }
                    let mut params = function.params.clone();
                    if !params.first().is_some_and(|param| param.name == "self") {
                        params.insert(
                            0,
                            Param {
                                name: "self".into(),
                                pattern: None,
                                type_annotation: None,
                                default: None,
                                is_positional_only: false,
                                is_keyword_only: false,
                                is_variadic_positional: false,
                                is_variadic_keyword: false,
                                is_gather: false,
                                span: function.span,
                            },
                        );
                    }
                    result = self.check_member_parts(
                        &params,
                        function.return_type.as_ref(),
                        &function.body,
                        function.span,
                    );
                    if result.is_err() {
                        break;
                    }
                }
                self.env.current_class = saved_class;
                self.env.variables = saved_vars;
                self.env.exact_variables = saved_exact_vars;
                result
            }
            Stmt::Function(func) => {
                if func.decorators.iter().any(
                    |decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager"),
                ) {
                    let yields = count_yields(&func.body);
                    if yields != 1 || !yield_guaranteed(&func.body) {
                        return Err(TypeError {
                            message: format!(
                                "contextmanager '{}' must yield exactly once on every path (found {yields})",
                                func.name
                            ),
                            span: func.span,
                        });
                    }
                }
                if count_yields(&func.body) > 0
                    && !func.decorators.iter().any(|decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager"))
                {
                    return Err(TypeError { message: "yield is only valid in a contextmanager definition".into(), span: func.span });
                }
                let ret_type = if let Some(ref r) = func.return_type {
                    Some(self.resolve_type_expr(r)?)
                } else {
                    None
                };

                let prev_ret = self.env.current_return_type.take();
                // An unannotated function still establishes a function
                // context; `Any` means returns are permitted without imposing
                // a declared result type.
                self.env.current_return_type =
                    Some(ret_type.unwrap_or_else(|| Type::TypeVar("Any".into())));

                let mut local_vars = self.env.variables.clone();
                let mut local_exact_vars = self.env.exact_variables.clone();
                let mut seen_positional_default = false;
                let mut parameter_names = HashSet::new();
                for param in &func.params {
                    if !parameter_names.insert(param.name.clone()) {
                        return Err(TypeError {
                            message: format!("duplicate parameter name '{}'", param.name),
                            span: param.span,
                        });
                    }
                    if param.default.is_some()
                        && !param.is_keyword_only
                        && !param.is_variadic_positional
                        && !param.is_variadic_keyword
                        && !param.is_gather
                    {
                        seen_positional_default = true;
                    } else if seen_positional_default
                        && !param.is_keyword_only
                        && !param.is_variadic_positional
                        && !param.is_variadic_keyword
                        && !param.is_gather
                    {
                        return Err(TypeError {
                            message: format!(
                                "required parameter '{}' follows a parameter with a default",
                                param.name
                            ),
                            span: param.span,
                        });
                    }
                    let pt = if let Some(ref t) = param.type_annotation {
                        self.resolve_type_expr(t)?
                    } else {
                        Type::TypeVar("Any".to_string())
                    };
                    if let Some(default) = &param.default {
                        let default_type = self.type_of_expr(default)?;
                        if !default_type.is_subtype_of(&pt, &self.env) {
                            return Err(TypeError {
                                message: format!(
                                    "default value for parameter '{}' does not match its annotation",
                                    param.name
                                ),
                                span: default.span(),
                            });
                        }
                    }
                    local_vars.insert(param.name.clone(), (pt.clone(), MutabilityView::Mutable));
                    local_exact_vars.remove(&param.name);
                    if let Some(pattern) = &param.pattern {
                        self.env.variables = local_vars.clone();
                        self.env.exact_variables = local_exact_vars.clone();
                        self.bind_match_pattern_types(pattern, &pt);
                        local_vars = self.env.variables.clone();
                        local_exact_vars = self.env.exact_variables.clone();
                    }
                }

                let old_vars = std::mem::replace(&mut self.env.variables, local_vars);
                let old_exact_vars =
                    std::mem::replace(&mut self.env.exact_variables, local_exact_vars);

                for s in &func.body {
                    self.check_statement(s)?;
                }

                self.env.variables = old_vars;
                self.env.exact_variables = old_exact_vars;
                self.env.current_return_type = prev_ret;
                Ok(())
            }
            Stmt::Return { value, span } => {
                let val_type = if let Some(ref e) = value {
                    Self::reject_bare_skip_value(e, "return value")?;
                    self.type_of_expr(e)?
                } else {
                    Type::None
                };

                let Some(expected) = self.env.current_return_type.as_ref() else {
                    return Err(TypeError {
                        message: "return is only valid inside a function".into(),
                        span: *span,
                    });
                };
                if !val_type.is_subtype_of(expected, &self.env) {
                    return Err(TypeError {
                        message: format!(
                            "return type mismatch: expected {:?}, got {:?}",
                            expected, val_type
                        ),
                        span: *span,
                    });
                }
                Ok(())
            }
            Stmt::Raise { exception, .. } => {
                // Exceptions represent broken invariants and remain
                // unchecked in function signatures, but their expression
                // still belongs to the checked expression language.
                Self::reject_bare_skip_value(exception, "raised value")?;
                self.type_of_expr(exception)?;
                Ok(())
            }
            Stmt::Assert {
                condition,
                message,
                span,
            } => {
                Self::reject_bare_skip_value(condition, "assert condition")?;
                let condition_type = self.type_of_expr(condition)?;
                if !self.is_truthy_type(&condition_type) {
                    return Err(TypeError {
                        message: format!("assert condition must be bool, got {:?}", condition_type),
                        span: *span,
                    });
                }
                if let Some(message) = message {
                    Self::reject_bare_skip_value(message, "assert message")?;
                    let message_type = self.type_of_expr(message)?;
                    let lazy_message = matches!(
                        &message_type,
                        Type::Function { params, return_type }
                            if params.is_empty()
                                && return_type.is_subtype_of(&Type::Str, &self.env)
                    );
                    if !message_type.is_subtype_of(&Type::Str, &self.env) && !lazy_message {
                        return Err(TypeError {
                            message: format!(
                                "assert message must be str or zero-argument function returning str, got {:?}",
                                message_type
                            ),
                            span: *span,
                        });
                    }
                }
                Ok(())
            }
            Stmt::Yield { value, .. } => {
                if self.env.current_return_type.is_none() {
                    return Err(TypeError {
                        message: "yield is only valid in a contextmanager definition".into(),
                        span: value.span(),
                    });
                }
                Self::reject_bare_skip_value(value, "yield value")?;
                let _ = self.type_of_expr(value)?;
                Ok(())
            }
            Stmt::Delete { names, span } => {
                for name in names {
                    if !self.env.variables.contains_key(name) {
                        return Err(TypeError {
                            message: format!("cannot delete undefined variable '{name}'"),
                            span: *span,
                        });
                    }
                }
                for name in names {
                    self.env.variables.remove(name);
                    self.env.exact_variables.remove(name);
                    self.env.final_variables.remove(name);
                }
                Ok(())
            }
            Stmt::VarDef {
                pattern,
                type_annotation,
                value,
                span,
                is_final,
                ..
            } => {
                let provisional = match (pattern, value.as_ref()) {
                    (
                        Pattern::Ident(name, _),
                        Some(Expr::AnonymousDef {
                            params,
                            return_type,
                            ..
                        }),
                    ) => Some((
                        name.clone(),
                        if let Some(annotation) = type_annotation {
                            self.resolve_type_expr(annotation)?
                        } else {
                            self.anonymous_function_type(params, return_type.as_ref())?
                        },
                    )),
                    _ => None,
                };
                let previous = provisional.as_ref().map(|(name, ty)| {
                    self.env
                        .variables
                        .insert(name.clone(), (ty.clone(), MutabilityView::Mutable))
                });
                let inferred_val = match value {
                    Some(e) => {
                        Self::reject_bare_skip_value(e, "variable initializer")?;
                        let typed = if let (
                            Some(annotation),
                            Expr::AnonymousDef {
                                params,
                                return_type,
                                body,
                                span,
                            },
                        ) = (type_annotation.as_ref(), e)
                        {
                            let expected = self.resolve_type_expr(annotation)?;
                            self.anonymous_function_type_against_expected(
                                params,
                                return_type.as_ref(),
                                body,
                                &expected,
                                *span,
                            )
                        } else if let (
                            Some(annotation),
                            Expr::Dict {
                                entries,
                                span: dict_span,
                            },
                        ) = (type_annotation.as_ref(), e)
                        {
                            let expected = self.resolve_type_expr(annotation)?;
                            self.dict_literal_type_against_record(entries, &expected, *dict_span)
                        } else {
                            self.type_of_expr(e)
                        };
                        match typed {
                            Ok(ty) => Some(ty),
                        Err(error) => {
                            if let Some((name, _)) = &provisional {
                                if let Some(Some(previous)) = previous.clone() {
                                    self.env.variables.insert(name.clone(), previous);
                                } else {
                                    self.env.variables.remove(name);
                                }
                            }
                            return Err(error);
                        }
                        }
                    }
                    None => None,
                };
                if let Some((name, _)) = &provisional {
                    if let Some(Some(previous)) = previous {
                        self.env.variables.insert(name.clone(), previous);
                    } else {
                        self.env.variables.remove(name);
                    }
                }

                let target_type = if let Some(ref t) = type_annotation {
                    let resolved = self.resolve_type_expr(t)?;
                    if let Some(ref iv) = inferred_val {
                        let anonymous_expected_function =
                            matches!(value.as_ref(), Some(Expr::AnonymousDef { .. }))
                                && matches!(resolved, Type::Function { .. });
                        let exact_integer_literal = matches!(
                            (&resolved, value.as_ref()),
                            (
                                Type::LiteralInt(expected),
                                Some(Expr::Literal {
                                    value: LiteralValue::Int(actual),
                                    ..
                                })
                            ) if expected == actual
                        );
                        let exact_float_literal = matches!(
                            (&resolved, value.as_ref()),
                            (
                                Type::LiteralFloat(expected),
                                Some(Expr::Literal {
                                    value: LiteralValue::Float(actual),
                                    ..
                                })
                            ) if expected == actual
                        );
                        let exact_string_literal = matches!(
                            (&resolved, value.as_ref()),
                            (
                                Type::LiteralStr(expected),
                                Some(Expr::Literal {
                                    value: LiteralValue::Str(actual),
                                    ..
                                })
                            ) if expected == actual
                        );
                        if !anonymous_expected_function
                            && !exact_integer_literal
                            && !exact_float_literal
                            && !exact_string_literal
                            && !iv.is_subtype_of(&resolved, &self.env)
                        {
                            return Err(TypeError {
                                message: format!(
                                    "type mismatch in variable definition: declared {:?}, got {:?}",
                                    resolved, iv
                                ),
                                span: *span,
                            });
                        }
                    }
                    resolved
                } else if let Some(ref iv) = inferred_val {
                    // A plain inferred binding widens literal booleans just
                    // like integer literals already widen to `int`; exact
                    // literal types remain available in annotations.
                    match iv {
                        Type::LiteralBool(_) => Type::Bool,
                        Type::LiteralFloat(_) => Type::Float,
                        Type::LiteralStr(_) => Type::Str,
                        _ => iv.clone(),
                    }
                } else {
                    Type::None
                };

                if let Pattern::Ident(name, _) = pattern {
                    if name == "_" {
                        return Ok(());
                    }
                    self.env
                        .variables
                        .insert(name.clone(), (target_type, MutabilityView::Mutable));
                    if let Some(value) = value {
                        if let Some(class_name) = self.exact_class_of_expr(value) {
                            self.env.exact_variables.insert(name.clone(), class_name);
                        } else {
                            self.env.exact_variables.remove(name);
                        }
                    } else {
                        self.env.exact_variables.remove(name);
                    }
                    if *is_final {
                        self.env.final_variables.insert(name.clone());
                    }
                } else {
                    if let Some(ref value_type) = inferred_val {
                        self.check_destructure_pattern(pattern, value_type, *span)?;
                    }
                    self.bind_match_pattern_types(pattern, &target_type);
                }
                Ok(())
            }
            Stmt::Assignment {
                target,
                value,
                span,
            } => {
                Self::reject_bare_skip_value(value, "assignment value")?;
                let exact_value_class = self.exact_class_of_expr(value);
                let provisional = match target {
                    Expr::Ident { name, .. } => match value {
                        Expr::AnonymousDef {
                            params,
                            return_type,
                            ..
                        } => Some((
                            name.clone(),
                            self.anonymous_function_type(params, return_type.as_ref())?,
                        )),
                        _ => None,
                    },
                    _ => None,
                };
                let previous = provisional.as_ref().map(|(name, ty)| {
                    self.env
                        .variables
                        .insert(name.clone(), (ty.clone(), MutabilityView::Mutable))
                });
                let val_type = match self.type_of_expr(value) {
                    Ok(ty) => ty,
                    Err(error) => {
                        if let Some((name, _)) = &provisional {
                            if let Some(Some(previous)) = previous.clone() {
                                self.env.variables.insert(name.clone(), previous);
                            } else {
                                self.env.variables.remove(name);
                            }
                        }
                        return Err(error);
                    }
                };
                if let Some((name, _)) = &provisional {
                    if let Some(Some(previous)) = previous {
                        self.env.variables.insert(name.clone(), previous);
                    } else {
                        self.env.variables.remove(name);
                    }
                }
                match target {
                    Expr::Ident { name, .. } => {
                        if name == "_" {
                            self.env.exact_variables.remove(name);
                            return Ok(());
                        }
                        if self.env.final_variables.contains(name) {
                            return Err(TypeError {
                                message: format!("cannot reassign final variable '{name}'"),
                                span: *span,
                            });
                        }
                        if let Some((existing_type, view)) = self.env.variables.get(name).cloned() {
                            if view == MutabilityView::ReadOnly || view == MutabilityView::Immutable
                            {
                                return Err(TypeError {
                                    message: format!(
                                        "cannot reassign to read-only or immutable variable '{name}'"
                                    ),
                                    span: *span,
                                });
                            }
                            if !val_type.is_subtype_of(&existing_type, &self.env) {
                                return Err(TypeError {
                                    message: format!(
                                        "cannot assign type {:?} to variable '{name}' of type {:?}",
                                        val_type, existing_type
                                    ),
                                    span: *span,
                                });
                            }
                        } else {
                            let inferred_type = match val_type {
                                Type::LiteralBool(_) => Type::Bool,
                                Type::LiteralFloat(_) => Type::Float,
                                Type::LiteralStr(_) => Type::Str,
                                other => other,
                            };
                            self.env
                                .variables
                                .insert(name.clone(), (inferred_type, MutabilityView::Mutable));
                        }
                        if let Some(class_name) = exact_value_class.clone() {
                            self.env.exact_variables.insert(name.clone(), class_name);
                        } else {
                            self.env.exact_variables.remove(name);
                        }
                    }
                    Expr::Record { fields, .. } => {
                        let element_type = match &val_type {
                            Type::Class {
                                name, type_args, ..
                            } if name == "list" => type_args
                                .first()
                                .cloned()
                                .unwrap_or(Type::TypeVar("Any".into())),
                            Type::Record { fields: values, .. } => values
                                .first()
                                .map(|(_, ty)| ty.clone())
                                .unwrap_or(Type::TypeVar("Any".into())),
                            Type::Shape(_) => Type::TypeVar("Any".into()),
                            _ => {
                                return Err(TypeError {
                                    message: format!(
                                        "cannot unpack assignment value of type {:?}",
                                        val_type
                                    ),
                                    span: *span,
                                });
                            }
                        };
                        for (_, target_expr) in fields {
                            let name = match target_expr {
                                Expr::Ident { name, .. } => Some(name),
                                Expr::Unary { expr, .. } => match &**expr {
                                    Expr::Ident { name, .. } => Some(name),
                                    _ => None,
                                },
                                _ => None,
                            };
                            if let Some(name) = name {
                                if name == "_" {
                                    continue;
                                }
                                if self.env.final_variables.contains(name) {
                                    return Err(TypeError {
                                        message: format!("cannot reassign final variable '{name}'"),
                                        span: *span,
                                    });
                                }
                                self.env.variables.insert(
                                    name.clone(),
                                    (element_type.clone(), MutabilityView::Mutable),
                                );
                            }
                        }
                    }
                    Expr::Attribute {
                        value: obj_expr,
                        attr,
                        ..
                    } => {
                        let obj_type = self.type_of_expr(obj_expr)?;
                        let class_name = match &obj_type {
                            Type::Class { name, .. } => Some(name.clone()),
                            Type::View { inner, .. } => match inner.as_ref() {
                                Type::Class { name, .. } => Some(name.clone()),
                                _ => None,
                            },
                            _ => None,
                        };
                        if let Some(class_name) = class_name.as_ref() {
                            let mut current = Some(class_name.clone());
                            while let Some(name) = current {
                                if self
                                    .env
                                    .final_fields
                                    .get(&name)
                                    .is_some_and(|fields| fields.contains(attr))
                                {
                                    return Err(TypeError {
                                        message: format!("cannot reassign final field '{attr}'"),
                                        span: *span,
                                    });
                                }
                                current = self.env.classes.get(&name).and_then(|ty| match ty {
                                    Type::Class { parent, .. } => parent.clone(),
                                    _ => None,
                                });
                            }
                        }
                        if let Type::View { mutability, .. } = &obj_type {
                            if *mutability == MutabilityView::ReadOnly
                                || *mutability == MutabilityView::Immutable
                            {
                                return Err(TypeError {
                                    message: format!(
                                        "cannot mutate attribute '{attr}' on read-only/immutable view"
                                    ),
                                    span: *span,
                                });
                            }
                        }
                        if let Some(class_name) = class_name.as_deref() {
                            if attr.starts_with('_')
                                && self.env.current_class.as_deref() != Some(class_name)
                            {
                                return Err(TypeError {
                                    message: format!(
                                        "member '{attr}' is private to class '{class_name}'"
                                    ),
                                    span: *span,
                                });
                            }
                            if let Some(field_type) = self
                                .class_field_type(class_name, attr)
                                .or_else(|| self.class_var_type(class_name, attr))
                            {
                                if !val_type.is_subtype_of(&field_type, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "cannot assign type {:?} to field '{}' of type {:?}",
                                            val_type, attr, field_type
                                        ),
                                        span: *span,
                                    });
                                }
                            } else if let Some(Type::Function { params, .. }) =
                                self.class_method_type(class_name, attr)
                            {
                                if let Some(parameter_type) = params.first() {
                                    if !val_type.is_subtype_of(parameter_type, &self.env) {
                                        return Err(TypeError {
                                            message: format!(
                                                "cannot assign type {:?} to setter '{}' expecting {:?}",
                                                val_type, attr, parameter_type
                                            ),
                                            span: *span,
                                        });
                                    }
                                }
                            } else {
                                return Err(TypeError {
                                    message: format!(
                                        "class '{class_name}' has no writable member '{attr}'"
                                    ),
                                    span: *span,
                                });
                            }
                        } else {
                            let setter_type = match &obj_type {
                                Type::Interface { name, .. } => {
                                    self.interface_method_type(name, attr)
                                }
                                Type::Trait { name, .. } => self.trait_method_type(name, attr),
                                Type::View { inner, .. } => match inner.as_ref() {
                                    Type::Interface { name, .. } => {
                                        self.interface_method_type(name, attr)
                                    }
                                    Type::Trait { name, .. } => self.trait_method_type(name, attr),
                                    _ => None,
                                },
                                _ => None,
                            };
                            if let Some(Type::Function { params, .. }) = setter_type {
                                if let Some(parameter_type) = params.first() {
                                    if !val_type.is_subtype_of(parameter_type, &self.env) {
                                        return Err(TypeError {
                                            message: format!(
                                                "cannot assign type {:?} to setter '{}' expecting {:?}",
                                                val_type, attr, parameter_type
                                            ),
                                            span: *span,
                                        });
                                    }
                                }
                            } else if matches!(
                                obj_type,
                                Type::Interface { .. } | Type::Trait { .. } | Type::View { .. }
                            ) {
                                return Err(TypeError {
                                    message: format!(
                                        "no writable member '{attr}' is declared by the receiver type"
                                    ),
                                    span: *span,
                                });
                            } else if let Type::Record { fields, .. } = &obj_type {
                                if let Some((_, field_type)) = fields
                                    .iter()
                                    .find(|(name, _)| name.as_deref() == Some(attr))
                                {
                                    if !val_type.is_subtype_of(field_type, &self.env) {
                                        return Err(TypeError {
                                            message: format!(
                                                "cannot assign type {:?} to field '{}' of type {:?}",
                                                val_type, attr, field_type
                                            ),
                                            span: *span,
                                        });
                                    }
                                }
                            }
                        }
                    }
                    Expr::Index {
                        value: object,
                        index,
                        ..
                    } => {
                        let object_type = self.type_of_expr(object)?;
                        let sequence_type = match object_type {
                            Type::View { mutability, inner } => {
                                if matches!(
                                    mutability,
                                    MutabilityView::ReadOnly | MutabilityView::Immutable
                                ) {
                                    return Err(TypeError {
                                        message:
                                            "cannot mutate through read-only or immutable view"
                                                .into(),
                                        span: *span,
                                    });
                                }
                                *inner
                            }
                            other => other,
                        };
                        match sequence_type {
                            Type::Record { fields, .. } => {
                                let field_type = match &**index {
                                    Expr::Literal {
                                        value: LiteralValue::Int(position),
                                        ..
                                    } => {
                                        let position = if *position < 0 {
                                            fields
                                                .len()
                                                .checked_sub(position.unsigned_abs() as usize)
                                        } else {
                                            usize::try_from(*position).ok()
                                        };
                                        position
                                            .and_then(|position| fields.get(position))
                                            .map(|(_, ty)| ty.clone())
                                            .ok_or_else(|| TypeError {
                                                message: "record index is out of bounds".into(),
                                                span: index.span(),
                                            })?
                                    }
                                    Expr::Literal {
                                        value: LiteralValue::Str(field),
                                        ..
                                    } => fields
                                        .iter()
                                        .find(|(name, _)| name.as_deref() == Some(field))
                                        .map(|(_, ty)| ty.clone())
                                        .ok_or_else(|| TypeError {
                                            message: format!("record has no field '{field}'"),
                                            span: index.span(),
                                        })?,
                                    _ => {
                                        let index_type = self.type_of_expr(index)?;
                                        if index_type.is_subtype_of(&Type::Int, &self.env)
                                            || index_type.is_subtype_of(&Type::Str, &self.env)
                                        {
                                            Type::make_union(
                                                fields
                                                    .iter()
                                                    .map(|(_, ty)| ty.clone())
                                                    .collect(),
                                            )
                                        } else {
                                            return Err(TypeError {
                                                message: format!(
                                                    "record index must be int or str, got {:?}",
                                                    index_type
                                                ),
                                                span: index.span(),
                                            });
                                        }
                                    }
                                };
                                if !val_type.is_subtype_of(&field_type, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "cannot assign {:?} into record field of {:?}",
                                            val_type, field_type
                                        ),
                                        span: *span,
                                    });
                                }
                            }
                            Type::Class {
                                name, type_args, ..
                            } if name == "list" => {
                                let index_type = self.type_of_expr(index)?;
                                if !index_type.is_subtype_of(&Type::Int, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "sequence index must be int, got {:?}",
                                            index_type
                                        ),
                                        span: index.span(),
                                    });
                                }
                                if let Some(element) = type_args.first() {
                                    if !val_type.is_subtype_of(element, &self.env) {
                                        return Err(TypeError {
                                            message: format!(
                                                "cannot assign {:?} into list of {:?}",
                                                val_type, element
                                            ),
                                            span: *span,
                                        });
                                    }
                                }
                            }
                            Type::Class { name, .. }
                                if matches!(name.as_str(), "ByteArray" | "MemoryView") =>
                            {
                                let index_type = self.type_of_expr(index)?;
                                if !index_type.is_subtype_of(&Type::Int, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "sequence index must be int, got {:?}",
                                            index_type
                                        ),
                                        span: index.span(),
                                    });
                                }
                                if !val_type.is_subtype_of(&Type::Int, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "byte buffer elements must be int, got {:?}",
                                            val_type
                                        ),
                                        span: *span,
                                    });
                                }
                            }
                            Type::Class { name, .. } if name == "Bytes" => {
                                return Err(TypeError {
                                    message: "cannot assign into immutable Bytes".into(),
                                    span: *span,
                                });
                            }
                            _ => {}
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            Stmt::Match {
                subject,
                arms,
                subject_alias,
                span,
                ..
            } => {
                let subject_type = self.type_of_expr(subject)?;
                self.check_match_exhaustiveness(&subject_type, arms, *span)?;
                let saved_match_vars = self.env.variables.clone();
                let saved_match_exact_vars = self.env.exact_variables.clone();
                if let Some(alias) = subject_alias {
                    self.env.variables.insert(
                        alias.clone(),
                        (subject_type.clone(), MutabilityView::ReadOnly),
                    );
                    self.env.exact_variables.remove(alias);
                }
                for arm in arms {
                    if let Some(guard) = &arm.guard {
                        let guard_type = self.type_of_expr(guard)?;
                        if !self.is_truthy_type(&guard_type) {
                            return Err(TypeError {
                                message: format!("match guard must be bool, got {:?}", guard_type),
                                span: guard.span(),
                            });
                        }
                    }
                    let saved_vars = self.env.variables.clone();
                    let saved_exact_vars = self.env.exact_variables.clone();
                    self.bind_match_pattern_types(&arm.pattern, &subject_type);
                    if let Some(alias) = subject_alias {
                        let narrowed = match &arm.pattern {
                            Pattern::Type(type_expr, _) => self
                                .resolve_type_expr(type_expr)
                                .unwrap_or(subject_type.clone()),
                            Pattern::ClassDestructure { class_name, .. } => self
                                .env
                                .classes
                                .get(class_name)
                                .cloned()
                                .unwrap_or(subject_type.clone()),
                            Pattern::Ident(name, _) => self
                                .env
                                .classes
                                .get(name)
                                .cloned()
                                .or_else(|| match name.as_str() {
                                    "int" => Some(Type::Int),
                                    "float" => Some(Type::Float),
                                    "bool" => Some(Type::Bool),
                                    "str" => Some(Type::Str),
                                    "none" => Some(Type::None),
                                    _ => None,
                                })
                                .unwrap_or(subject_type.clone()),
                            _ => subject_type.clone(),
                        };
                        self.env
                            .variables
                            .insert(alias.clone(), (narrowed, MutabilityView::ReadOnly));
                        self.env.exact_variables.remove(alias);
                    }
                    for s in &arm.body {
                        self.check_statement(s)?;
                    }
                    self.env.variables = saved_vars;
                    self.env.exact_variables = saved_exact_vars;
                }
                self.env.variables = saved_match_vars;
                self.env.exact_variables = saved_match_exact_vars;
                Ok(())
            }
            Stmt::If {
                condition,
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                Self::reject_bare_skip_value(condition, "if condition")?;
                let cond_type = self.type_of_expr(condition)?;
                if !self.is_truthy_type(&cond_type) {
                    return Err(TypeError {
                        message: format!("if condition must be bool, got {:?}", cond_type),
                        span: condition.span(),
                    });
                }
                for s in then_branch {
                    self.check_statement(s)?;
                }
                for (c, b) in elif_branches {
                    Self::reject_bare_skip_value(c, "elif condition")?;
                    let elif_type = self.type_of_expr(c)?;
                    if !self.is_truthy_type(&elif_type) {
                        return Err(TypeError {
                            message: format!("elif condition must be bool, got {:?}", elif_type),
                            span: c.span(),
                        });
                    }
                    for s in b {
                        self.check_statement(s)?;
                    }
                }
                if let Some(ref eb) = else_branch {
                    for s in eb {
                        self.check_statement(s)?;
                    }
                }
                Ok(())
            }
            Stmt::For {
                target,
                iterable,
                body,
                if_broken,
                ..
            } => {
                let iter_type = self.type_of_expr(iterable)?;
                let iterable_base = match &iter_type {
                    Type::View { inner, .. } => inner.as_ref(),
                    other => other,
                };
                let is_string = matches!(iterable_base, Type::Str)
                    || matches!(iterable_base, Type::Class { name, .. } if name == "str");
                if is_string {
                    return Err(TypeError {
                        message: "str is not Iterable; iterate over str.chars instead".into(),
                        span: iterable.span(),
                    });
                }
                if !self.is_iterable_type(&iter_type) {
                    return Err(TypeError {
                        message: format!("value of type {:?} is not iterable", iter_type),
                        span: iterable.span(),
                    });
                }
                let elem_type = self.iterable_element_type(&iter_type);

                let old_vars = self.env.variables.clone();
                let old_exact_vars = self.env.exact_variables.clone();
                self.bind_match_pattern_types(target, &elem_type);

                self.env.loop_depth += 1;
                for s in body {
                    self.check_statement(s)?;
                }
                self.env.loop_depth -= 1;
                self.env.variables = old_vars;
                self.env.exact_variables = old_exact_vars;

                if let Some(ref ib) = if_broken {
                    for s in ib {
                        self.check_statement(s)?;
                    }
                }
                Ok(())
            }
            Stmt::While {
                condition,
                body,
                if_broken,
                ..
            } => {
                Self::reject_bare_skip_value(condition, "while condition")?;
                let condition_type = self.type_of_expr(condition)?;
                if !self.is_truthy_type(&condition_type) {
                    return Err(TypeError {
                        message: format!("while condition must be bool, got {:?}", condition_type),
                        span: condition.span(),
                    });
                }
                self.env.loop_depth += 1;
                for s in body {
                    self.check_statement(s)?;
                }
                self.env.loop_depth -= 1;
                if let Some(ref ib) = if_broken {
                    for s in ib {
                        self.check_statement(s)?;
                    }
                }
                Ok(())
            }
            Stmt::With { items, body, .. } => {
                for item in items {
                    let ctx_ty = self.type_of_expr(&item.context_expr)?;
                    match &ctx_ty {
                        Type::Class { name, .. } => {
                            let has_context_manager = self
                                .env
                                .class_members
                                .get(name)
                                .is_some_and(|members| members.contains("__cm__"));
                            if !has_context_manager {
                                return Err(TypeError {
                                    message: format!(
                                        "class '{name}' is not a contextmanager; declare __cm__"
                                    ),
                                    span: item.context_expr.span(),
                                });
                            }
                        }
                        Type::View { inner, .. } => {
                            let Some(Type::Class { name, .. }) = Some(inner.as_ref()) else {
                                return Err(TypeError {
                                    message: format!(
                                        "with expression must be a contextmanager, got {:?}",
                                        ctx_ty
                                    ),
                                    span: item.context_expr.span(),
                                });
                            };
                            let has_context_manager = self
                                .env
                                .class_members
                                .get(name)
                                .is_some_and(|members| members.contains("__cm__"));
                            if !has_context_manager {
                                return Err(TypeError {
                                    message: format!(
                                        "class '{name}' is not a contextmanager; declare __cm__"
                                    ),
                                    span: item.context_expr.span(),
                                });
                            }
                        }
                        Type::TypeVar(name)
                            if matches!(name.as_str(), "Any" | "ContextManager") => {}
                        other => {
                            return Err(TypeError {
                                message: format!(
                                    "with expression must be a contextmanager, got {:?}",
                                    other
                                ),
                                span: item.context_expr.span(),
                            });
                        }
                    }
                    if let Some(ref target) = item.target {
                        self.bind_match_pattern_types(target, &ctx_ty);
                    }
                }
                for s in body {
                    self.check_statement(s)?;
                }
                Ok(())
            }
            Stmt::Try {
                body,
                handlers,
                finally_body,
                ..
            } => {
                for statement in body {
                    self.check_statement(statement)?;
                }
                for handler in handlers {
                    let handler_type = self.resolve_type_expr(&handler.exception_type)?;
                    let previous = handler.name.as_ref().and_then(|name| {
                        self.env.variables.insert(
                            name.clone(),
                            (handler_type.clone(), MutabilityView::Mutable),
                        )
                    });
                    for statement in &handler.body {
                        self.check_statement(statement)?;
                    }
                    if let Some(name) = &handler.name {
                        if let Some(previous) = previous {
                            self.env.variables.insert(name.clone(), previous);
                        } else {
                            self.env.variables.remove(name);
                        }
                    }
                }
                if let Some(finally_body) = finally_body {
                    for statement in finally_body {
                        self.check_statement(statement)?;
                    }
                }
                Ok(())
            }
            Stmt::Import {
                module,
                alias,
                span,
            } => {
                let bound_name = alias
                    .clone()
                    .unwrap_or_else(|| module.rsplit('.').next().unwrap_or(module).to_string());
                if !self.env.import_bindings.insert(bound_name.clone()) {
                    return Err(TypeError {
                        message: format!("duplicate imported binding '{bound_name}'"),
                        span: *span,
                    });
                }
                self.env.variables.insert(
                    bound_name,
                    (
                        Type::TypeVar("module".to_string()),
                        MutabilityView::ReadOnly,
                    ),
                );
                Ok(())
            }
            Stmt::FromImport {
                module,
                names,
                span,
                ..
            } => {
                for (name, alias) in names {
                    if name.starts_with('_') {
                        return Err(TypeError {
                            message: format!(
                                "cannot import private name '{name}' from module '{module}'"
                            ),
                            span: *span,
                        });
                    }
                    let bound_name = alias.as_ref().unwrap_or(name).clone();
                    if !self.env.import_bindings.insert(bound_name.clone()) {
                        return Err(TypeError {
                            message: format!("duplicate imported binding '{bound_name}'"),
                            span: *span,
                        });
                    }
                    self.env.variables.insert(
                        bound_name,
                        (Type::TypeVar("Any".to_string()), MutabilityView::ReadOnly),
                    );
                }
                Ok(())
            }
            Stmt::AugAssign {
                target,
                op,
                value,
                span,
            } => {
                let target_type = self.type_of_expr(target)?;
                let value_type = self.type_of_expr(value)?;
                let result_type = self.type_of_expr(&Expr::Binary {
                    op: op.clone(),
                    left: Box::new(target.clone()),
                    right: Box::new(value.clone()),
                    span: *span,
                })?;
                if !result_type.is_subtype_of(&target_type, &self.env) {
                    return Err(TypeError {
                        message: format!(
                            "augmented assignment produces {:?}, but target has type {:?} (value is {:?})",
                            result_type, target_type, value_type
                        ),
                        span: *span,
                    });
                }
                match target {
                    Expr::Ident { name, .. } => {
                        if let Some((_, view)) = self.env.variables.get(name) {
                            if *view == MutabilityView::ReadOnly
                                || *view == MutabilityView::Immutable
                            {
                                return Err(TypeError {
                                    message: format!(
                                        "cannot reassign to read-only or immutable variable '{name}'"
                                    ),
                                    span: *span,
                                });
                            }
                        }
                    }
                    Expr::Attribute {
                        value: obj_expr,
                        attr,
                        ..
                    } => {
                        let obj_type = self.type_of_expr(obj_expr)?;
                        if let Type::View { mutability, .. } = obj_type {
                            if mutability == MutabilityView::ReadOnly
                                || mutability == MutabilityView::Immutable
                            {
                                return Err(TypeError {
                                    message: format!(
                                        "cannot mutate attribute '{attr}' on read-only/immutable view"
                                    ),
                                    span: *span,
                                });
                            }
                        }
                    }
                    _ => {}
                }
                Ok(())
            }
            Stmt::Break(span) => {
                if self.env.loop_depth == 0 {
                    return Err(TypeError {
                        message: "break is only valid inside a loop".into(),
                        span: *span,
                    });
                }
                Ok(())
            }
            Stmt::Continue(span) => {
                if self.env.loop_depth == 0 {
                    return Err(TypeError {
                        message: "continue is only valid inside a loop".into(),
                        span: *span,
                    });
                }
                Ok(())
            }
            Stmt::Expr(expr) => {
                // Expression statements still need full type checking.  The
                // result is intentionally discarded, but evaluating the
                // expression can expose invalid operators, calls, and
                // attribute accesses just like an expression used in an
                // assignment.
                Self::reject_bare_skip_value(expr, "expression statement")?;
                self.type_of_expr(expr)?;
                Ok(())
            }
            _ => Ok(()),
        }
    }

    fn reserved_module_binding(stmt: &Stmt) -> Option<(&str, Span)> {
        match stmt {
            Stmt::Export(inner) => Self::reserved_module_binding(inner),
            Stmt::ClassDef { name, span, .. }
            | Stmt::InterfaceDef { name, span, .. }
            | Stmt::TraitDef { name, span, .. }
            | Stmt::TypeAlias { name, span, .. } => Some((name.as_str(), *span)),
            Stmt::Function(FunctionDef { name, span, .. }) => Some((name.as_str(), *span)),
            Stmt::VarDef {
                pattern: Pattern::Ident(name, _),
                span,
                ..
            }
            | Stmt::Assignment {
                target: Expr::Ident { name, .. },
                span,
                ..
            } => Some((name.as_str(), *span)),
            _ => None,
        }
    }

    fn check_member_function(&mut self, func: &FunctionDef) -> Result<(), TypeError> {
        let yields = count_yields(&func.body);
        let is_contextmanager = func.decorators.iter().any(
            |decorator| matches!(decorator, Expr::Ident { name, .. } if name == "contextmanager"),
        );
        if yields > 0 && !is_contextmanager {
            return Err(TypeError {
                message: "yield is only valid in a contextmanager definition".into(),
                span: func.span,
            });
        }
        self.check_member_parts(
            &func.params,
            func.return_type.as_ref(),
            &func.body,
            func.span,
        )
    }

    fn check_member_parts(
        &mut self,
        params: &[Param],
        return_expr: Option<&TypeExpr>,
        body: &[Stmt],
        span: Span,
    ) -> Result<(), TypeError> {
        if let Some((gather_index, gather)) = params
            .iter()
            .enumerate()
            .find(|(_, param)| param.is_gather)
        {
            if gather_index + 1 != params.len()
                || params.iter().skip(gather_index + 1).any(|param| param.is_gather)
            {
                return Err(TypeError {
                    message: format!(
                        "gather parameter '{}' must be the method's final parameter",
                        gather.name
                    ),
                    span: gather.span,
                });
            }
        }
        let saved_vars = self.env.variables.clone();
        let saved_exact_vars = self.env.exact_variables.clone();
        let saved_return = self.env.current_return_type.take();
        let mut vars = saved_vars.clone();
        let mut exact_vars = saved_exact_vars.clone();
        for param in params {
            let ty = if param.type_annotation.is_none()
                && matches!(param.name.as_str(), "self" | "cls")
            {
                self.env
                    .current_class
                    .as_ref()
                    .and_then(|name| self.env.classes.get(name))
                    .cloned()
                    .unwrap_or(Type::TypeVar("Any".into()))
            } else {
                param
                    .type_annotation
                    .as_ref()
                    .map(|expr| self.resolve_type_expr(expr))
                    .transpose()?
                    .unwrap_or(Type::TypeVar("Any".into()))
            };
            vars.insert(param.name.clone(), (ty.clone(), MutabilityView::Mutable));
            exact_vars.remove(&param.name);
            if let Some(pattern) = &param.pattern {
                self.env.variables = vars.clone();
                self.env.exact_variables = exact_vars.clone();
                self.bind_match_pattern_types(pattern, &ty);
                vars = self.env.variables.clone();
                exact_vars = self.env.exact_variables.clone();
            }
        }
        self.env.variables = vars;
        self.env.exact_variables = exact_vars;
        self.env.current_return_type = Some(
            return_expr
                .map(|expr| self.resolve_type_expr(expr))
                .transpose()?
                .unwrap_or_else(|| Type::TypeVar("Any".into())),
        );
        let result = (|| {
            for statement in body {
                self.check_statement(statement)?;
            }
            Ok(())
        })();
        self.env.variables = saved_vars;
        self.env.exact_variables = saved_exact_vars;
        self.env.current_return_type = saved_return;
        result.map_err(|mut error: TypeError| {
            if error.span == Span::default() {
                error.span = span;
            }
            error
        })
    }

    fn check_match_exhaustiveness(
        &self,
        subject_type: &Type,
        arms: &[MatchArm],
        span: Span,
    ) -> Result<(), TypeError> {
        let has_wildcard = arms
            .iter()
            .any(|arm| arm.guard.is_none() && matches!(arm.pattern, Pattern::Wildcard(_)));
        if has_wildcard {
            return Ok(());
        }

        match subject_type {
            Type::Union(variants) => {
                let mut uncovered = variants.clone();
                for arm in arms {
                    // A guarded pattern may fail at runtime and therefore
                    // cannot establish exhaustiveness by itself.
                    if arm.guard.is_some() {
                        continue;
                    }
                    match &arm.pattern {
                        Pattern::Ident(name, _)
                        | Pattern::Type(TypeExpr::Named { name, .. }, _)
                        | Pattern::ClassDestructure {
                            class_name: name, ..
                        } => {
                            uncovered.retain(|v| match v {
                                Type::Class { name: c_name, .. } => c_name != name,
                                Type::Int if name == "int" => false,
                                Type::Float if name == "float" => false,
                                Type::Bool if name == "bool" => false,
                                Type::Str if name == "str" => false,
                                Type::None if name == "none" => false,
                                _ => true,
                            });
                        }
                        Pattern::Wildcard(_) => {
                            uncovered.clear();
                        }
                        _ => {}
                    }
                }

                if !uncovered.is_empty() {
                    return Err(TypeError {
                        message: format!(
                            "non-exhaustive match: uncovered variants: {:?}",
                            uncovered
                        ),
                        span,
                    });
                }
            }
            Type::Class {
                name,
                is_sealed: true,
                ..
            } => {
                if let Some(subclasses) = self.env.sealed_subclasses.get(name) {
                    let mut uncovered = subclasses.clone();
                    for arm in arms {
                        match &arm.pattern {
                            Pattern::Ident(c_name, _)
                            | Pattern::ClassDestructure {
                                class_name: c_name, ..
                            } => {
                                uncovered.retain(|sub| sub != c_name);
                            }
                            Pattern::Wildcard(_) => {
                                uncovered.clear();
                            }
                            _ => {}
                        }
                    }
                    if !uncovered.is_empty() {
                        return Err(TypeError {
                            message: format!(
                                "non-exhaustive match on sealed class '{name}': uncovered subclasses: {:?}",
                                uncovered
                            ),
                            span,
                        });
                    }
                }
            }
            _ => {}
        }
        Ok(())
    }

    fn bind_match_pattern_types(&mut self, pattern: &Pattern, subject_type: &Type) {
        match pattern {
            Pattern::Ident(name, _)
                if !matches!(
                    name.as_str(),
                    "_" | "int" | "float" | "bool" | "str" | "none" | "None"
                ) && !self.env.classes.contains_key(name) =>
            {
                self.env.variables.insert(
                    name.clone(),
                    (subject_type.clone(), MutabilityView::Mutable),
                );
                self.env.exact_variables.remove(name);
            }
            Pattern::Tuple(items, _) => {
                let element_types = match subject_type {
                    Type::Record { fields, .. } => {
                        fields.iter().map(|(_, ty)| ty.clone()).collect::<Vec<_>>()
                    }
                    Type::Class {
                        name, type_args, ..
                    } if name == "list" => {
                        let element = type_args
                            .first()
                            .cloned()
                            .unwrap_or(Type::TypeVar("Any".into()));
                        vec![element; items.len()]
                    }
                    _ => Vec::new(),
                };
                for (index, item) in items.iter().enumerate() {
                    self.bind_match_pattern_types(
                        item,
                        element_types
                            .get(index)
                            .unwrap_or(&Type::TypeVar("Any".into())),
                    );
                }
            }
            Pattern::Star(nested, _) => {
                let list_type = Type::Class {
                    name: "list".into(),
                    type_args: vec![subject_type.clone()],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                };
                self.bind_match_pattern_types(nested, &list_type);
            }
            Pattern::RecordDestructure(fields, _) => {
                for (field_name, nested) in fields {
                    let field_type = field_name
                        .as_ref()
                        .and_then(|name| match subject_type {
                            Type::Class { fields, .. } => fields.get(name),
                            Type::Record { fields, .. } => fields
                                .iter()
                                .find(|(field, _)| field.as_ref() == Some(name))
                                .map(|(_, ty)| ty),
                            _ => None,
                        })
                        .cloned()
                        .unwrap_or(Type::TypeVar("Any".into()));
                    self.bind_match_pattern_types(nested, &field_type);
                }
            }
            Pattern::ClassDestructure {
                class_name, fields, ..
            } => {
                let declared = self
                    .env
                    .classes
                    .get(class_name)
                    .and_then(|ty| match ty {
                        Type::Class { fields, .. } => Some(fields),
                        _ => None,
                    })
                    .cloned();
                let order = self.env.class_field_order.get(class_name).cloned();
                for (index, (field_name, nested)) in fields.iter().enumerate() {
                    let field_type = field_name
                        .as_ref()
                        .and_then(|name| declared.as_ref().and_then(|fields| fields.get(name)))
                        .or_else(|| {
                            order
                                .as_ref()
                                .and_then(|order| order.get(index))
                                .and_then(|name| {
                                    declared.as_ref().and_then(|fields| fields.get(name))
                                })
                        })
                        .cloned()
                        .unwrap_or(Type::TypeVar("Any".into()));
                    self.bind_match_pattern_types(nested, &field_type);
                }
            }
            _ => {}
        }
    }

    fn check_destructure_pattern(
        &self,
        pattern: &Pattern,
        subject_type: &Type,
        span: Span,
    ) -> Result<(), TypeError> {
        match pattern {
            Pattern::ClassDestructure {
                class_name, fields, ..
            } => {
                let expected = self.env.classes.get(class_name).ok_or_else(|| TypeError {
                    message: format!("unknown class in destructuring pattern '{class_name}'"),
                    span,
                })?;
                if !subject_type.is_subtype_of(expected, &self.env) {
                    return Err(TypeError {
                        message: format!("cannot destructure {:?} as {class_name}", subject_type),
                        span,
                    });
                }
                if let Type::Class {
                    fields: declared, ..
                } = expected
                {
                    if fields.len() > declared.len() {
                        return Err(TypeError {
                            message: format!(
                                "too many fields in {class_name} destructuring pattern"
                            ),
                            span,
                        });
                    }
                }
                Ok(())
            }
            Pattern::Tuple(items, _) => match subject_type {
                Type::Record { fields, .. }
                    if items.len().saturating_sub(usize::from(
                        items.iter().any(|p| matches!(p, Pattern::Star(_, _))),
                    )) <= fields.len() =>
                {
                    Ok(())
                }
                Type::Class { name, .. } if name == "list" => Ok(()),
                _ => Err(TypeError {
                    message: format!("cannot tuple-destructure {:?}", subject_type),
                    span,
                }),
            },
            Pattern::Star(_, _) => Ok(()),
            Pattern::RecordDestructure(fields, _) => match subject_type {
                Type::Record {
                    fields: declared, ..
                } => {
                    for (name, _) in fields {
                        if let Some(name) = name {
                            if !declared
                                .iter()
                                .any(|(field, _)| field.as_deref() == Some(name))
                            {
                                return Err(TypeError {
                                    message: format!("record has no field '{name}'"),
                                    span,
                                });
                            }
                        }
                    }
                    Ok(())
                }
                _ => Err(TypeError {
                    message: format!("cannot record-destructure {:?}", subject_type),
                    span,
                }),
            },
            _ => Ok(()),
        }
    }

    fn anonymous_function_signature(
        &self,
        params: &[Param],
        return_type: Option<&TypeExpr>,
    ) -> Result<(Vec<Type>, Type, Type), TypeError> {
        let ptypes: Vec<Type> = params
            .iter()
            .map(|p| {
                p.type_annotation
                    .as_ref()
                    .map(|te| self.resolve_type_expr(te))
                    .transpose()
                    .and_then(|ty| {
                        ty.ok_or_else(|| TypeError {
                            message: format!(
                                "anonymous function parameter '{}' needs an annotation or an expected function type",
                                p.name
                            ),
                            span: p.span,
                        })
                    })
            })
            .collect::<Result<Vec<_>, TypeError>>()?;
        let declared_return = return_type
            .map(|te| self.resolve_type_expr(te))
            .transpose()?;
        let function_return = declared_return.clone().unwrap_or(Type::None);
        let body_return = declared_return.unwrap_or_else(|| Type::TypeVar("Any".into()));
        Ok((ptypes, function_return, body_return))
    }

    fn anonymous_function_type(
        &self,
        params: &[Param],
        return_type: Option<&TypeExpr>,
    ) -> Result<Type, TypeError> {
        let (params, return_type, _) = self.anonymous_function_signature(params, return_type)?;
        Ok(Type::Function {
            params,
            return_type: Box::new(return_type),
        })
    }

    fn anonymous_params_need_expected(params: &[Param]) -> bool {
        params
            .iter()
            .any(|param| param.type_annotation.is_none() && !param.is_gather)
    }

    fn is_argument_bundle_type(ty: &Type) -> bool {
        matches!(
            ty,
            Type::Class { name, .. }
                if name == "Arguments"
                    || name == "Parameters"
                    || name.ends_with("Arguments")
                    || name.ends_with("Parameters")
        )
    }

    fn anonymous_function_type_against_expected(
        &self,
        params: &[Param],
        return_type: Option<&TypeExpr>,
        body: &[Stmt],
        expected: &Type,
        span: Span,
    ) -> Result<Type, TypeError> {
        let Type::Function {
            params: expected_params,
            return_type: expected_return,
        } = expected
        else {
            return Err(TypeError {
                message: "anonymous function needs an expected function type".into(),
                span,
            });
        };
        if params.len() != expected_params.len() {
            return Err(TypeError {
                message: format!(
                    "anonymous function expected {} parameter(s), got {}",
                    expected_params.len(),
                    params.len()
                ),
                span,
            });
        }

        let mut parameter_types = Vec::with_capacity(params.len());
        for (param, expected_type) in params.iter().zip(expected_params.iter()) {
            let parameter_type = param
                .type_annotation
                .as_ref()
                .map(|annotation| self.resolve_type_expr(annotation))
                .transpose()?
                .unwrap_or_else(|| expected_type.clone());
            if !parameter_type.is_subtype_of(expected_type, &self.env)
                && !expected_type.is_subtype_of(&parameter_type, &self.env)
            {
                return Err(TypeError {
                    message: format!(
                        "anonymous function parameter '{}' has incompatible type",
                        param.name
                    ),
                    span: param.span,
                });
            }
            parameter_types.push(parameter_type);
        }

        let declared_return = return_type
            .map(|annotation| self.resolve_type_expr(annotation))
            .transpose()?;
        let function_return = declared_return
            .clone()
            .unwrap_or_else(|| expected_return.as_ref().clone());
        if !function_return.is_subtype_of(expected_return, &self.env)
            && !expected_return.is_subtype_of(&function_return, &self.env)
        {
            return Err(TypeError {
                message: "anonymous function return type is incompatible with expected function type"
                    .into(),
                span,
            });
        }

        let mut closure_checker = self.clone();
        closure_checker.env.current_return_type = Some(function_return.clone());
        for (param, parameter_type) in params.iter().zip(parameter_types.iter()) {
            closure_checker.env.variables.insert(
                param.name.clone(),
                (parameter_type.clone(), MutabilityView::Mutable),
            );
        }
        for statement in body {
            closure_checker.check_statement(statement)?;
        }

        Ok(Type::Function {
            params: parameter_types,
            return_type: Box::new(function_return),
        })
    }

    fn dict_literal_type_against_record(
        &self,
        entries: &[(Expr, Expr)],
        expected: &Type,
        span: Span,
    ) -> Result<Type, TypeError> {
        let Type::Record {
            fields,
            is_open,
        } = expected
        else {
            return self.type_of_expr(&Expr::Dict {
                entries: entries.to_vec(),
                span,
            });
        };
        let mut expected_fields = HashMap::new();
        for (position, (name, ty)) in fields.iter().enumerate() {
            if let Some(name) = name {
                expected_fields.insert(name.clone(), ty);
            } else {
                expected_fields.insert(position.to_string(), ty);
            }
        }

        let mut seen = HashSet::new();
        for (key, value) in entries {
            let field_name = match key {
                Expr::Literal {
                    value: LiteralValue::Str(name),
                    ..
                } => name.clone(),
                Expr::Literal {
                    value: LiteralValue::Int(position),
                    ..
                } => position.to_string(),
                _ => {
                    return Err(TypeError {
                        message: "record literal keys must be literal field names".into(),
                        span: key.span(),
                    });
                }
            };
            let Some(field_type) = expected_fields.get(&field_name) else {
                if *is_open {
                    self.type_of_expr(value)?;
                    continue;
                } else {
                    return Err(TypeError {
                        message: format!("record has no field '{field_name}'"),
                        span: key.span(),
                    });
                }
            };
            if !seen.insert(field_name.clone()) {
                return Err(TypeError {
                    message: format!("duplicate record field '{field_name}'"),
                    span: key.span(),
                });
            }
            let value_type = self.type_of_expr(value)?;
            if !value_type.is_subtype_of(field_type, &self.env) {
                return Err(TypeError {
                    message: format!(
                        "record field '{}' expects {:?}, got {:?}",
                        field_name, field_type, value_type
                    ),
                    span: value.span(),
                });
            }
        }

        for field_name in expected_fields.keys() {
            if !seen.contains(field_name) {
                return Err(TypeError {
                    message: format!("record field '{field_name}' is missing"),
                    span,
                });
            }
        }

        Ok(expected.clone())
    }

    fn argument_type_against_parameter(&self, argument: &Arg, parameter: &Type) -> Result<Type, TypeError> {
        if let Expr::AnonymousDef {
            params,
            return_type,
            body,
            span,
        } = &argument.value
        {
            if Self::anonymous_params_need_expected(params) {
                self.anonymous_function_type_against_expected(
                    params,
                    return_type.as_ref(),
                    body,
                    parameter,
                    *span,
                )
            } else {
                self.type_of_expr(&argument.value)
            }
        } else {
            self.type_of_expr(&argument.value)
        }
    }

    fn dispatch_return_for_types(
        &self,
        name: &str,
        argument_types: &[Type],
        span: Span,
    ) -> Result<Option<Type>, TypeError> {
        if !self.env.overloaded_functions.contains(name) {
            return Ok(None);
        }
        let candidates = self
            .env
            .function_overloads
            .get(name)
            .into_iter()
            .flatten()
            .filter_map(|candidate| match candidate {
                Type::Function {
                    params,
                    return_type,
                } if params.len() == argument_types.len()
                    && params
                        .iter()
                        .zip(argument_types.iter())
                        .all(|(parameter, argument)| argument.is_subtype_of(parameter, &self.env)) =>
                {
                    Some((params.clone(), return_type.as_ref().clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        if candidates.is_empty() {
            return Ok(None);
        }
        let maximal = candidates
            .iter()
            .filter(|(params, _)| {
                !candidates.iter().any(|(other, _)| {
                    other != params
                        && other
                            .iter()
                            .zip(params.iter())
                            .all(|(a, b)| a.is_subtype_of(b, &self.env))
                        && other.iter().zip(params.iter()).any(|(a, b)| a != b)
                })
            })
            .collect::<Vec<_>>();
        if maximal.len() != 1 {
            return Err(TypeError {
                message: format!(
                    "ambiguous dispatch call to '{}' ({} applicable overloads)",
                    name,
                    maximal.len()
                ),
                span,
            });
        }
        Ok(maximal.first().map(|(_, return_type)| return_type.clone()))
    }

    fn binary_dispatch_name(op: BinaryOp) -> Option<&'static str> {
        match op {
            BinaryOp::Add => Some("__add__"),
            BinaryOp::Sub => Some("__sub__"),
            BinaryOp::Mul => Some("__mul__"),
            BinaryOp::Div => Some("__truediv__"),
            BinaryOp::FloorDiv => Some("__floordiv__"),
            BinaryOp::Mod => Some("__mod__"),
            BinaryOp::Pow => Some("__pow__"),
            BinaryOp::Lt => Some("__lt__"),
            BinaryOp::LtEq => Some("__le__"),
            BinaryOp::Gt => Some("__gt__"),
            BinaryOp::GtEq => Some("__ge__"),
            BinaryOp::Eq => Some("__eq__"),
            BinaryOp::NotEq => Some("__ne__"),
            _ => None,
        }
    }

    fn exact_class_of_expr(&self, expr: &Expr) -> Option<String> {
        match expr {
            Expr::Ident { name, .. } => self.env.exact_variables.get(name).cloned(),
            Expr::Call { func, .. } => match &**func {
                Expr::Ident { name, .. }
                    if self.env.classes.contains_key(name)
                        && self.env.class_constructor_arity.contains_key(name) =>
                {
                    Some(name.clone())
                }
                _ => None,
            },
            _ => None,
        }
    }

    fn exact_class_may_satisfy(&self, class_name: &str, target: &Type) -> bool {
        self.env
            .classes
            .get(class_name)
            .is_some_and(|class_type| class_type.is_subtype_of(target, &self.env))
    }

    pub fn type_of_expr(&self, expr: &Expr) -> Result<Type, TypeError> {
        match expr {
            Expr::Literal { value, .. } => Ok(match value {
                LiteralValue::Int(_) => Type::Int,
                LiteralValue::BigInt(_) => Type::Int,
                LiteralValue::Float(_) => Type::Float,
                LiteralValue::Complex(_) => self
                    .env
                    .classes
                    .get("complex")
                    .cloned()
                    .unwrap_or(Type::Float),
                LiteralValue::Bool(value) => Type::LiteralBool(*value),
                LiteralValue::Str(_) => Type::Str,
                LiteralValue::Bytes(_) => Type::Class {
                    name: "Bytes".into(),
                    type_args: Vec::new(),
                    parent: None,
                    traits: Vec::new(),
                    interfaces: vec!["Buffer".into(), "Sized".into(), "Container".into()],
                    fields: HashMap::new(),
                    is_sealed: true,
                },
                LiteralValue::None => Type::None,
                LiteralValue::Sentinel(s) => Type::TypeVar(s.clone()),
                LiteralValue::Ellipsis => Type::None,
            }),
            Expr::Ident { name, span } => {
                if name == "_" {
                    Err(TypeError {
                        message: "'_' is a black-hole assignment target, not a readable value"
                            .into(),
                        span: *span,
                    })
                } else if let Some((t, _)) = self.env.variables.get(name) {
                    Ok(t.clone())
                } else if let Some(t) = self.env.classes.get(name) {
                    Ok(t.clone())
                } else if name == "super" {
                    // `super` is resolved against the enclosing class at runtime.
                    // Keep it as an opaque receiver here so attribute lookup and
                    // calls remain type-checkable without inventing a second
                    // class hierarchy in the expression checker.
                    Ok(Type::TypeVar("super".to_string()))
                } else if let Some(message) = Self::removed_builtin_message(name) {
                    Err(TypeError {
                        message: message.into(),
                        span: *span,
                    })
                } else {
                    Err(TypeError {
                        message: format!("undefined variable '{name}'"),
                        span: *span,
                    })
                }
            }
            Expr::Binary {
                op,
                left,
                right,
                span: _,
            } => {
                let lt = match self.type_of_expr(left)? {
                    Type::LiteralStr(_) => Type::Str,
                    Type::LiteralFloat(_) => Type::Float,
                    Type::LiteralBool(_) => Type::Bool,
                    other => other,
                };
                let rt = match self.type_of_expr(right)? {
                    Type::LiteralStr(_) => Type::Str,
                    Type::LiteralFloat(_) => Type::Float,
                    Type::LiteralBool(_) => Type::Bool,
                    other => other,
                };

                if matches!(op, BinaryOp::Eq | BinaryOp::NotEq)
                    && matches!(&lt, Type::Class { .. })
                    && !matches!(&lt, Type::Class { name, .. } if name == "complex")
                    && !lt.is_subtype_of(
                        &Type::Trait {
                            name: "Eq".into(),
                            type_args: Vec::new(),
                            methods: HashSet::new(),
                        },
                        &self.env,
                    )
                    && !matches!(&lt, Type::Class { name, .. } if self.env.class_members.get(name).is_some_and(|members| members.contains("__eq__")))
                {
                    return Err(TypeError { message: "comparison requires Eq; declare without Eq only with an explicit replacement".into(), span: left.span() });
                }
                if matches!(op, BinaryOp::Eq | BinaryOp::NotEq)
                    && matches!(&rt, Type::Class { .. })
                    && !rt.is_subtype_of(
                        &Type::Trait {
                            name: "Eq".into(),
                            type_args: Vec::new(),
                            methods: HashSet::new(),
                        },
                        &self.env,
                    )
                    && !matches!(&rt, Type::Class { name, .. } if self.env.class_members.get(name).is_some_and(|members| members.contains("__eq__")))
                {
                    return Err(TypeError {
                        message: "comparison requires Eq on both operands".into(),
                        span: right.span(),
                    });
                }
                if matches!(
                    op,
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq
                ) && matches!(&lt, Type::Class { .. })
                    && !matches!(&lt, Type::Class { name, .. } if name == "complex")
                    && !lt.is_subtype_of(
                        &Type::Trait {
                            name: "Ord".into(),
                            type_args: Vec::new(),
                            methods: HashSet::new(),
                        },
                        &self.env,
                    )
                    && !matches!(&lt, Type::Class { name, .. } if self.env.class_members.get(name).is_some_and(|members| members.contains("__lt__")))
                {
                    return Err(TypeError { message: "ordering requires Ord; declare without Ord only with an explicit replacement".into(), span: left.span() });
                }
                if matches!(
                    op,
                    BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq
                ) && matches!(&rt, Type::Class { .. })
                    && !matches!(&rt, Type::Class { name, .. } if name == "complex")
                    && !rt.is_subtype_of(
                        &Type::Trait {
                            name: "Ord".into(),
                            type_args: Vec::new(),
                            methods: HashSet::new(),
                        },
                        &self.env,
                    )
                    && !matches!(&rt, Type::Class { name, .. } if self.env.class_members.get(name).is_some_and(|members| members.contains("__lt__")))
                {
                    return Err(TypeError {
                        message: "ordering requires Ord on both operands".into(),
                        span: right.span(),
                    });
                }

                let is_numeric = |ty: &Type| {
                    matches!(ty, Type::Int | Type::LiteralInt(_) | Type::Float)
                        || matches!(ty, Type::Class { name, .. } if name == "complex")
                        || matches!(ty, Type::Never)
                        || matches!(ty, Type::TypeVar(name) if name == "Any")
                };
                if let Some(dispatch_name) = Self::binary_dispatch_name(op.clone()) {
                    if let Some(return_type) =
                        self.dispatch_return_for_types(dispatch_name, &[lt.clone(), rt.clone()], left.span())?
                    {
                        return Ok(return_type);
                    }
                }
                match op {
                    BinaryOp::Add | BinaryOp::Sub | BinaryOp::Mul => {
                        let unknown = |ty: &Type| {
                            matches!(ty, Type::Never)
                                || matches!(ty, Type::TypeVar(name) if name == "Any")
                        };
                        let complex = matches!(&lt, Type::Class { name, .. } if name == "complex")
                            || matches!(&rt, Type::Class { name, .. } if name == "complex");
                        if unknown(&lt) || unknown(&rt) {
                            Ok(Type::TypeVar("Any".into()))
                        } else if matches!(op, BinaryOp::Add) && lt == Type::Str && rt == Type::Str
                        {
                            Ok(Type::Str)
                        } else if !is_numeric(&lt) || !is_numeric(&rt) {
                            Err(TypeError {
                                message: format!(
                                    "unsupported operands for {:?}: {:?} and {:?}",
                                    op, lt, rt
                                ),
                                span: left.span(),
                            })
                        } else if complex {
                            Ok(self
                                .env
                                .classes
                                .get("complex")
                                .cloned()
                                .unwrap_or(Type::Float))
                        } else if lt == Type::Int && rt == Type::Int {
                            Ok(Type::Int)
                        } else if lt == Type::Float || rt == Type::Float {
                            Ok(Type::Float)
                        } else {
                            Err(TypeError {
                                message: format!(
                                    "unsupported operands for {:?}: {:?} and {:?}",
                                    op, lt, rt
                                ),
                                span: left.span(),
                            })
                        }
                    }
                    BinaryOp::Div => {
                        if !is_numeric(&lt) || !is_numeric(&rt) {
                            return Err(TypeError {
                                message: format!(
                                    "unsupported operands for /: {:?} and {:?}",
                                    lt, rt
                                ),
                                span: left.span(),
                            });
                        }
                        if matches!(&lt, Type::Class { name, .. } if name == "complex")
                            || matches!(&rt, Type::Class { name, .. } if name == "complex")
                        {
                            Ok(self
                                .env
                                .classes
                                .get("complex")
                                .cloned()
                                .unwrap_or(Type::Float))
                        } else {
                            Ok(Type::Float)
                        }
                    }
                    BinaryOp::FloorDiv => {
                        if lt == Type::Int && rt == Type::Int {
                            Ok(Type::Int)
                        } else if matches!(
                            (&lt, &rt),
                            (Type::Int | Type::Float, Type::Int | Type::Float)
                        ) {
                            Ok(Type::Float)
                        } else {
                            Err(TypeError {
                                message: format!(
                                    "unsupported operands for //: {:?} and {:?}",
                                    lt, rt
                                ),
                                span: left.span(),
                            })
                        }
                    }
                    BinaryOp::Mod => {
                        if lt == Type::Int && rt == Type::Int {
                            Ok(Type::Int)
                        } else if matches!(
                            (&lt, &rt),
                            (Type::Int | Type::Float, Type::Int | Type::Float)
                        ) {
                            Ok(Type::Float)
                        } else {
                            Err(TypeError {
                                message: format!(
                                    "unsupported operands for %: {:?} and {:?}",
                                    lt, rt
                                ),
                                span: left.span(),
                            })
                        }
                    }
                    BinaryOp::Pow => {
                        if !is_numeric(&lt) || !is_numeric(&rt) {
                            return Err(TypeError {
                                message: format!(
                                    "unsupported operands for **: {:?} and {:?}",
                                    lt, rt
                                ),
                                span: left.span(),
                            });
                        }
                        if matches!(&lt, Type::Class { name, .. } if name == "complex")
                            || matches!(&rt, Type::Class { name, .. } if name == "complex")
                        {
                            Ok(self
                                .env
                                .classes
                                .get("complex")
                                .cloned()
                                .unwrap_or(Type::Float))
                        } else if lt == Type::Float || rt == Type::Float {
                            Ok(Type::Float)
                        } else {
                            Ok(Type::Int)
                        }
                    }
                    BinaryOp::Eq
                    | BinaryOp::NotEq
                    | BinaryOp::Lt
                    | BinaryOp::LtEq
                    | BinaryOp::Gt
                    | BinaryOp::GtEq => {
                        if (matches!(&lt, Type::Class { name, .. } if name == "complex")
                            || matches!(&rt, Type::Class { name, .. } if name == "complex"))
                            && matches!(
                                op,
                                BinaryOp::Lt | BinaryOp::LtEq | BinaryOp::Gt | BinaryOp::GtEq
                            )
                        {
                            return Err(TypeError {
                                message: "complex values are not orderable".into(),
                                span: Span::default(),
                            });
                        }
                        Ok(Type::Bool)
                    }
                    BinaryOp::In | BinaryOp::NotIn => {
                        let membership_target = match &rt {
                            Type::View { inner, .. } => inner.as_ref(),
                            other => other,
                        };
                        let supported = match membership_target {
                            Type::Class {
                                name, type_args, ..
                            } if name == "list" || name == "set" => {
                                if let Some(element_type) = type_args.first() {
                                    if !lt.is_subtype_of(element_type, &self.env) {
                                        return Err(TypeError {
                                            message: format!(
                                                "membership value has type {:?}, expected {:?}",
                                                lt, element_type
                                            ),
                                            span: left.span(),
                                        });
                                    }
                                }
                                true
                            }
                            Type::Class {
                                name, type_args, ..
                            } if name == "dict" => {
                                if let Some(key_type) = type_args.first() {
                                    if !lt.is_subtype_of(key_type, &self.env) {
                                        return Err(TypeError {
                                            message: format!(
                                                "dictionary membership key has type {:?}, expected {:?}",
                                                lt, key_type
                                            ),
                                            span: left.span(),
                                        });
                                    }
                                }
                                true
                            }
                            Type::Trait {
                                name, type_args, ..
                            } if matches!(name.as_str(), "Container" | "Collection" | "Sequence") =>
                            {
                                if let Some(element_type) = type_args.first() {
                                    if !lt.is_subtype_of(element_type, &self.env) {
                                        return Err(TypeError {
                                            message: format!(
                                                "membership value has type {:?}, expected {:?}",
                                                lt, element_type
                                            ),
                                            span: left.span(),
                                        });
                                    }
                                }
                                true
                            }
                            Type::Str => {
                                if lt != Type::Str {
                                    return Err(TypeError {
                                        message: format!(
                                            "string membership requires str, got {:?}",
                                            lt
                                        ),
                                        span: left.span(),
                                    });
                                }
                                true
                            }
                            Type::Class { name, .. } if name == "str" => {
                                if lt != Type::Str {
                                    return Err(TypeError {
                                        message: format!(
                                            "string membership requires str, got {:?}",
                                            lt
                                        ),
                                        span: left.span(),
                                    });
                                }
                                true
                            }
                            Type::Class { name, .. } if name == "range" => {
                                if !lt.is_subtype_of(&Type::Int, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "range membership requires int, got {:?}",
                                            lt
                                        ),
                                        span: left.span(),
                                    });
                                }
                                true
                            }
                            Type::Shape(_) => {
                                if !lt.is_subtype_of(&Type::Int, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "shape membership requires int, got {:?}",
                                            lt
                                        ),
                                        span: left.span(),
                                    });
                                }
                                true
                            }
                            Type::Class { name, .. } => self
                                .env
                                .class_members
                                .get(name)
                                .is_some_and(|members| members.contains("__contains__")),
                            _ => false,
                        };
                        if !supported {
                            return Err(TypeError {
                                message: format!("'in' is not supported for type {:?}", rt),
                                span: right.span(),
                            });
                        }
                        Ok(Type::Bool)
                    }
                    BinaryOp::Is | BinaryOp::IsNot | BinaryOp::Identity | BinaryOp::NotIdentity => {
                        let declaration_kind_check =
                            matches!(&**right, Expr::Ident { name, .. } if matches!(name.as_str(), "class" | "trait" | "interface"))
                                || matches!(&**right, Expr::Type(TypeExpr::Named { name, .. }) if matches!(name.as_str(), "class" | "trait" | "interface"));
                        let tested_type = if let Expr::Ident { name, .. } = &**right {
                            match name.as_str() {
                                "int" => Type::Int,
                                "float" => Type::Float,
                                "bool" => Type::Bool,
                                "str" => Type::Str,
                                "none" => Type::None,
                                _ => rt.clone(),
                            }
                        } else {
                            rt.clone()
                        };
                        if matches!(op, BinaryOp::Is) && !declaration_kind_check {
                            if let Some(exact_class) = self.exact_class_of_expr(left) {
                                if !self.exact_class_may_satisfy(&exact_class, &tested_type) {
                                    return Err(TypeError {
                                        message: format!(
                                            "instance check between exact class '{exact_class}' and {:?} can never succeed",
                                            tested_type
                                        ),
                                        span: right.span(),
                                    });
                                }
                            }
                        }
                        if matches!(op, BinaryOp::Is)
                            && !declaration_kind_check
                            && !types_may_overlap(&lt, &tested_type, &self.env)
                        {
                            return Err(TypeError {
                                message: format!(
                                    "instance check between {:?} and {:?} can never succeed",
                                    lt, tested_type
                                ),
                                span: right.span(),
                            });
                        }
                        Ok(Type::Bool)
                    }
                    BinaryOp::And | BinaryOp::Or => Ok(Type::Bool),
                    BinaryOp::BitAnd
                    | BinaryOp::BitOr
                    | BinaryOp::BitXor
                    | BinaryOp::Shl
                    | BinaryOp::Shr => {
                        if lt == Type::Int && rt == Type::Int {
                            Ok(Type::Int)
                        } else {
                            Err(TypeError {
                                message: format!(
                                    "unsupported operands for {:?}: {:?} and {:?}",
                                    op, lt, rt
                                ),
                                span: left.span(),
                            })
                        }
                    }
                }
            }
            Expr::Unary { op, expr, .. } => {
                let inner = self.type_of_expr(expr)?;
                match op {
                    UnaryOp::Not => Ok(Type::Bool),
                    UnaryOp::Pos | UnaryOp::Neg => {
                        let numeric =
                            matches!(&inner, Type::Int | Type::LiteralInt(_) | Type::Float)
                                || matches!(&inner, Type::Class { name, .. } if name == "complex")
                                || matches!(&inner, Type::TypeVar(name) if name == "Any");
                        if numeric {
                            Ok(inner)
                        } else {
                            Err(TypeError {
                                message: format!("unsupported operand for {:?}: {:?}", op, inner),
                                span: expr.span(),
                            })
                        }
                    }
                    UnaryOp::Invert => {
                        if matches!(&inner, Type::Int | Type::LiteralInt(_))
                            || matches!(&inner, Type::TypeVar(name) if name == "Any")
                        {
                            Ok(inner)
                        } else {
                            Err(TypeError {
                                message: format!("unsupported operand for ~: {:?}", inner),
                                span: expr.span(),
                            })
                        }
                    }
                    UnaryOp::Spread | UnaryOp::GatherSpread => Ok(inner),
                }
            }
            Expr::Await { expr, .. } => {
                let awaited = self.type_of_expr(expr)?;
                match awaited {
                    Type::Future(inner) => Ok(*inner),
                    Type::TypeVar(ref name) if name == "Any" => Ok(awaited),
                    Type::Never => Ok(Type::Never),
                    other => Err(TypeError {
                        message: format!("cannot await value of type {:?}; expected Future", other),
                        span: expr.span(),
                    }),
                }
            }
            Expr::Call { func, args, .. } => {
                Self::check_call_argument_order(args)?;
                if matches!(&**func, Expr::Type(_)) {
                    return Err(TypeError {
                        message: "type() is not supported; use class[X] for type annotations and `is` for instance checks".into(),
                        span: func.span(),
                    });
                }
                if let Expr::Ident { name, .. } = &**func {
                    if name == "type" {
                        return Err(TypeError {
                            message: "type() is not supported; use class[X] for type annotations and `is` for instance checks".into(),
                            span: func.span(),
                        });
                    }
                    if let Some(message) = Self::removed_builtin_message(name) {
                        return Err(TypeError {
                            message: message.into(),
                            span: func.span(),
                        });
                    }
                    if name == "print" {
                        for argument in args {
                            if matches!(argument.value, Expr::Skip(_))
                                || matches!(&argument.value, Expr::Ident { name, .. } if name == "_")
                            {
                                continue;
                            }
                            self.type_of_expr(&argument.value)?;
                        }
                    }
                    if name == "round" && args.len() == 2 {
                        let digits = self.type_of_expr(&args[1].value)?;
                        if !digits.is_subtype_of(&Type::Int, &self.env) {
                            return Err(TypeError {
                                message: format!("round() ndigits must be int, got {:?}", digits),
                                span: args[1].value.span(),
                            });
                        }
                    }
                    if name == "sum" {
                        if let Some(argument) = args.first() {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            if !self.is_iterable_type(&argument_type) {
                                return Err(TypeError {
                                    message: format!(
                                        "sum() argument must be iterable, got {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                            let element_type = self.iterable_element_type(&argument_type);
                            let numeric_element = matches!(
                                element_type,
                                Type::Int | Type::LiteralInt(_) | Type::Float
                            ) || matches!(
                                &element_type,
                                Type::Class { name, .. } if name == "complex"
                            );
                            if !numeric_element {
                                return Err(TypeError {
                                    message: format!(
                                        "sum() elements must be numeric, got {:?}",
                                        element_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                        if let Some(argument) = args.get(1) {
                            let start_type = self.type_of_expr(&argument.value)?;
                            let numeric =
                                matches!(start_type, Type::Int | Type::LiteralInt(_) | Type::Float)
                                    || matches!(
                                        &start_type,
                                        Type::Class { name, .. } if name == "complex"
                                    );
                            if !numeric {
                                return Err(TypeError {
                                    message: format!(
                                        "sum() start must be numeric, got {:?}",
                                        start_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if name == "abs" {
                        if let Some(argument) = args.first() {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            let numeric = matches!(
                                argument_type,
                                Type::Int | Type::LiteralInt(_) | Type::Float
                            ) || matches!(
                                &argument_type,
                                Type::Class { name, .. } if name == "complex"
                            );
                            if !numeric {
                                return Err(TypeError {
                                    message: format!(
                                        "abs() argument must be numeric, got {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if name == "pow" {
                        for argument in args.iter().take(2) {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            let numeric = matches!(
                                argument_type,
                                Type::Int | Type::LiteralInt(_) | Type::Float
                            ) || matches!(
                                &argument_type,
                                Type::Class { name, .. } if name == "complex"
                            );
                            if !numeric {
                                return Err(TypeError {
                                    message: format!(
                                        "pow() arguments must be numeric, got {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                        if let Some(argument) = args.get(2) {
                            let modulus_type = self.type_of_expr(&argument.value)?;
                            if !modulus_type.is_subtype_of(&Type::Int, &self.env) {
                                return Err(TypeError {
                                    message: format!(
                                        "three-argument pow() modulus must be int, got {:?}",
                                        modulus_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if name == "complex" {
                        for argument in args {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            let numeric = matches!(
                                argument_type,
                                Type::Int | Type::LiteralInt(_) | Type::Float
                            );
                            if !numeric {
                                return Err(TypeError {
                                    message: format!(
                                        "complex() arguments must be numeric, got {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if name == "len" {
                        if let Some(argument) = args.first() {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            let sized = matches!(&argument_type, Type::Str | Type::Shape(_))
                                || matches!(&argument_type, Type::Class { name, .. }
                                    if matches!(name.as_str(), "list" | "set" | "dict" | "range" | "str")
                                        || self.env.class_members.get(name).is_some_and(|members| members.contains("__len__")));
                            if !sized {
                                return Err(TypeError {
                                    message: format!(
                                        "len() argument is not sized: {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if name == "range" {
                        for argument in args {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            if !argument_type.is_subtype_of(&Type::Int, &self.env) {
                                return Err(TypeError {
                                    message: format!(
                                        "range() arguments must be int, got {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                        if let Some(Expr::Literal {
                            value: LiteralValue::Int(step),
                            ..
                        }) = args.get(2).map(|argument| &argument.value)
                        {
                            if *step == 0 {
                                return Err(TypeError {
                                    message: "range() step cannot be zero".into(),
                                    span: args[2].value.span(),
                                });
                            }
                        }
                    }
                    if name == "slice" {
                        for argument in args {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            if !argument_type.is_subtype_of(&Type::Int, &self.env)
                                && argument_type != Type::None
                            {
                                return Err(TypeError {
                                    message: format!(
                                        "slice bounds must be int or none, got {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if matches!(name.as_str(), "list" | "set") {
                        if let Some(argument) = args.first() {
                            if !argument.is_spread
                                && !argument.is_dict_spread
                                && !argument.is_gather_spread
                            {
                                let argument_type = self.type_of_expr(&argument.value)?;
                                if !self.is_iterable_type(&argument_type) {
                                    return Err(TypeError {
                                        message: format!(
                                            "{}() argument must be iterable, got {:?}",
                                            name, argument_type
                                        ),
                                        span: argument.value.span(),
                                    });
                                }
                            }
                        }
                    }
                    if name == "dict" {
                        if let Some(argument) = args.first() {
                            if !argument.is_spread
                                && !argument.is_dict_spread
                                && !argument.is_gather_spread
                            {
                                let argument_type = self.type_of_expr(&argument.value)?;
                                let mapping = matches!(
                                    &argument_type,
                                    Type::Class { name, type_args, .. }
                                        if name == "dict" && type_args.len() == 2
                                );
                                if !mapping && !self.is_iterable_type(&argument_type) {
                                    return Err(TypeError {
                                        message: format!(
                                            "dict() argument must be a mapping or iterable, got {:?}",
                                            argument_type
                                        ),
                                        span: argument.value.span(),
                                    });
                                }
                            }
                        }
                    }
                    if matches!(name.as_str(), "enumerate" | "iter" | "any" | "all") {
                        if let Some(argument) = args.first() {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            if !self.is_iterable_type(&argument_type) {
                                return Err(TypeError {
                                    message: format!(
                                        "{}() argument must be iterable, got {:?}",
                                        name, argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if name == "reversed" {
                        if let Some(argument) = args.first() {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            if !self.is_reversible_type(&argument_type) {
                                return Err(TypeError {
                                    message: format!(
                                        "reversed() argument is not reversible, got {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if name == "map" {
                        // Let the shared arity checker report missing arguments first.
                        if args.len() >= 2 {
                            if let Some(argument) = args.first() {
                                let mapper_type = self.type_of_expr(&argument.value)?;
                                let callable = matches!(&mapper_type, Type::Function { .. })
                                    || matches!(&mapper_type, Type::TypeVar(name) if name == "Any")
                                    || mapper_type.is_subtype_of(
                                        &Type::Trait {
                                            name: "Callable".into(),
                                            type_args: Vec::new(),
                                            methods: HashSet::new(),
                                        },
                                        &self.env,
                                    );
                                if !callable {
                                    return Err(TypeError {
                                        message: format!(
                                            "map() first argument must be callable, got {:?}",
                                            mapper_type
                                        ),
                                        span: argument.value.span(),
                                    });
                                }
                            }
                            if let Some(argument) = args.get(1) {
                                let argument_type = self.type_of_expr(&argument.value)?;
                                if !self.is_iterable_type(&argument_type) {
                                    return Err(TypeError {
                                        message: format!(
                                            "map() iterable argument must be iterable, got {:?}",
                                            argument_type
                                        ),
                                        span: argument.value.span(),
                                    });
                                }
                            }
                        }
                    }
                    if name == "enumerate" {
                        if let Some(argument) = args.get(1) {
                            let start_type = self.type_of_expr(&argument.value)?;
                            if !start_type.is_subtype_of(&Type::Int, &self.env) {
                                return Err(TypeError {
                                    message: format!(
                                        "enumerate() start must be int, got {:?}",
                                        start_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if matches!(name.as_str(), "min" | "max") && args.len() == 1 {
                        let argument_type = self.type_of_expr(&args[0].value)?;
                        if !self.is_iterable_type(&argument_type) {
                            return Err(TypeError {
                                message: format!(
                                    "{}() argument must be iterable, got {:?}",
                                    name, argument_type
                                ),
                                span: args[0].value.span(),
                            });
                        }
                    }
                    if name == "sorted" {
                        if let Some(argument) = args.first() {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            if !self.is_iterable_type(&argument_type) {
                                return Err(TypeError {
                                    message: format!(
                                        "sorted() argument must be iterable, got {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if name == "zip" {
                        for argument in args {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            if !self.is_iterable_type(&argument_type) {
                                return Err(TypeError {
                                    message: format!(
                                        "zip() arguments must be iterable, got {:?}",
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    if matches!(name.as_str(), "bytes" | "bytearray" | "memoryview") {
                        if let Some(argument) = args.first() {
                            let argument_type = self.type_of_expr(&argument.value)?;
                            let accepted = if name == "memoryview" {
                                self.is_buffer_type(&argument_type)
                            } else {
                                self.is_byte_conversion_input(&argument_type)
                            };
                            if !accepted {
                                return Err(TypeError {
                                    message: format!(
                                        "{}() argument must be a buffer{}got {:?}",
                                        name,
                                        if name == "memoryview" {
                                            ", "
                                        } else {
                                            ", string, or integer list, "
                                        },
                                        argument_type
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                }
                if let Expr::Attribute { value, attr, .. } = &**func {
                    let receiver_type = self.type_of_expr(value)?;
                    if let Type::View { mutability, inner } = &receiver_type {
                        let is_mutating_collection_method = matches!(
                            inner.as_ref(),
                            Type::Class { name, .. }
                                if matches!(name.as_str(), "list" | "set" | "dict")
                        ) && matches!(
                            attr.as_str(),
                            "append"
                                | "extend"
                                | "insert"
                                | "remove"
                                | "discard"
                                | "add"
                                | "clear"
                                | "pop"
                        );
                        if is_mutating_collection_method
                            && matches!(
                                mutability,
                                MutabilityView::ReadOnly | MutabilityView::Immutable
                            )
                        {
                            return Err(TypeError {
                                message: format!(
                                    "cannot call mutating method '{attr}' on read-only/immutable view"
                                ),
                                span: func.span(),
                            });
                        }
                    }
                    let is_string = matches!(&receiver_type, Type::Str)
                        || matches!(&receiver_type, Type::Class { name, .. } if name == "str");
                    if is_string {
                        let valid = match attr.as_str() {
                            "split" => args.len() <= 1,
                            "join" | "startswith" | "endswith" => args.len() == 1,
                            "replace" => args.len() == 2,
                            "upper" | "lower" | "strip" | "trim" | "lstrip" | "rstrip" => {
                                args.is_empty()
                            }
                            "bin" | "oct" | "hex" => args.len() == 1,
                            _ => true,
                        };
                        if !valid {
                            return Err(TypeError {
                                message: format!("invalid argument count for str.{attr}()"),
                                span: func.span(),
                            });
                        }
                        let expected = match attr.as_str() {
                            "split" => args
                                .first()
                                .map(|_| Type::Str)
                                .into_iter()
                                .collect::<Vec<_>>(),
                            "replace" => vec![Type::Str, Type::Str],
                            "startswith" | "endswith" => vec![Type::Str],
                            "bin" | "oct" | "hex" => vec![Type::Int],
                            _ => Vec::new(),
                        };
                        for (argument, expected_type) in args.iter().zip(expected.iter()) {
                            let actual = self.type_of_expr(&argument.value)?;
                            if !actual.is_subtype_of(expected_type, &self.env) {
                                return Err(TypeError {
                                    message: format!(
                                        "str.{attr}() argument has incompatible type: expected {:?}, got {:?}",
                                        expected_type, actual
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                    }
                    let collection_bounds = match receiver_type {
                        Type::Class { ref name, .. } if name == "list" => match attr.as_str() {
                            "append" | "extend" | "remove" => Some((1, 1)),
                            "insert" => Some((2, 2)),
                            "clear" => Some((0, 0)),
                            "pop" => Some((0, 2)),
                            _ => None,
                        },
                        Type::Class { ref name, .. } if name == "dict" => match attr.as_str() {
                            "get" => Some((1, 2)),
                            "pop" => Some((1, 2)),
                            "keys" | "values" | "items" | "clear" => Some((0, 0)),
                            _ => None,
                        },
                        Type::Class { ref name, .. } if name == "set" => match attr.as_str() {
                            "add" | "remove" | "discard" => Some((1, 1)),
                            "clear" | "pop" => Some((0, 0)),
                            _ => None,
                        },
                        _ => None,
                    };
                    if let Some((minimum, maximum)) = collection_bounds {
                        if args.len() < minimum || args.len() > maximum {
                            return Err(TypeError {
                                message: format!("invalid argument count for {attr}()"),
                                span: func.span(),
                            });
                        }
                    }
                }
                let ft = self.type_of_expr(func)?;
                let called_name = match &**func {
                    Expr::Ident { name, .. } => Some(name.as_str()),
                    _ => None,
                };
                let called_member_params = if let Expr::Attribute { value, attr, .. } = &**func {
                    let receiver_type = self.type_of_expr(value)?;
                    match receiver_type {
                        Type::Class { name, .. } => self.class_method_params(&name, attr),
                        Type::Interface { name, .. } => self.interface_method_params(&name, attr),
                        Type::Trait { name, .. } => self.trait_method_params(&name, attr),
                        Type::View { inner, .. } => match *inner {
                            Type::Class { name, .. } => self.class_method_params(&name, attr),
                            Type::Interface { name, .. } => {
                                self.interface_method_params(&name, attr)
                            }
                            Type::Trait { name, .. } => self.trait_method_params(&name, attr),
                            _ => None,
                        },
                        _ => None,
                    }
                } else {
                    None
                };
                if let Expr::Ident { name, .. } = &**func {
                    if name == "hash" {
                        if let Some(arg) = args.first() {
                            let arg_type = self.type_of_expr(&arg.value)?;
                            if matches!(&arg_type, Type::Class { .. })
                                && !arg_type.is_subtype_of(
                                    &Type::Trait {
                                        name: "Hashable".into(),
                                        type_args: Vec::new(),
                                        methods: HashSet::new(),
                                    },
                                    &self.env,
                                )
                                && !matches!(&arg_type, Type::Class { name, .. } if self.env.class_members.get(name).is_some_and(|members| members.contains("__hash__")))
                            {
                                return Err(TypeError { message: "hash() requires Hashable; declare without Hashable only with an explicit __hash__ replacement".into(), span: arg.value.span() });
                            }
                        }
                    }
                    if let Some((required, maximum)) = self.env.function_arity.get(name).copied() {
                        // Spread bundles are checked when expanded by the
                        // call binder; their runtime cardinality is not known
                        // at this stage, so do not reject them based on the
                        // count of explicit arguments alone.
                        if !args.iter().any(|argument| {
                            argument.is_spread
                                || argument.is_dict_spread
                                || argument.is_gather_spread
                        }) {
                            let accepts_gather_bundle = matches!(
                                &ft,
                                Type::Function { params, .. }
                                    if params
                                        .last()
                                        .is_some_and(Self::is_argument_bundle_type)
                            );
                            if let Some(parameter_names) = self.env.function_param_names.get(name) {
                                let mut seen_named = HashSet::new();
                                let mut saw_named = false;
                                let mut bound = HashSet::new();
                                let mut positional_index = 0usize;
                                for argument in args {
                                    if let Some(argument_name) = &argument.name {
                                        saw_named = true;
                                        let known_parameter = parameter_names
                                            .iter()
                                            .any(|parameter_name| parameter_name == argument_name);
                                        if !known_parameter && !accepts_gather_bundle {
                                            return Err(TypeError {
                                                message: format!(
                                                    "function '{}' has no parameter named '{}'",
                                                    name, argument_name
                                                ),
                                                span: argument.value.span(),
                                            });
                                        }
                                        if !seen_named.insert(argument_name.clone()) {
                                            return Err(TypeError {
                                                message: format!(
                                                    "argument '{}' passed more than once",
                                                    argument_name
                                                ),
                                                span: argument.value.span(),
                                            });
                                        }
                                        if known_parameter {
                                            bound.insert(argument_name.clone());
                                        }
                                    } else if saw_named {
                                        return Err(TypeError {
                                            message: format!(
                                                "positional argument follows named argument in call to '{}'",
                                                name
                                            ),
                                            span: argument.value.span(),
                                        });
                                    } else {
                                        if let Some(parameter_name) =
                                            parameter_names.get(positional_index)
                                        {
                                            bound.insert(parameter_name.clone());
                                        }
                                        positional_index += 1;
                                    }
                                }
                                if !args.is_empty() {
                                    if let Some(required_names) =
                                        self.env.function_required_params.get(name)
                                    {
                                        if let Some(missing) = required_names
                                            .iter()
                                            .find(|required| !bound.contains(*required))
                                        {
                                            return Err(TypeError {
                                                message: format!(
                                                    "function '{}' is missing required argument '{}'",
                                                    name, missing
                                                ),
                                                span: args
                                                    .last()
                                                    .map(|argument| argument.value.span())
                                                    .unwrap_or_default(),
                                            });
                                        }
                                    }
                                }
                            }
                            let supplied = args.len();
                            if supplied < required {
                                return Err(TypeError {
                                    message: format!(
                                        "function '{}' requires at least {} argument(s), got {}",
                                        name, required, supplied
                                    ),
                                    span: args
                                        .last()
                                        .map(|argument| argument.value.span())
                                        .unwrap_or_default(),
                                });
                            }
                            if let Some(maximum) = maximum {
                                if supplied > maximum {
                                    return Err(TypeError {
                                        message: format!(
                                            "function '{}' accepts at most {} argument(s), got {}",
                                            name, maximum, supplied
                                        ),
                                        span: args
                                            .last()
                                            .map(|argument| argument.value.span())
                                            .unwrap_or_default(),
                                    });
                                }
                            }
                        }
                    }
                    if matches!(&ft, Type::Class { .. })
                        && self.env.class_constructor_arity.contains_key(name)
                        && !args.iter().any(|argument| {
                            argument.is_spread
                                || argument.is_dict_spread
                                || argument.is_gather_spread
                        })
                    {
                        let expected = self.class_constructor_field_count(name);
                        let required = self.class_constructor_required_count(name);
                        if args.len() < required || args.len() > expected {
                            return Err(TypeError {
                                message: format!(
                                    "constructor '{}' expects between {} and {} argument(s), got {}",
                                    name,
                                    required,
                                    expected,
                                    args.len()
                                ),
                                span: args
                                    .last()
                                    .map(|argument| argument.value.span())
                                    .unwrap_or_default(),
                            });
                        }
                        let field_names = self.class_constructor_field_names(name);
                        let mut bound = HashSet::new();
                        let mut positional_index = 0usize;
                        let mut saw_named = false;
                        for argument in args {
                            let index = if let Some(argument_name) = &argument.name {
                                saw_named = true;
                                let Some(index) = field_names
                                    .iter()
                                    .position(|field_name| field_name == argument_name)
                                else {
                                    return Err(TypeError {
                                        message: format!(
                                            "constructor '{}' has no field named '{}'",
                                            name, argument_name
                                        ),
                                        span: argument.value.span(),
                                    });
                                };
                                if !bound.insert(argument_name.clone()) {
                                    return Err(TypeError {
                                        message: format!(
                                            "constructor argument '{}' passed more than once",
                                            argument_name
                                        ),
                                        span: argument.value.span(),
                                    });
                                }
                                index
                            } else {
                                if saw_named {
                                    return Err(TypeError {
                                        message: format!(
                                            "positional argument follows named argument in constructor '{}'",
                                            name
                                        ),
                                        span: argument.value.span(),
                                    });
                                }
                                let index = positional_index;
                                positional_index += 1;
                                index
                            };
                            if let Some(field_name) = field_names.get(index) {
                                let field_type = self
                                    .class_field_type(name, field_name)
                                    .unwrap_or(Type::TypeVar("Any".into()));
                                let argument_type = self.type_of_expr(&argument.value)?;
                                // An uninstantiated constructor may infer its
                                // class type arguments from the supplied
                                // fields; defer checks against bare type
                                // variables until specialization.
                                let generic_field = self
                                    .env
                                    .class_type_params
                                    .get(name)
                                    .is_some_and(|params| {
                                        matches!(&field_type, Type::Class { name: field, .. } if params.contains(field))
                                    });
                                if !matches!(field_type, Type::TypeVar(_))
                                    && !generic_field
                                    && !argument_type.is_subtype_of(&field_type, &self.env)
                                {
                                    return Err(TypeError {
                                        message: format!(
                                            "constructor field '{}' expects {:?}, got {:?}",
                                            field_name, field_type, argument_type
                                        ),
                                        span: argument.value.span(),
                                    });
                                }
                            }
                        }
                    }
                }
                let generic_return = if let Expr::Ident { name, .. } = &**func {
                    let generic_params = self.env.function_type_params.get(name);
                    if let (
                        Some(generic_params),
                        Type::Function {
                            params,
                            return_type,
                        },
                    ) = (generic_params, &ft)
                    {
                        if generic_params.is_empty() {
                            None
                        } else {
                        let mut substitutions = HashMap::new();
                        let generic_names = generic_params
                            .iter()
                            .map(|generic| generic.name.clone())
                            .collect::<HashSet<_>>();
                        let parameter_names = self.env.function_param_names.get(name);
                        let mut positional_index = 0usize;
                        for argument in args {
                            let index = argument
                                .name
                                .as_ref()
                                .and_then(|argument_name| {
                                    parameter_names.and_then(|names| {
                                        names.iter().position(|parameter_name| {
                                            parameter_name == argument_name
                                        })
                                    })
                                })
                                .unwrap_or_else(|| {
                                    let index = positional_index;
                                    positional_index += 1;
                                    index
                                });
                            let Some(parameter) = params.get(index) else {
                                continue;
                            };
                            let argument_type = self.type_of_expr(&argument.value)?;
                            if !infer_type_arguments(
                                parameter,
                                &argument_type,
                                &generic_names,
                                &mut substitutions,
                            ) {
                                return Err(TypeError {
                                    message: format!(
                                        "arguments to generic function '{}' infer conflicting type arguments",
                                        name
                                    ),
                                    span: argument.value.span(),
                                });
                            }
                        }
                        for generic in generic_params {
                            if let Some(argument_type) = substitutions.get(&generic.name) {
                                if let Some(bound) = generic
                                    .bound
                                    .as_ref()
                                    .and_then(|bound| self.resolve_type_expr(bound).ok())
                                {
                                    if !argument_type.is_subtype_of(&bound, &self.env) {
                                        return Err(TypeError {
                                            message: format!(
                                                "type argument for '{}' does not satisfy bound on '{}'",
                                                name, generic.name
                                            ),
                                            span: args
                                                .first()
                                                .map(|argument| argument.value.span())
                                                .unwrap_or_default(),
                                        });
                                    }
                                }
                            }
                        }
                        Some(substitute_type(return_type, &substitutions))
                        }
                    } else {
                        None
                    }
                } else {
                    None
                };
                let overload_return = if let Some(name) = called_name {
                    if self.env.overloaded_functions.contains(name)
                        && !args.iter().any(|argument| {
                            argument.is_spread
                                || argument.is_dict_spread
                                || argument.is_gather_spread
                        })
                    {
                        let argument_types = args
                            .iter()
                            .map(|argument| self.type_of_expr(&argument.value))
                            .collect::<Result<Vec<_>, _>>()?;
                        let candidates = self
                            .env
                            .function_overloads
                            .get(name)
                            .into_iter()
                            .flatten()
                            .filter_map(|candidate| match candidate {
                                Type::Function {
                                    params,
                                    return_type,
                                } if params.len() == argument_types.len()
                                    && params.iter().zip(argument_types.iter()).all(
                                        |(parameter, argument)| {
                                            argument.is_subtype_of(parameter, &self.env)
                                        },
                                    ) =>
                                {
                                    Some((params.clone(), return_type.as_ref().clone()))
                                }
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        if candidates.is_empty() {
                            return Err(TypeError {
                                message: format!(
                                    "no overload of '{}' accepts the supplied argument types",
                                    name
                                ),
                                span: args
                                    .first()
                                    .map(|argument| argument.value.span())
                                    .unwrap_or_default(),
                            });
                        }
                        // Multiple dispatch must not depend on declaration
                        // order. Keep only candidates that are not strictly
                        // less specific than another applicable candidate;
                        // more than one maximal candidate is an ambiguity.
                        let maximal = candidates
                            .iter()
                            .filter(|(params, _)| {
                                !candidates.iter().any(|(other, _)| {
                                    other != params
                                        && other
                                            .iter()
                                            .zip(params.iter())
                                            .all(|(a, b)| a.is_subtype_of(b, &self.env))
                                        && other.iter().zip(params.iter()).any(|(a, b)| a != b)
                                })
                            })
                            .collect::<Vec<_>>();
                        if maximal.len() != 1 {
                            return Err(TypeError {
                                message: format!(
                                    "ambiguous dispatch call to '{}' ({} applicable overloads)",
                                    name,
                                    maximal.len()
                                ),
                                span: args
                                    .first()
                                    .map(|argument| argument.value.span())
                                    .unwrap_or_default(),
                            });
                        }
                        maximal.first().map(|(_, return_type)| return_type.clone())
                    } else {
                        None
                    }
                } else {
                    None
                };
                match ft {
                    Type::Function {
                        params,
                        return_type,
                    } => {
                        if called_member_params.is_some()
                            && !args.iter().any(|argument| {
                                argument.is_spread
                                    || argument.is_dict_spread
                                    || argument.is_gather_spread
                            })
                            && !params.last().is_some_and(|parameter| {
                                matches!(
                                    parameter,
                                    Type::Class { name, .. }
                                        if name == "Arguments"
                                            || name == "Parameters"
                                            || name.ends_with("Arguments")
                                            || name.ends_with("Parameters")
                                )
                            })
                            && args
                                .iter()
                                .filter(|argument| !matches!(argument.value, Expr::Skip(_)))
                                .count()
                                > params.len()
                        {
                            return Err(TypeError {
                                message: "method accepts fewer arguments than supplied".into(),
                                span: args
                                    .last()
                                    .map(|argument| argument.value.span())
                                    .unwrap_or_default(),
                            });
                        }
                        if called_name.is_none()
                            && !args.iter().any(|argument| {
                                argument.is_spread
                                    || argument.is_dict_spread
                                    || argument.is_gather_spread
                            })
                        {
                            let mut positional_index = 0usize;
                            let mut saw_named = false;
                            let mut seen_named = HashSet::new();
                            for (fallback_index, argument) in args.iter().enumerate() {
                                let index = if let Some(argument_name) = &argument.name {
                                    saw_named = true;
                                    if let Some(parameter_names) = &called_member_params {
                                        let Some(index) =
                                            parameter_names.iter().position(|parameter_name| {
                                                parameter_name == argument_name
                                            })
                                        else {
                                            return Err(TypeError {
                                                message: format!(
                                                    "callable has no parameter named '{}'",
                                                    argument_name
                                                ),
                                                span: argument.value.span(),
                                            });
                                        };
                                        if !seen_named.insert(argument_name.clone()) {
                                            return Err(TypeError {
                                                message: format!(
                                                    "argument '{}' passed more than once",
                                                    argument_name
                                                ),
                                                span: argument.value.span(),
                                            });
                                        }
                                        index
                                    } else {
                                        fallback_index
                                    }
                                } else {
                                    if saw_named {
                                        return Err(TypeError {
                                            message: "positional argument follows named argument"
                                                .into(),
                                            span: argument.value.span(),
                                        });
                                    }
                                    let index = positional_index;
                                    positional_index += 1;
                                    index
                                };
                                let Some(parameter) = params.get(index) else {
                                    continue;
                                };
                                if called_member_params.is_some()
                                    && index + 1 == params.len()
                                    && matches!(
                                        parameter,
                                        Type::Class { name, .. }
                                            if name == "Arguments"
                                                || name == "Parameters"
                                                || name.ends_with("Arguments")
                                                || name.ends_with("Parameters")
                                    )
                                {
                                    // A method's final Arguments/Parameters
                                    // slot receives the remaining call
                                    // arguments as a gather bundle; the
                                    // individual values are checked when the
                                    // bundle is constructed, not against the
                                    // bundle class itself.
                                    continue;
                                }
                                if matches!(&argument.value, Expr::Ident { name, .. } if name == "_")
                                {
                                    continue;
                                }
                                let argument_type =
                                    self.argument_type_against_parameter(argument, parameter)?;
                                if !argument_type.is_subtype_of(parameter, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "argument {} has incompatible type for callable",
                                            index + 1
                                        ),
                                        span: argument.value.span(),
                                    });
                                }
                            }
                        }
                        if let Some(name) = called_name {
                            if self.env.function_param_names.contains_key(name)
                                && !self.env.overloaded_functions.contains(name)
                                && self
                                    .env
                                    .function_type_params
                                    .get(name)
                                    .is_none_or(Vec::is_empty)
                                && !args.iter().any(|argument| {
                                    argument.is_spread
                                        || argument.is_dict_spread
                                        || argument.is_gather_spread
                                })
                            {
                                let parameter_names = self
                                    .env
                                    .function_param_names
                                    .get(name)
                                    .cloned()
                                    .unwrap_or_default();
                                if let Some((required, maximum)) = self.env.function_arity.get(name) {
                                    let supplied = args
                                        .iter()
                                        .filter(|argument| !matches!(argument.value, Expr::Skip(_)))
                                        .count();
                                    if supplied < *required {
                                        return Err(TypeError {
                                            message: format!(
                                                "function '{}' requires at least {} arguments",
                                                name, required
                                            ),
                                            span: args
                                                .first()
                                                .map(|argument| argument.value.span())
                                                .unwrap_or_default(),
                                        });
                                    }
                                    if maximum.is_some_and(|maximum| supplied > maximum) {
                                        return Err(TypeError {
                                            message: format!(
                                                "function '{}' accepts at most {} arguments",
                                                name,
                                                maximum.unwrap_or_default()
                                            ),
                                            span: args
                                                .last()
                                                .map(|argument| argument.value.span())
                                                .unwrap_or_default(),
                                        });
                                    }
                                }
                                let mut positional_index = 0usize;
                                let mut saw_named = false;
                                let mut seen_named = HashSet::new();
                                let accepts_gather_bundle =
                                    params.last().is_some_and(Self::is_argument_bundle_type);
                                for argument in args {
                                    let index = if let Some(argument_name) = &argument.name {
                                        saw_named = true;
                                        let index = parameter_names
                                            .iter()
                                            .position(|parameter_name| parameter_name == argument_name);
                                        if index.is_none() && !accepts_gather_bundle {
                                            return Err(TypeError {
                                                message: format!(
                                                    "callable has no parameter named '{}'",
                                                    argument_name
                                                ),
                                                span: argument.value.span(),
                                            });
                                        }
                                        if self
                                            .env
                                            .function_positional_only
                                            .get(name)
                                            .is_some_and(|only| only.contains(argument_name))
                                        {
                                            return Err(TypeError {
                                                message: format!(
                                                    "positional-only argument '{}' passed by keyword",
                                                    argument_name
                                                ),
                                                span: argument.value.span(),
                                            });
                                        }
                                        if !seen_named.insert(argument_name.clone()) {
                                            return Err(TypeError {
                                                message: format!(
                                                    "argument '{}' passed more than once",
                                                    argument_name
                                                ),
                                                span: argument.value.span(),
                                            });
                                        }
                                        let Some(index) = index else {
                                            continue;
                                        };
                                        index
                                    } else {
                                        if saw_named {
                                            return Err(TypeError {
                                                message: "positional argument follows named argument".into(),
                                                span: argument.value.span(),
                                            });
                                        }
                                        let index = positional_index;
                                        positional_index += 1;
                                        if parameter_names
                                            .get(index)
                                            .is_some_and(|parameter_name| {
                                                self.env
                                                    .function_keyword_only
                                                    .get(name)
                                                    .is_some_and(|only| only.contains(parameter_name))
                                            })
                                        {
                                            return Err(TypeError {
                                                message: "keyword-only argument passed positionally".into(),
                                                span: argument.value.span(),
                                            });
                                        }
                                        index
                                    };
                                    let Some(parameter) = params.get(index) else {
                                        continue;
                                    };
                                    if index + 1 == params.len()
                                        && Self::is_argument_bundle_type(parameter)
                                    {
                                        continue;
                                    }
                                    if matches!(&argument.value, Expr::Ident { name, .. } if name == "_")
                                    {
                                        continue;
                                    }
                                    let argument_type =
                                        self.argument_type_against_parameter(argument, parameter)?;
                                    if !argument_type.is_subtype_of(parameter, &self.env) {
                                        return Err(TypeError {
                                            message: format!(
                                                "argument {} to '{}' has incompatible type",
                                                index + 1,
                                                name
                                            ),
                                            span: argument.value.span(),
                                        });
                                    }
                                }
                            }
                        }
                        let holes = args
                            .iter()
                            .filter(|arg| {
                                !arg.is_spread
                                    && !arg.is_dict_spread
                                    && matches!(&arg.value, Expr::Ident { name, .. } if name == "_")
                            })
                            .count();
                        if holes > 0 {
                            let mut bound = vec![false; params.len()];
                            let mut positional_index = 0usize;
                            for arg in args.iter() {
                                if arg.is_spread || arg.is_dict_spread || arg.is_gather_spread {
                                    continue;
                                }
                                // Function types currently retain parameter types but not names;
                                // named arguments therefore follow declaration order here.
                                let index =
                                    (positional_index..params.len()).find(|index| !bound[*index]);
                                positional_index = positional_index.saturating_add(1);
                                if let Some(index) = index {
                                    if !matches!(&arg.value, Expr::Ident { name, .. } if name == "_")
                                    {
                                        bound[index] = true;
                                    }
                                }
                            }
                            let remaining = params
                                .into_iter()
                                .enumerate()
                                .filter_map(|(index, param)| (!bound[index]).then_some(param))
                                .collect();
                            Ok(Type::Function {
                                params: remaining,
                                return_type,
                            })
                        } else {
                            let builtin_return = called_name.and_then(|name| {
                                let argument = args.first().filter(|arg| {
                                    !arg.is_spread && !arg.is_dict_spread && !arg.is_gather_spread
                                })?;
                                let argument_type = self.type_of_expr(&argument.value).ok()?;
                                match name {
                                    "list" if matches!(&argument_type, Type::Shape(_)) => {
                                        Some(Type::Class { name: "list".into(), type_args: vec![Type::Int], parent: None, traits: Vec::new(), interfaces: Vec::new(), fields: HashMap::new(), is_sealed: false })
                                    }
                                    "list" if matches!(&argument_type, Type::Class { type_args, .. } if type_args.len() == 1) => {
                                        if let Type::Class { type_args, .. } = argument_type {
                                            Some(Type::Class { name: "list".into(), type_args, parent: None, traits: Vec::new(), interfaces: Vec::new(), fields: HashMap::new(), is_sealed: false })
                                        } else { None }
                                    }
                                    "set" if matches!(&argument_type, Type::Shape(_)) => {
                                        Some(Type::Class { name: "set".into(), type_args: vec![Type::Int], parent: None, traits: Vec::new(), interfaces: Vec::new(), fields: HashMap::new(), is_sealed: false })
                                    }
                                    "set" if matches!(&argument_type, Type::Class { type_args, .. } if type_args.len() == 1) => {
                                        if let Type::Class { type_args, .. } = argument_type {
                                            Some(Type::Class { name: "set".into(), type_args, parent: None, traits: Vec::new(), interfaces: Vec::new(), fields: HashMap::new(), is_sealed: false })
                                        } else { None }
                                    }
                                    "dict" if matches!(&argument_type, Type::Class { name, type_args, .. } if name == "dict" && type_args.len() == 2) => {
                                        if let Type::Class { type_args, .. } = argument_type {
                                            Some(Type::Class { name: "dict".into(), type_args, parent: None, traits: Vec::new(), interfaces: Vec::new(), fields: HashMap::new(), is_sealed: false })
                                        } else { None }
                                    }
                                    "abs" => match argument_type {
                                        Type::Int | Type::LiteralInt(_) => Some(Type::Int),
                                        Type::Float => Some(Type::Float),
                                        Type::Class { ref name, .. } if name == "complex" => {
                                            Some(Type::Float)
                                        }
                                        _ => None,
                                    },
                                    "round" => match argument_type {
                                        Type::Int | Type::LiteralInt(_) => Some(Type::Int),
                                        Type::Float if args.len() == 1 => Some(Type::Int),
                                        Type::Float => Some(Type::Float),
                                        _ => None,
                                    },
                                    "sum" => match argument_type {
                                        Type::Class { type_args, .. } => type_args.first().map(|ty| match ty {
                                            Type::Float => Type::Float,
                                            _ => Type::Int,
                                        }),
                                        Type::Shape(_) => Some(Type::Int),
                                        _ => None,
                                    },
                                    "min" | "max" if args.len() == 1 => {
                                        Some(self.iterable_element_type(&argument_type))
                                    }
                                    "min" | "max" => {
                                        let mut types = Vec::new();
                                        for argument in args {
                                            types.push(self.type_of_expr(&argument.value).ok()?);
                                        }
                                        Some(Type::make_union(types))
                                    }
                                    _ => None,
                                }
                            });
                            if let Some(builtin_return) = builtin_return {
                                Ok(builtin_return)
                            } else {
                                let is_contextmanager_call = called_name.is_some_and(|name| {
                                    self.env.contextmanager_functions.contains(name)
                                }) || matches!(
                                    &**func,
                                    Expr::Attribute { attr, .. } if attr == "__cm__"
                                );
                                if is_contextmanager_call {
                                    Ok(Type::TypeVar("ContextManager".into()))
                                } else {
                                    Ok(overload_return.or(generic_return).unwrap_or(*return_type))
                                }
                            }
                        }
                    }
                    Type::Class { .. } => Ok(ft), // class construction
                    Type::TypeVar(ref name) if name == "Any" => Ok(ft),
                    Type::Never => Ok(Type::Never),
                    other => Err(TypeError {
                        message: format!("value of type {:?} is not callable", other),
                        span: func.span(),
                    }),
                }
            }
            Expr::Propagate { expr, span } => {
                let inner = self.type_of_expr(expr)?;
                if self.env.current_return_type.is_none() {
                    return Err(TypeError {
                        message: "cannot use '?' outside a function".into(),
                        span: *span,
                    });
                }
                // In Lucid: read_file(path)? extracts Ok value or propagates Error
                match inner {
                    Type::Union(variants) => {
                        // Extract non-error variant
                        let mut ok_types = Vec::new();
                        let mut err_types = Vec::new();

                        for v in variants {
                            if matches!(v, Type::Class { ref name, .. } if name.ends_with("Error"))
                            {
                                err_types.push(v);
                            } else {
                                ok_types.push(v);
                            }
                        }

                        if err_types.is_empty() && ok_types.len() > 1 {
                            let mut alternatives = ok_types.into_iter();
                            let ok_type = alternatives.next().unwrap_or(Type::Never);
                            ok_types = vec![ok_type];
                            err_types = alternatives.collect();
                        }

                        if err_types.is_empty() {
                            return Err(TypeError {
                                message: "cannot use '?' on a type with no recoverable error"
                                    .into(),
                                span: *span,
                            });
                        }

                        if let Some(ref expected_ret) = self.env.current_return_type {
                            for err in err_types {
                                if !err.is_subtype_of(expected_ret, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "'?' propagates error type {:?}, but enclosing function return type does not include it",
                                            err
                                        ),
                                        span: *span,
                                    });
                                }
                            }
                        }

                        Ok(Type::make_union(ok_types))
                    }
                    Type::TypeVar(ref name) if name == "Any" => Ok(inner),
                    Type::None => Ok(Type::None),
                    Type::Never => Ok(Type::Never),
                    other => Err(TypeError {
                        message: format!("cannot use '?' on non-recoverable type {:?}", other),
                        span: *span,
                    }),
                }
            }
            Expr::Freeze { expr, .. } => {
                let inner = self.type_of_expr(expr)?;
                Ok(Type::View {
                    mutability: MutabilityView::Immutable,
                    inner: Box::new(inner),
                })
            }
            Expr::Trust {
                target_type,
                expr,
                span,
            } => {
                let source = self.type_of_expr(expr)?;
                let is_object = matches!(&source, Type::Class { name, .. } if name == "object")
                    || matches!(&source, Type::TypeVar(name) if name == "Any");
                if !is_object {
                    return Err(TypeError {
                        message: "trust[...] only accepts an object-typed operand".into(),
                        span: *span,
                    });
                }
                self.resolve_type_expr(target_type)
            }
            Expr::Skip(_) => Ok(Type::Never),
            Expr::IfExpr {
                condition,
                then_branch,
                else_branch,
                ..
            } => {
                let _ = self.type_of_expr(condition)?;
                let then_t = self.type_of_expr(then_branch)?;
                let else_t = self.type_of_expr(else_branch)?;
                Ok(Type::make_union(vec![then_t, else_t]))
            }
            Expr::List { elements, .. } => {
                if !elements.is_empty()
                    && elements.iter().all(|element| {
                        matches!(
                            element,
                            Expr::Literal {
                                value: LiteralValue::Int(_),
                                ..
                            }
                        )
                    })
                {
                    return Ok(Type::Shape(
                        elements
                            .iter()
                            .map(|element| match element {
                                Expr::Literal {
                                    value: LiteralValue::Int(n),
                                    ..
                                } => Some(*n),
                                _ => None,
                            })
                            .collect(),
                    ));
                }
                let elem_types: Vec<Type> = elements
                    .iter()
                    .filter(|e| !matches!(e, Expr::Skip(_)))
                    .map(|e| self.type_of_expr(e))
                    .collect::<Result<Vec<_>, TypeError>>()?;
                Ok(Type::Class {
                    name: "list".to_string(),
                    type_args: vec![Type::make_union(elem_types)],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                })
            }
            Expr::Set { elements, .. } => {
                let elem_types: Vec<Type> = elements
                    .iter()
                    .filter(|e| !matches!(e, Expr::Skip(_)))
                    .map(|e| self.type_of_expr(e))
                    .collect::<Result<Vec<_>, TypeError>>()?;
                if let Some(unhashable) = elem_types.iter().find(|element_type| {
                    matches!(element_type, Type::Class { name, .. }
                        if matches!(name.as_str(), "list" | "set" | "dict"))
                }) {
                    return Err(TypeError {
                        message: format!("set element of type {:?} is not hashable", unhashable),
                        span: expr.span(),
                    });
                }
                Ok(Type::Class {
                    name: "set".to_string(),
                    type_args: vec![Type::make_union(elem_types)],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                })
            }
            Expr::Record { fields, .. } => {
                let typed_fields = fields
                    .iter()
                    .map(|(name, value)| Ok((name.clone(), self.type_of_expr(value)?)))
                    .collect::<Result<Vec<_>, TypeError>>()?;
                Ok(Type::Record {
                    fields: typed_fields,
                    is_open: false,
                })
            }
            Expr::Dict { entries, .. } => {
                let key_types = entries
                    .iter()
                    .map(|(key, _)| self.type_of_expr(key))
                    .collect::<Result<Vec<_>, TypeError>>()?;
                let value_types = entries
                    .iter()
                    .map(|(_, value)| self.type_of_expr(value))
                    .collect::<Result<Vec<_>, TypeError>>()?;
                if let Some(unhashable) = key_types.iter().find(|key_type| {
                    matches!(key_type, Type::Class { name, .. }
                        if matches!(name.as_str(), "list" | "set" | "dict"))
                }) {
                    return Err(TypeError {
                        message: format!("dictionary key of type {:?} is not hashable", unhashable),
                        span: expr.span(),
                    });
                }
                Ok(Type::Class {
                    name: "dict".to_string(),
                    type_args: vec![Type::make_union(key_types), Type::make_union(value_types)],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                })
            }
            Expr::Construct { args, span } => {
                if !self.env.current_factory {
                    return Err(TypeError {
                        message: "construct(...) is only valid inside a class factory".into(),
                        span: *span,
                    });
                }
                let Some(class_name) = self.env.current_class.as_ref() else {
                    return Err(TypeError {
                        message: "construct(...) requires an enclosing class factory".into(),
                        span: *span,
                    });
                };
                let Some(class_type) = self.env.classes.get(class_name).cloned() else {
                    return Err(TypeError {
                        message: format!("unknown enclosing class '{class_name}'"),
                        span: *span,
                    });
                };
                let field_order = self
                    .env
                    .class_field_order
                    .get(class_name)
                    .cloned()
                    .unwrap_or_default();
                if args.len() != field_order.len() {
                    return Err(TypeError {
                        message: format!(
                            "construct for '{}' expects {} field value(s), got {}",
                            class_name,
                            field_order.len(),
                            args.len()
                        ),
                        span: *span,
                    });
                }
                if let Type::Class { fields, .. } = &class_type {
                    for (index, argument) in args.iter().enumerate() {
                        let field_name = argument
                            .name
                            .as_deref()
                            .unwrap_or_else(|| field_order[index].as_str());
                        let Some(field_type) = fields.get(field_name) else {
                            return Err(TypeError {
                                message: format!(
                                    "construct for '{}' has no field named '{}'",
                                    class_name, field_name
                                ),
                                span: argument.span,
                            });
                        };
                        let argument_type = self.type_of_expr(&argument.value)?;
                        if !argument_type.is_subtype_of(field_type, &self.env) {
                            return Err(TypeError {
                                message: format!(
                                    "construct field '{}' expects {:?}, got {:?}",
                                    field_name, field_type, argument_type
                                ),
                                span: argument.value.span(),
                            });
                        }
                    }
                }
                Ok(class_type)
            }
            Expr::Attribute { value, attr, .. } => {
                let obj_type = match self.type_of_expr(value)? {
                    Type::LiteralStr(_) => Type::Str,
                    Type::LiteralFloat(_) => Type::Float,
                    Type::LiteralBool(_) => Type::Bool,
                    other => other,
                };
                if attr.starts_with('_')
                    && matches!(&obj_type, Type::TypeVar(name) if name == "module")
                {
                    return Err(TypeError {
                        message: format!("module attribute '{attr}' is private"),
                        span: expr.span(),
                    });
                }
                if matches!(value.as_ref(), Expr::Ident { name, .. } if name == "str")
                    && matches!(attr.as_str(), "bin" | "oct" | "hex")
                {
                    return Ok(Type::Function {
                        params: vec![Type::Int],
                        return_type: Box::new(Type::Str),
                    });
                }
                if let Expr::Ident { name, .. } = &**value {
                    if matches!(
                        (name.as_str(), attr.as_str()),
                        ("float", "inf" | "nan") | ("int", "inf" | "nan") | ("complex", "nan")
                    ) {
                        return Ok(match name.as_str() {
                            "float" => Type::Float,
                            "int" => Type::Int,
                            "complex" => self
                                .env
                                .classes
                                .get("complex")
                                .cloned()
                                .unwrap_or(Type::Float),
                            _ => {
                                return Err(TypeError {
                                    message: "invalid numeric special value".into(),
                                    span: expr.span(),
                                })
                            }
                        });
                    }
                }
                if matches!(&obj_type, Type::Class { name, .. } if name == "complex")
                    && matches!(attr.as_str(), "real" | "imag")
                {
                    return Ok(Type::Float);
                }
                match obj_type {
                    Type::Str => match attr.as_str() {
                        "chars" => Ok(Type::View {
                            mutability: MutabilityView::Immutable,
                            inner: Box::new(Type::Class {
                                name: "list".into(),
                                type_args: vec![Type::Str],
                                parent: None,
                                traits: Vec::new(),
                                interfaces: Vec::new(),
                                fields: HashMap::new(),
                                is_sealed: false,
                            }),
                        }),
                        "split" => Ok(Type::Function {
                            params: Vec::new(),
                            return_type: Box::new(Type::Class {
                                name: "list".into(),
                                type_args: vec![Type::Str],
                                parent: None,
                                traits: Vec::new(),
                                interfaces: Vec::new(),
                                fields: HashMap::new(),
                                is_sealed: false,
                            }),
                        }),
                        "upper" | "lower" | "strip" | "trim" | "lstrip" | "rstrip" => {
                            Ok(Type::Function {
                                params: Vec::new(),
                                return_type: Box::new(Type::Str),
                            })
                        }
                        "join" => Ok(Type::Function {
                            params: vec![Type::TypeVar("Any".into())],
                            return_type: Box::new(Type::Str),
                        }),
                        "replace" => Ok(Type::Function {
                            params: vec![Type::Str, Type::Str],
                            return_type: Box::new(Type::Str),
                        }),
                        "startswith" | "endswith" => Ok(Type::Function {
                            params: vec![Type::Str],
                            return_type: Box::new(Type::Bool),
                        }),
                        _ => Err(TypeError {
                            message: format!("type Str has no attribute '{}'", attr),
                            span: expr.span(),
                        }),
                    },
                    Type::Class { ref name, .. } => {
                        if matches!(
                            (name.as_str(), attr.as_str()),
                            ("float", "inf" | "nan") | ("int", "inf" | "nan") | ("complex", "nan")
                        ) {
                            return Ok(match name.as_str() {
                                "float" => Type::Float,
                                "int" => Type::Int,
                                "complex" => self
                                    .env
                                    .classes
                                    .get("complex")
                                    .cloned()
                                    .unwrap_or(Type::Float),
                                _ => {
                                    return Err(TypeError {
                                        message: "invalid numeric special value".into(),
                                        span: expr.span(),
                                    })
                                }
                            });
                        }
                        if name == "str" && matches!(attr.as_str(), "bin" | "oct" | "hex") {
                            return Ok(Type::Function {
                                params: vec![Type::Int],
                                return_type: Box::new(Type::Str),
                            });
                        }
                        if attr.starts_with('_') && self.env.current_class.as_deref() != Some(name)
                        {
                            return Err(TypeError {
                                message: format!("member '{attr}' is private to class '{name}'"),
                                span: expr.span(),
                            });
                        }
                        if let Some(getter_type) = self.class_getter_type(name, attr) {
                            Ok(getter_type)
                        } else if let Some(field_type) = self.class_field_type(name, attr) {
                            Ok(field_type)
                        } else if let Some(class_var_type) = self.class_var_type(name, attr) {
                            Ok(class_var_type)
                        } else if let Some(method_type) = self.class_method_type(name, attr) {
                            Ok(method_type)
                        } else {
                            Err(TypeError {
                                message: format!("class '{name}' has no member '{attr}'"),
                                span: expr.span(),
                            })
                        }
                    }
                    Type::View { ref inner, .. } => {
                        match **inner {
                            Type::Class { ref name, .. } => {
                                if let Some(getter_type) = self.class_getter_type(name, attr) {
                                    return Ok(getter_type);
                                }
                                if let Some(field_type) = self.class_field_type(name, attr) {
                                    return Ok(field_type);
                                }
                                if let Some(method_type) = self.class_method_type(name, attr) {
                                    return Ok(method_type);
                                }
                            }
                            Type::Interface { ref name, .. } => {
                                if let Some(getter_type) = self.interface_getter_type(name, attr) {
                                    return Ok(getter_type);
                                }
                                if let Some(field_type) = self.interface_field_type(name, attr) {
                                    return Ok(field_type);
                                }
                                if let Some(method_type) = self.interface_method_type(name, attr) {
                                    return Ok(method_type);
                                }
                            }
                            Type::Trait { ref name, .. } => {
                                if let Some(getter_type) = self.trait_getter_type(name, attr) {
                                    return Ok(getter_type);
                                }
                                if let Some(field_type) = self.trait_field_type(name, attr) {
                                    return Ok(field_type);
                                }
                                if let Some(method_type) = self.trait_method_type(name, attr) {
                                    return Ok(method_type);
                                }
                            }
                            Type::Record { ref fields, .. } => {
                                if let Some((_, field_type)) = fields
                                    .iter()
                                    .find(|(name, _)| name.as_deref() == Some(attr))
                                {
                                    return Ok(field_type.clone());
                                }
                            }
                            _ => {}
                        }
                        Err(TypeError {
                            message: "view has no member".into(),
                            span: expr.span(),
                        })
                    }
                    Type::Interface { ref name, .. } => {
                        if let Some(getter_type) = self.interface_getter_type(name, attr) {
                            Ok(getter_type)
                        } else if let Some(field_type) = self.interface_field_type(name, attr) {
                            Ok(field_type)
                        } else if let Some(method_type) = self.interface_method_type(name, attr) {
                            Ok(method_type)
                        } else {
                            Err(TypeError {
                                message: format!("interface '{name}' has no member '{attr}'"),
                                span: expr.span(),
                            })
                        }
                    }
                    Type::Trait { ref name, .. } => {
                        if let Some(getter_type) = self.trait_getter_type(name, attr) {
                            Ok(getter_type)
                        } else if let Some(field_type) = self.trait_field_type(name, attr) {
                            Ok(field_type)
                        } else if let Some(method_type) = self.trait_method_type(name, attr) {
                            Ok(method_type)
                        } else {
                            Err(TypeError {
                                message: format!("trait '{name}' has no member '{attr}'"),
                                span: expr.span(),
                            })
                        }
                    }
                    Type::Record { fields, .. } => fields
                        .iter()
                        .find(|(name, _)| name.as_deref() == Some(attr))
                        .map(|(_, field_type)| field_type.clone())
                        .ok_or_else(|| TypeError {
                            message: format!("record has no field '{attr}'"),
                            span: expr.span(),
                        }),
                    Type::TypeVar(name) if matches!(name.as_str(), "Any" | "module" | "super") => {
                        Ok(Type::TypeVar("Any".to_string()))
                    }
                    other => Err(TypeError {
                        message: format!("type {:?} has no attribute '{}'", other, attr),
                        span: expr.span(),
                    }),
                }
            }
            Expr::AnonymousDef {
                params,
                return_type,
                body,
                ..
            } => {
                let (ptypes, ret_t, body_return_type) =
                    self.anonymous_function_signature(params, return_type.as_ref())?;
                let mut closure_checker = self.clone();
                closure_checker.env.current_return_type = Some(body_return_type);
                for (param, parameter_type) in params.iter().zip(ptypes.iter()) {
                    closure_checker.env.variables.insert(
                        param.name.clone(),
                        (parameter_type.clone(), MutabilityView::Mutable),
                    );
                }
                for statement in body {
                    closure_checker.check_statement(statement)?;
                }
                Ok(Type::Function {
                    params: ptypes,
                    return_type: Box::new(ret_t),
                })
            }
            Expr::Type(te) => self.resolve_type_expr(te),
            Expr::ListComp {
                element,
                target,
                iter,
                condition,
                ..
            } => {
                let iter_t = self.type_of_expr(iter)?;
                if !self.is_iterable_type(&iter_t) {
                    return Err(TypeError {
                        message: format!("value of type {:?} is not iterable", iter_t),
                        span: iter.span(),
                    });
                }
                let elem_t = self.iterable_element_type(&iter_t);
                let mut sub = self.clone();
                if let Pattern::Ident(name, _) = target {
                    if name != "_" {
                    sub.env
                        .variables
                        .insert(name.clone(), (elem_t, MutabilityView::ReadOnly));
                    }
                }
                if let Some(condition) = condition {
                    let condition_type = sub.type_of_expr(condition)?;
                    if !sub.is_truthy_type(&condition_type) {
                        return Err(TypeError {
                            message: format!(
                                "list comprehension condition must be bool, got {:?}",
                                condition_type
                            ),
                            span: condition.span(),
                        });
                    }
                }
                let et = sub.type_of_expr(element)?;
                Ok(Type::Class {
                    name: "list".to_string(),
                    type_args: vec![et],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                })
            }
            Expr::SetComp {
                element,
                target,
                iter,
                condition,
                ..
            } => {
                let iter_t = self.type_of_expr(iter)?;
                if !self.is_iterable_type(&iter_t) {
                    return Err(TypeError {
                        message: format!("value of type {:?} is not iterable", iter_t),
                        span: iter.span(),
                    });
                }
                let elem_t = self.iterable_element_type(&iter_t);
                let mut sub = self.clone();
                if let Pattern::Ident(name, _) = target {
                    if name != "_" {
                    sub.env
                        .variables
                        .insert(name.clone(), (elem_t, MutabilityView::ReadOnly));
                    }
                }
                if let Some(condition) = condition {
                    let condition_type = sub.type_of_expr(condition)?;
                    if !sub.is_truthy_type(&condition_type) {
                        return Err(TypeError {
                            message: format!(
                                "set comprehension condition must be bool, got {:?}",
                                condition_type
                            ),
                            span: condition.span(),
                        });
                    }
                }
                let et = sub.type_of_expr(element)?;
                Ok(Type::Class {
                    name: "set".to_string(),
                    type_args: vec![et],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                })
            }
            Expr::DictComp {
                key,
                value,
                target,
                iter,
                condition,
                ..
            } => {
                let iter_t = self.type_of_expr(iter)?;
                if !self.is_iterable_type(&iter_t) {
                    return Err(TypeError {
                        message: format!("value of type {:?} is not iterable", iter_t),
                        span: iter.span(),
                    });
                }
                let elem_t = self.iterable_element_type(&iter_t);
                let mut sub = self.clone();
                if let Pattern::Ident(name, _) = target {
                    if name != "_" {
                    sub.env
                        .variables
                        .insert(name.clone(), (elem_t, MutabilityView::ReadOnly));
                    }
                }
                if let Some(condition) = condition {
                    let condition_type = sub.type_of_expr(condition)?;
                    if !sub.is_truthy_type(&condition_type) {
                        return Err(TypeError {
                            message: format!(
                                "dict comprehension condition must be bool, got {:?}",
                                condition_type
                            ),
                            span: condition.span(),
                        });
                    }
                }
                let kt = sub.type_of_expr(key)?;
                let vt = sub.type_of_expr(value)?;
                Ok(Type::Class {
                    name: "dict".to_string(),
                    type_args: vec![kt, vt],
                    parent: None,
                    traits: Vec::new(),
                    interfaces: Vec::new(),
                    fields: HashMap::new(),
                    is_sealed: false,
                })
            }
            Expr::Index { value, index, .. } => {
                let val_t = self.type_of_expr(value)?;
                if matches!(**index, Expr::Slice { .. }) {
                    if let Expr::Slice {
                        start, stop, step, ..
                    } = &**index
                    {
                        for bound in [start.as_deref(), stop.as_deref(), step.as_deref()]
                            .into_iter()
                            .flatten()
                        {
                            let bound_type = self.type_of_expr(bound)?;
                            if !bound_type.is_subtype_of(&Type::Int, &self.env) {
                                return Err(TypeError {
                                    message: format!(
                                        "slice bound must be int, got {:?}",
                                        bound_type
                                    ),
                                    span: bound.span(),
                                });
                            }
                        }
                        if let Some(Expr::Literal {
                            value: LiteralValue::Int(step),
                            ..
                        }) = step.as_deref()
                        {
                            if *step == 0 {
                                return Err(TypeError {
                                    message: "slice step cannot be zero".into(),
                                    span: index.span(),
                                });
                            }
                        }
                    }
                    match &val_t {
                        Type::View { ref inner, .. } => match inner.as_ref() {
                            Type::Class { name, .. }
                                if matches!(
                                    name.as_str(),
                                    "list" | "str" | "range" | "Bytes" | "ByteArray" | "MemoryView"
                                ) =>
                            {
                                Ok(val_t.clone())
                            }
                            Type::Shape(_) | Type::Record { .. } => Ok(val_t.clone()),
                            _ => Err(TypeError {
                                message: "value is not sliceable".into(),
                                span: index.span(),
                            }),
                        },
                        Type::Class { name, .. }
                            if matches!(
                                name.as_str(),
                                "list" | "str" | "range" | "Bytes" | "ByteArray" | "MemoryView"
                            ) || matches!(&val_t, Type::Shape(_) | Type::Record { .. }) =>
                        {
                            Ok(val_t)
                        }
                        Type::Shape(_) | Type::Record { .. } => Ok(val_t),
                        _ => Err(TypeError {
                            message: "value is not sliceable".into(),
                            span: index.span(),
                        }),
                    }
                } else {
                    let index_t = self.type_of_expr(index)?;
                    let require_int = || {
                        if !index_t.is_subtype_of(&Type::Int, &self.env) {
                            return Err(TypeError {
                                message: format!("sequence index must be int, got {:?}", index_t),
                                span: index.span(),
                            });
                        }
                        Ok(())
                    };
                    match val_t {
                        Type::Class {
                            ref name,
                            ref type_args,
                            ..
                        } if name == "list" => {
                            require_int()?;
                            Ok(type_args
                                .first()
                                .cloned()
                                .unwrap_or(Type::TypeVar("Any".to_string())))
                        }
                        Type::Trait {
                            ref name,
                            ref type_args,
                            ..
                        } if name == "Sequence" => {
                            require_int()?;
                            Ok(type_args
                                .first()
                                .cloned()
                                .unwrap_or(Type::TypeVar("Any".to_string())))
                        }
                        Type::Class { ref name, .. }
                            if matches!(name.as_str(), "Bytes" | "ByteArray" | "MemoryView") =>
                        {
                            require_int()?;
                            Ok(Type::Int)
                        }
                        Type::View { ref inner, .. } => match inner.as_ref() {
                            Type::Class {
                                name, type_args, ..
                            } if name == "list" => {
                                require_int()?;
                                Ok(type_args
                                    .first()
                                    .cloned()
                                    .unwrap_or(Type::TypeVar("Any".to_string())))
                            }
                            Type::Class { name, .. } if name == "str" => {
                                require_int()?;
                                Ok(Type::Str)
                            }
                            Type::Trait {
                                name, type_args, ..
                            } if name == "Sequence" => {
                                require_int()?;
                                Ok(type_args
                                    .first()
                                    .cloned()
                                    .unwrap_or(Type::TypeVar("Any".to_string())))
                            }
                            Type::Class { name, .. }
                                if matches!(
                                    name.as_str(),
                                    "Bytes" | "ByteArray" | "MemoryView"
                                ) =>
                            {
                                require_int()?;
                                Ok(Type::Int)
                            }
                            _ => Err(TypeError {
                                message: format!("type {:?} is not indexable", val_t),
                                span: index.span(),
                            }),
                        },
                        Type::Class {
                            ref name,
                            ref type_args,
                            ..
                        } if name == "dict" => {
                            if let Some(key_type) = type_args.first() {
                                if !index_t.is_subtype_of(key_type, &self.env) {
                                    return Err(TypeError {
                                        message: format!(
                                            "dictionary key has type {:?}, expected {:?}",
                                            index_t, key_type
                                        ),
                                        span: index.span(),
                                    });
                                }
                            }
                            Ok(type_args
                                .get(1)
                                .cloned()
                                .unwrap_or(Type::TypeVar("Any".to_string())))
                        }
                        Type::Shape(dimensions) => {
                            require_int()?;
                            if let Expr::Literal {
                                value: LiteralValue::Int(position),
                                ..
                            } = &**index
                            {
                                let position = if *position < 0 {
                                    dimensions
                                        .len()
                                        .checked_sub(position.unsigned_abs() as usize)
                                } else {
                                    usize::try_from(*position).ok()
                                };
                                position
                                    .and_then(|position| dimensions.get(position).copied())
                                    .map(|dimension| dimension.map_or(Type::Int, Type::LiteralInt))
                                    .ok_or_else(|| TypeError {
                                        message: "shape index is out of bounds".into(),
                                        span: index.span(),
                                    })
                            } else {
                                Ok(Type::Int)
                            }
                        }
                        Type::Record { fields, .. } => match &**index {
                            Expr::Literal {
                                value: LiteralValue::Int(position),
                                ..
                            } => {
                                let position = if *position < 0 {
                                    fields.len().checked_sub(position.unsigned_abs() as usize)
                                } else {
                                    usize::try_from(*position).ok()
                                };
                                position
                                    .and_then(|position| fields.get(position))
                                    .map(|(_, ty)| ty.clone())
                                    .ok_or_else(|| TypeError {
                                        message: "record index is out of bounds".into(),
                                        span: index.span(),
                                    })
                            }
                            Expr::Literal {
                                value: LiteralValue::Str(field),
                                ..
                            } => fields
                                .iter()
                                .find(|(name, _)| name.as_deref() == Some(field))
                                .map(|(_, ty)| ty.clone())
                                .ok_or_else(|| TypeError {
                                    message: format!("record has no field '{field}'"),
                                    span: index.span(),
                                }),
                            _ if index_t.is_subtype_of(&Type::Int, &self.env)
                                || index_t.is_subtype_of(&Type::Str, &self.env) =>
                            {
                                Ok(Type::make_union(
                                    fields.iter().map(|(_, ty)| ty.clone()).collect(),
                                ))
                            }
                            _ => Err(TypeError {
                                message: format!(
                                    "record index must be int or str, got {:?}",
                                    index_t
                                ),
                                span: index.span(),
                            }),
                        },
                        Type::Class { ref name, .. } if name == "str" => {
                            require_int()?;
                            Ok(Type::Str)
                        }
                        Type::Str => {
                            require_int()?;
                            Ok(Type::Str)
                        }
                        _ => Err(TypeError {
                            message: format!("type {:?} is not indexable", val_t),
                            span: index.span(),
                        }),
                    }
                }
            }
            Expr::Slice { span, .. } => Err(TypeError {
                message: "slice expression is only valid inside indexing".into(),
                span: *span,
            }),
        }
    }

    pub fn resolve_type_expr(&self, texpr: &TypeExpr) -> Result<Type, TypeError> {
        match texpr {
            TypeExpr::Named {
                name,
                args,
                span: _,
            } => {
                let resolved_args = args
                    .iter()
                    .map(|a| self.resolve_type_expr(a))
                    .collect::<Result<Vec<_>, _>>()?;
                match name.as_str() {
                    "int" | "float" | "bool" | "str" | "bytes" | "Bytes" | "none" | "Never" => {
                        if !resolved_args.is_empty() {
                            return Err(TypeError {
                                message: format!(
                                    "type '{}' expects no type arguments, got {}",
                                    name,
                                    resolved_args.len()
                                ),
                                span: texpr.span(),
                            });
                        }
                        Ok(match name.as_str() {
                            "int" => Type::Int,
                            "float" => Type::Float,
                            "bool" => Type::Bool,
                            "str" => Type::Str,
                            "bytes" | "Bytes" => Type::Class {
                                name: "Bytes".into(),
                                type_args: Vec::new(),
                                parent: None,
                                traits: Vec::new(),
                                interfaces: vec![
                                    "Buffer".into(),
                                    "Sized".into(),
                                    "Container".into(),
                                ],
                                fields: HashMap::new(),
                                is_sealed: true,
                            },
                            "none" => Type::None,
                            "Never" => Type::Never,
                            _ => {
                                return Err(TypeError {
                                    message: format!("unsupported type name '{name}'"),
                                    span: texpr.span(),
                                })
                            }
                        })
                    }
                    "typing.shape" | "shape" => {
                        // A shape position is either an integer literal or a
                        // type variable standing for an unknown dimension.
                        // Silently turning arbitrary types (for example
                        // `str` or `float`) into an unknown dimension makes a
                        // malformed shape appear valid and defeats exact
                        // shape checking.
                        let mut dimensions = Vec::with_capacity(resolved_args.len());
                        for (arg, resolved) in args.iter().zip(resolved_args.iter()) {
                            match resolved {
                                Type::LiteralInt(value) if *value >= 0 => {
                                    dimensions.push(Some(*value))
                                }
                                Type::LiteralInt(_) => {
                                    return Err(TypeError {
                                        message: "shape dimensions cannot be negative".into(),
                                        span: arg.span(),
                                    });
                                }
                                Type::TypeVar(_) => dimensions.push(None),
                                _ => {
                                    return Err(TypeError { message: "shape dimensions must be int literals or type variables".into(), span: arg.span() });
                                }
                            }
                        }
                        Ok(Type::Shape(dimensions))
                    }
                    "__shape_add__" => {
                        if let [Type::LiteralInt(a), Type::LiteralInt(b)] = resolved_args.as_slice()
                        {
                            return a.checked_add(*b).map(Type::LiteralInt).ok_or_else(|| {
                                TypeError {
                                    message: "shape dimension arithmetic overflow".into(),
                                    span: texpr.span(),
                                }
                            });
                        }
                        if let [Type::Shape(left), Type::Shape(right)] = resolved_args.as_slice() {
                            let mut dimensions = left.clone();
                            dimensions.extend(right.iter().copied());
                            Ok(Type::Shape(dimensions))
                        } else {
                            Ok(Type::TypeVar("Any".into()))
                        }
                    }
                    "__shape_sub__" | "__shape_mul__" | "__shape_neg__" => {
                        // Arithmetic over scalar literal dimensions is retained
                        // for the future constant evaluator; shape concatenation
                        // above is already structural and exact.
                        match (name.as_str(), resolved_args.as_slice()) {
                            ("__shape_sub__", [Type::LiteralInt(a), Type::LiteralInt(b)]) => a
                                .checked_sub(*b)
                                .map(Type::LiteralInt)
                                .ok_or_else(|| TypeError {
                                    message: "shape dimension arithmetic overflow".into(),
                                    span: texpr.span(),
                                }),
                            ("__shape_mul__", [Type::LiteralInt(a), Type::LiteralInt(b)]) => a
                                .checked_mul(*b)
                                .map(Type::LiteralInt)
                                .ok_or_else(|| TypeError {
                                    message: "shape dimension arithmetic overflow".into(),
                                    span: texpr.span(),
                                }),
                            ("__shape_neg__", [Type::LiteralInt(a)]) => a
                                .checked_neg()
                                .map(Type::LiteralInt)
                                .ok_or_else(|| TypeError {
                                    message: "shape dimension arithmetic overflow".into(),
                                    span: texpr.span(),
                                }),
                            _ => Ok(Type::TypeVar("shape-op".into())),
                        }
                    }
                    "__shape_cond__" => Ok(Type::make_union(resolved_args)),
                    "__shape_slice__" => {
                        if let [Type::Shape(values), start, stop, step] = resolved_args.as_slice() {
                            let to_index = |value: &Type| match value {
                                Type::LiteralInt(n) => Some(*n),
                                _ => None,
                            };
                            let step_value = to_index(step).unwrap_or(1);
                            if step_value == 0 {
                                return Err(TypeError {
                                    message: "shape slice step cannot be zero".into(),
                                    span: texpr.span(),
                                });
                            }
                            let len = values.len() as i64;
                            let normalize =
                                |n: i64| if n < 0 { (len + n).max(0) } else { n.min(len) };
                            let begin = to_index(start)
                                .map(normalize)
                                .unwrap_or(if step_value > 0 { 0 } else { len - 1 });
                            let finish = to_index(stop)
                                .map(normalize)
                                .unwrap_or(if step_value > 0 { len } else { -1 });
                            let mut out = Vec::new();
                            let mut index = begin;
                            while if step_value > 0 {
                                index < finish
                            } else {
                                index > finish
                            } {
                                if index < 0 || index >= len {
                                    break;
                                }
                                out.push(values[index as usize]);
                                let Some(next) = index.checked_add(step_value) else {
                                    return Err(TypeError {
                                        message: "shape slice step overflow".into(),
                                        span: texpr.span(),
                                    });
                                };
                                index = next;
                            }
                            Ok(Type::Shape(out))
                        } else {
                            Ok(Type::TypeVar("shape-op".into()))
                        }
                    }
                    "__shape_index__" => {
                        if let [Type::Shape(values), Type::LiteralInt(index)] =
                            resolved_args.as_slice()
                        {
                            let mut index = *index;
                            if index < 0 {
                                index += values.len() as i64;
                            }
                            if index < 0 || index >= values.len() as i64 {
                                return Err(TypeError {
                                    message: "shape index out of range".into(),
                                    span: texpr.span(),
                                });
                            }
                            Ok(values[index as usize]
                                .map(Type::LiteralInt)
                                .unwrap_or(Type::TypeVar("int".into())))
                        } else {
                            Ok(Type::TypeVar("int".into()))
                        }
                    }
                    "__shape_rank__" => {
                        if let [Type::Shape(values)] = resolved_args.as_slice() {
                            Ok(Type::LiteralInt(values.len() as i64))
                        } else {
                            // Generic or unconstrained shapes have no
                            // statically known rank; retain an integer type
                            // variable for later constraint solving.
                            Ok(Type::TypeVar("int".into()))
                        }
                    }
                    "__shape_product__" => {
                        if let [Type::Shape(values)] = resolved_args.as_slice() {
                            let mut product = 1i64;
                            for dimension in values {
                                let Some(dimension) = dimension else {
                                    return Ok(Type::TypeVar("int".into()));
                                };
                                product =
                                    product.checked_mul(*dimension).ok_or_else(|| TypeError {
                                        message: "shape element-count arithmetic overflow".into(),
                                        span: texpr.span(),
                                    })?;
                            }
                            Ok(Type::LiteralInt(product))
                        } else {
                            Ok(Type::TypeVar("int".into()))
                        }
                    }
                    "__shape_broadcast_dim__" => {
                        if let [Type::LiteralInt(left), Type::LiteralInt(right)] =
                            resolved_args.as_slice()
                        {
                            if left == right {
                                return Ok(Type::LiteralInt(*left));
                            }
                            if *left == 1 {
                                return Ok(Type::LiteralInt(*right));
                            }
                            if *right == 1 {
                                return Ok(Type::LiteralInt(*left));
                            }
                            return Err(TypeError {
                                message: "shape dimensions are not broadcast-compatible".into(),
                                span: texpr.span(),
                            });
                        }
                        Ok(Type::TypeVar("int".into()))
                    }
                    "__shape_broadcast__" => {
                        if let [Type::Shape(left), Type::Shape(right)] = resolved_args.as_slice() {
                            let mut result = Vec::with_capacity(left.len().max(right.len()));
                            let mut li = left.len();
                            let mut ri = right.len();
                            while li > 0 || ri > 0 {
                                let ldim = li.checked_sub(1).and_then(|index| left.get(index));
                                let rdim = ri.checked_sub(1).and_then(|index| right.get(index));
                                let dimension = match (ldim, rdim) {
                                    (Some(Some(l)), Some(Some(r))) if l == r => Some(*l),
                                    (Some(Some(1)), Some(Some(r))) => Some(*r),
                                    (Some(Some(l)), Some(Some(1))) => Some(*l),
                                    (Some(Some(_)), Some(Some(_))) => {
                                        return Err(TypeError {
                                            message:
                                                "shape dimensions are not broadcast-compatible"
                                                    .into(),
                                            span: texpr.span(),
                                        });
                                    }
                                    (Some(Some(value)), None) | (None, Some(Some(value))) => {
                                        Some(*value)
                                    }
                                    _ => None,
                                };
                                result.push(dimension);
                                li = li.saturating_sub(1);
                                ri = ri.saturating_sub(1);
                            }
                            result.reverse();
                            Ok(Type::Shape(result))
                        } else {
                            Ok(Type::TypeVar("shape-op".into()))
                        }
                    }
                    "__shape_matmul__" => {
                        let [Type::Shape(left), Type::Shape(right)] = resolved_args.as_slice()
                        else {
                            return Ok(Type::TypeVar("shape-op".into()));
                        };
                        if left.len() < 2 || right.len() < 2 {
                            return Err(TypeError {
                                message: "matrix multiplication requires shapes of rank at least 2"
                                    .into(),
                                span: texpr.span(),
                            });
                        }
                        let left_inner = left[left.len() - 1];
                        let right_inner = right[right.len() - 2];
                        if let (Some(left_inner), Some(right_inner)) = (left_inner, right_inner) {
                            if left_inner != right_inner {
                                return Err(TypeError {
                                    message: "matrix multiplication inner dimensions must match"
                                        .into(),
                                    span: texpr.span(),
                                });
                            }
                        }
                        let left_batch = &left[..left.len() - 2];
                        let right_batch = &right[..right.len() - 2];
                        let mut batch = Vec::with_capacity(left_batch.len().max(right_batch.len()));
                        let mut li = left_batch.len();
                        let mut ri = right_batch.len();
                        while li > 0 || ri > 0 {
                            let ldim = li.checked_sub(1).and_then(|index| left_batch.get(index));
                            let rdim = ri.checked_sub(1).and_then(|index| right_batch.get(index));
                            let dimension = match (ldim, rdim) {
                                (Some(Some(l)), Some(Some(r))) if l == r => Some(*l),
                                (Some(Some(1)), Some(Some(r))) => Some(*r),
                                (Some(Some(l)), Some(Some(1))) => Some(*l),
                                (Some(Some(_)), Some(Some(_))) => {
                                    return Err(TypeError {
                                        message: "shape dimensions are not broadcast-compatible"
                                            .into(),
                                        span: texpr.span(),
                                    });
                                }
                                (Some(Some(value)), None) | (None, Some(Some(value))) => {
                                    Some(*value)
                                }
                                _ => None,
                            };
                            batch.push(dimension);
                            li = li.saturating_sub(1);
                            ri = ri.saturating_sub(1);
                        }
                        batch.reverse();
                        batch.push(left[left.len() - 2]);
                        batch.push(right[right.len() - 1]);
                        Ok(Type::Shape(batch))
                    }
                    "__shape_reshape__" => {
                        let [Type::Shape(source), Type::Shape(target)] = resolved_args.as_slice()
                        else {
                            return Ok(Type::TypeVar("shape-op".into()));
                        };
                        let product = |values: &[Option<i64>]| {
                            let mut total = 1i64;
                            for value in values {
                                let value = (*value)?;
                                total = total.checked_mul(value)?;
                            }
                            Some(total)
                        };
                        if let (Some(source_count), Some(target_count)) =
                            (product(source), product(target))
                        {
                            if source_count != target_count {
                                return Err(TypeError {
                                    message: "reshape element counts must match".into(),
                                    span: texpr.span(),
                                });
                            }
                        }
                        Ok(Type::Shape(target.clone()))
                    }
                    "__shape_concat_axis0__" => {
                        let [Type::Shape(left), Type::Shape(right)] = resolved_args.as_slice()
                        else {
                            return Ok(Type::TypeVar("shape-op".into()));
                        };
                        if left.is_empty() || right.is_empty() || left.len() != right.len() {
                            return Err(TypeError {
                                message: "axis-0 concatenation requires equal non-empty ranks"
                                    .into(),
                                span: texpr.span(),
                            });
                        }
                        let mut trailing = Vec::with_capacity(left.len() - 1);
                        for (l, r) in left[1..].iter().zip(&right[1..]) {
                            match (l, r) {
                                (Some(l), Some(r)) if l != r => {
                                    return Err(TypeError {
                                        message: "axis-0 concatenation requires matching trailing dimensions".into(),
                                        span: texpr.span(),
                                    });
                                }
                                (Some(value), Some(_)) => trailing.push(Some(*value)),
                                _ => trailing.push(None),
                            }
                        }
                        let first = match (left[0], right[0]) {
                            (Some(l), Some(r)) => {
                                Some(l.checked_add(r).ok_or_else(|| TypeError {
                                    message: "shape dimension arithmetic overflow".into(),
                                    span: texpr.span(),
                                })?)
                            }
                            _ => None,
                        };
                        let mut result = Vec::with_capacity(left.len());
                        result.push(first);
                        result.extend(trailing);
                        Ok(Type::Shape(result))
                    }
                    "__shape_reverse__" => {
                        if let [Type::Shape(values)] = resolved_args.as_slice() {
                            let mut reversed = values.clone();
                            reversed.reverse();
                            Ok(Type::Shape(reversed))
                        } else {
                            Ok(Type::TypeVar("shape-op".into()))
                        }
                    }
                    "__shape_swap_last2__" => {
                        if let [Type::Shape(values)] = resolved_args.as_slice() {
                            if values.len() < 2 {
                                return Err(TypeError {
                                    message: "swapping the last two axes requires rank at least 2"
                                        .into(),
                                    span: texpr.span(),
                                });
                            }
                            let mut swapped = values.clone();
                            let last = swapped.len() - 1;
                            swapped.swap(last, last - 1);
                            Ok(Type::Shape(swapped))
                        } else {
                            Ok(Type::TypeVar("shape-op".into()))
                        }
                    }
                    "__shape_insert_at__" => {
                        if let [Type::Shape(values), Type::LiteralInt(index), Type::LiteralInt(dimension)] =
                            resolved_args.as_slice()
                        {
                            let mut index = *index;
                            if index < 0 {
                                index += values.len() as i64;
                            }
                            if index < 0 || index > values.len() as i64 || *dimension < 0 {
                                return Err(TypeError {
                                    message: "shape insertion index or dimension is out of range"
                                        .into(),
                                    span: texpr.span(),
                                });
                            }
                            let mut result = values.clone();
                            result.insert(index as usize, Some(*dimension));
                            Ok(Type::Shape(result))
                        } else {
                            Ok(Type::TypeVar("shape-op".into()))
                        }
                    }
                    "__shape_drop_at__" => {
                        if let [Type::Shape(values), Type::LiteralInt(index)] =
                            resolved_args.as_slice()
                        {
                            let mut index = *index;
                            if index < 0 {
                                index += values.len() as i64;
                            }
                            if index < 0 || index >= values.len() as i64 {
                                return Err(TypeError {
                                    message: "shape deletion index is out of range".into(),
                                    span: texpr.span(),
                                });
                            }
                            let mut result = values.clone();
                            result.remove(index as usize);
                            Ok(Type::Shape(result))
                        } else {
                            Ok(Type::TypeVar("shape-op".into()))
                        }
                    }
                    "__shape_stack__" => {
                        if resolved_args.is_empty()
                            || !resolved_args
                                .iter()
                                .all(|value| matches!(value, Type::Shape(_)))
                        {
                            return Ok(Type::TypeVar("shape-op".into()));
                        }
                        let shapes = resolved_args
                            .iter()
                            .filter_map(|value| match value {
                                Type::Shape(shape) => Some(shape),
                                _ => None,
                            })
                            .collect::<Vec<_>>();
                        if shapes.len() != resolved_args.len() {
                            return Ok(Type::TypeVar("shape-op".into()));
                        }
                        let rank = shapes[0].len();
                        if shapes.iter().any(|shape| shape.len() != rank) {
                            return Err(TypeError {
                                message: "stack requires all operands to have the same rank".into(),
                                span: texpr.span(),
                            });
                        }
                        let mut dimensions = Vec::with_capacity(rank + 1);
                        dimensions.push(Some(shapes.len() as i64));
                        for axis in 0..rank {
                            let mut known = None;
                            let mut all_known = true;
                            for shape in &shapes {
                                let value = shape[axis];
                                if value.is_none() {
                                    all_known = false;
                                }
                                match (known, value) {
                                    (Some(left), Some(right)) if left != right => {
                                        return Err(TypeError {
                                            message: "stack requires matching operand shapes"
                                                .into(),
                                            span: texpr.span(),
                                        });
                                    }
                                    (None, Some(value)) => known = Some(value),
                                    _ => {}
                                }
                            }
                            dimensions.push(all_known.then_some(known).flatten());
                        }
                        Ok(Type::Shape(dimensions))
                    }
                    other => {
                        if other == "__intersection__" {
                            return Ok(Type::Intersection(resolved_args));
                        }
                        if other == "__not__" {
                            return Ok(Type::Negation(Box::new(
                                resolved_args
                                    .into_iter()
                                    .next()
                                    .unwrap_or(Type::TypeVar("Any".into())),
                            )));
                        }
                        if other.starts_with("__shape_") {
                            return Ok(Type::TypeVar(other.to_string()));
                        }
                        if let Some(alias) = self.env.type_aliases.get(other) {
                            let params = self
                                .env
                                .type_alias_params
                                .get(other)
                                .cloned()
                                .unwrap_or_default();
                            if !params.is_empty() {
                                if params.len() != resolved_args.len() {
                                    return Err(TypeError {
                                        message: format!(
                                            "type alias '{}' expects {} argument(s), got {}",
                                            other,
                                            params.len(),
                                            resolved_args.len()
                                        ),
                                        span: texpr.span(),
                                    });
                                }
                                if let Some(bounds) = self.env.type_alias_bounds.get(other) {
                                    for (index, argument) in resolved_args.iter().enumerate() {
                                        if let Some(Some(bound)) = bounds.get(index) {
                                            if !argument.is_subtype_of(bound, &self.env) {
                                                return Err(TypeError {
                                                    message: format!(
                                                        "type argument {} for alias '{}' does not satisfy its bound",
                                                        index + 1,
                                                        other
                                                    ),
                                                    span: texpr.span(),
                                                });
                                            }
                                        }
                                    }
                                }
                                let substitutions = params
                                    .into_iter()
                                    .zip(resolved_args.iter().cloned())
                                    .collect::<HashMap<_, _>>();
                                return Ok(substitute_type(alias, &substitutions));
                            }
                            if !resolved_args.is_empty() {
                                return Err(TypeError {
                                    message: format!(
                                        "type alias '{}' expects no type arguments, got {}",
                                        other,
                                        resolved_args.len()
                                    ),
                                    span: texpr.span(),
                                });
                            }
                            return Ok(alias.clone());
                        }
                        if let Some(c) = self.env.classes.get(other) {
                            if let Some(parameters) = self.env.class_variance.get(other) {
                                if parameters.len() != resolved_args.len() {
                                    return Err(TypeError {
                                        message: format!(
                                            "type '{}' expects {} argument(s), got {}",
                                            other,
                                            parameters.len(),
                                            resolved_args.len()
                                        ),
                                        span: texpr.span(),
                                    });
                                }
                            }
                            if let Some(bounds) = self.env.class_bounds.get(other) {
                                for (index, argument) in resolved_args.iter().enumerate() {
                                    if let Some(Some(bound)) = bounds.get(index) {
                                        if !argument.is_subtype_of(bound, &self.env) {
                                            return Err(TypeError {
                                                message: format!(
                                                    "type argument {} for '{}' does not satisfy its bound",
                                                    index + 1,
                                                    other
                                                ),
                                                span: texpr.span(),
                                            });
                                        }
                                    }
                                }
                            }
                            let mut c_clone = c.clone();
                            if let Type::Class {
                                ref mut type_args, ..
                            } = c_clone
                            {
                                *type_args = resolved_args;
                            }
                            return Ok(c_clone);
                        }
                        if let Some(iface) = self.env.interfaces.get(other) {
                            if let Some(parameters) = self.env.interface_variance.get(other) {
                                if parameters.len() != resolved_args.len() {
                                    return Err(TypeError {
                                        message: format!(
                                            "type '{}' expects {} argument(s), got {}",
                                            other,
                                            parameters.len(),
                                            resolved_args.len()
                                        ),
                                        span: texpr.span(),
                                    });
                                }
                            }
                            if let Some(bounds) = self.env.interface_bounds.get(other) {
                                for (index, argument) in resolved_args.iter().enumerate() {
                                    if let Some(Some(bound)) = bounds.get(index) {
                                        if !argument.is_subtype_of(bound, &self.env) {
                                            return Err(TypeError {
                                                message: format!(
                                                    "type argument {} for '{}' does not satisfy its bound",
                                                    index + 1,
                                                    other
                                                ),
                                                span: texpr.span(),
                                            });
                                        }
                                    }
                                }
                            }
                            let mut iface_clone = iface.clone();
                            if let Type::Interface {
                                ref mut type_args, ..
                            } = iface_clone
                            {
                                *type_args = resolved_args;
                            }
                            return Ok(iface_clone);
                        }
                        if let Some(tr) = self.env.traits.get(other) {
                            if let Some(parameters) = self.env.trait_variance.get(other) {
                                if parameters.len() != resolved_args.len() {
                                    return Err(TypeError {
                                        message: format!(
                                            "type '{}' expects {} argument(s), got {}",
                                            other,
                                            parameters.len(),
                                            resolved_args.len()
                                        ),
                                        span: texpr.span(),
                                    });
                                }
                            }
                            if let Some(bounds) = self.env.trait_bounds.get(other) {
                                for (index, argument) in resolved_args.iter().enumerate() {
                                    if let Some(Some(bound)) = bounds.get(index) {
                                        if !argument.is_subtype_of(bound, &self.env) {
                                            return Err(TypeError {
                                                message: format!(
                                                    "type argument {} for '{}' does not satisfy its bound",
                                                    index + 1,
                                                    other
                                                ),
                                                span: texpr.span(),
                                            });
                                        }
                                    }
                                }
                            }
                            let mut trait_clone = tr.clone();
                            if let Type::Trait {
                                ref mut type_args, ..
                            } = trait_clone
                            {
                                *type_args = resolved_args;
                            }
                            return Ok(trait_clone);
                        }
                        // Default to TypeVar or forward reference
                        Ok(Type::Class {
                            name: other.to_string(),
                            type_args: resolved_args,
                            parent: None,
                            traits: Vec::new(),
                            interfaces: Vec::new(),
                            fields: HashMap::new(),
                            is_sealed: false,
                        })
                    }
                }
            }
            TypeExpr::Record {
                fields, is_open, ..
            } => {
                let mut names = HashSet::new();
                let mut resolved = Vec::with_capacity(fields.len());
                let mut saw_variadic_positional = false;
                let mut saw_keyword_only = false;
                let mut saw_variadic_keyword = false;
                for field in fields {
                    if saw_variadic_keyword {
                        return Err(TypeError {
                            message:
                                "no record or parameter-shape field may follow variadic keyword tail"
                                    .into(),
                            span: field.type_expr.span(),
                        });
                    }
                    if field.is_keyword_only {
                        saw_keyword_only = true;
                    }
                    if saw_variadic_positional && !saw_keyword_only {
                        return Err(TypeError {
                            message:
                                "no fixed positional field may follow variadic positional tail"
                                    .into(),
                            span: field.type_expr.span(),
                        });
                    }
                    if field.is_variadic_positional {
                        if saw_variadic_positional {
                            return Err(TypeError {
                                message:
                                    "record or parameter shape may have only one variadic positional tail"
                                        .into(),
                                span: field.type_expr.span(),
                            });
                        }
                        saw_variadic_positional = true;
                    }
                    if field.is_variadic_keyword {
                        if saw_variadic_keyword {
                            return Err(TypeError {
                                message:
                                    "record or parameter shape may have only one variadic keyword tail"
                                        .into(),
                                span: field.type_expr.span(),
                            });
                        }
                        saw_variadic_keyword = true;
                    }
                    if let Some(name) = &field.name {
                        if !names.insert(name.clone()) {
                            return Err(TypeError {
                                message: format!("duplicate record field '{name}'"),
                                span: field.type_expr.span(),
                            });
                        }
                    }
                    resolved.push((
                        field.name.clone(),
                        self.resolve_type_expr(&field.type_expr)?,
                    ));
                }
                Ok(Type::Record {
                    fields: resolved,
                    is_open: *is_open,
                })
            }
            TypeExpr::Function {
                params,
                return_type,
                ..
            } => {
                let resolved_params = params
                    .iter()
                    .map(|p| self.resolve_type_expr(p))
                    .collect::<Result<Vec<_>, _>>()?;
                let resolved_ret = self.resolve_type_expr(return_type)?;
                Ok(Type::Function {
                    params: resolved_params,
                    return_type: Box::new(resolved_ret),
                })
            }
            TypeExpr::Union { types, .. } => {
                let resolved = types
                    .iter()
                    .map(|t| self.resolve_type_expr(t))
                    .collect::<Result<Vec<_>, _>>()?;
                Ok(Type::make_union(resolved))
            }
            TypeExpr::View {
                mutability, inner, ..
            } => {
                let resolved_inner = self.resolve_type_expr(inner)?;
                Ok(Type::View {
                    mutability: mutability.clone(),
                    inner: Box::new(resolved_inner),
                })
            }
            // Existential quantification hides the concrete implementor but
            // keeps the underlying obligation for subtype and member checks.
            TypeExpr::Existential { interface, .. } => self.resolve_type_expr(interface),
            // A reified type expression keeps the same semantic identity at
            // compile time; reflection supplies the value-level type object.
            TypeExpr::Reification { inner, .. } => self.resolve_type_expr(inner),
            TypeExpr::Match { arms, .. } => {
                let mut resolved_arms = Vec::new();
                for (_, res) in arms {
                    resolved_arms.push(self.resolve_type_expr(res)?);
                }
                Ok(Type::make_union(resolved_arms))
            }
            TypeExpr::Literal { value, .. } => match value {
                LiteralValue::Int(value) => Ok(Type::LiteralInt(*value)),
                LiteralValue::BigInt(_) => Ok(Type::Int),
                LiteralValue::Float(value) => Ok(Type::LiteralFloat(*value)),
                LiteralValue::Complex(_) => Ok(self
                    .env
                    .classes
                    .get("complex")
                    .cloned()
                    .unwrap_or(Type::Float)),
                LiteralValue::Str(value) => Ok(Type::LiteralStr(value.clone())),
                LiteralValue::Bytes(_) => Ok(Type::Class {
                    name: "Bytes".into(),
                    type_args: Vec::new(),
                    parent: None,
                    traits: Vec::new(),
                    interfaces: vec!["Buffer".into(), "Sized".into(), "Container".into()],
                    fields: HashMap::new(),
                    is_sealed: true,
                }),
                LiteralValue::Bool(value) => Ok(Type::LiteralBool(*value)),
                LiteralValue::None => Ok(Type::None),
                LiteralValue::Sentinel(_) => Ok(Type::TypeVar("Sentinel".into())),
                LiteralValue::Ellipsis => Ok(Type::Never),
            },
            TypeExpr::Wildcard(_) => Ok(Type::TypeVar("Any".into())),
            TypeExpr::Never(_) => Ok(Type::Never),
        }
    }
}

fn count_yields(statements: &[Stmt]) -> usize {
    statements
        .iter()
        .map(|stmt| match stmt {
            Stmt::Yield { .. } => 1,
            Stmt::If {
                then_branch,
                elif_branches,
                else_branch,
                ..
            } => {
                then_branch
                    .iter()
                    .map(|s| count_yields(std::slice::from_ref(s)))
                    .sum::<usize>()
                    + elif_branches
                        .iter()
                        .map(|(_, body)| count_yields(body))
                        .sum::<usize>()
                    + else_branch
                        .as_ref()
                        .map(|body| count_yields(body))
                        .unwrap_or(0)
            }
            Stmt::While { body, .. } | Stmt::For { body, .. } => count_yields(body),
            Stmt::Try {
                body,
                handlers,
                finally_body,
                ..
            } => {
                count_yields(body)
                    + handlers
                        .iter()
                        .map(|h| count_yields(&h.body))
                        .sum::<usize>()
                    + finally_body
                        .as_ref()
                        .map(|body| count_yields(body))
                        .unwrap_or(0)
            }
            _ => 0,
        })
        .sum()
}

fn types_may_overlap(left: &Type, right: &Type, env: &TypeEnvironment) -> bool {
    if let Type::View { inner, .. } = left {
        return types_may_overlap(inner, right, env);
    }
    if let Type::View { inner, .. } = right {
        return types_may_overlap(left, inner, env);
    }
    if matches!(left, Type::Never) || matches!(right, Type::Never) {
        return false;
    }
    if matches!(left, Type::TypeVar(name) if name == "Any")
        || matches!(right, Type::TypeVar(name) if name == "Any")
    {
        return true;
    }
    if let Type::Union(parts) = left {
        return parts.iter().any(|part| types_may_overlap(part, right, env));
    }
    if let Type::Union(parts) = right {
        return parts.iter().any(|part| types_may_overlap(left, part, env));
    }
    if left == right {
        return true;
    }
    if left.is_subtype_of(right, env) || right.is_subtype_of(left, env) {
        return true;
    }
    if matches!(left, Type::Function { .. })
        && matches!(right, Type::Trait { name, .. } if name == "Callable")
        || matches!(right, Type::Function { .. })
            && matches!(left, Type::Trait { name, .. } if name == "Callable")
    {
        return true;
    }
    matches!(
        (left, right),
        (Type::Trait { .. }, Type::Class { .. })
            | (Type::Class { .. }, Type::Trait { .. })
            | (Type::Trait { .. }, Type::Trait { .. })
            | (Type::Interface { .. }, Type::Class { .. })
            | (Type::Class { .. }, Type::Interface { .. })
            | (Type::Interface { .. }, Type::Interface { .. })
    )
}

/// Whether every normal fall-through path through a statement list reaches a
/// yield.  Loops and conditionals without an unconditional else path may run
/// zero times, so they cannot establish the context-manager suspension point.
fn yield_guaranteed(statements: &[Stmt]) -> bool {
    for statement in statements {
        match statement {
            Stmt::Yield { .. } => return true,
            Stmt::If {
                then_branch,
                elif_branches,
                else_branch: Some(else_branch),
                ..
            } => {
                if yield_guaranteed(then_branch)
                    && elif_branches
                        .iter()
                        .all(|(_, branch)| yield_guaranteed(branch))
                    && yield_guaranteed(else_branch)
                {
                    return true;
                }
            }
            Stmt::Try {
                finally_body: Some(finally_body),
                ..
            } if yield_guaranteed(finally_body) => {
                return true;
            }
            Stmt::Return { .. } => return false,
            Stmt::For { .. } | Stmt::While { .. } => {}
            _ => {}
        }
    }
    false
}

#[cfg(test)]
mod tests {
    use super::*;
    use lucid_syntax::parse;

    #[test]
    fn test_single_inheritance_rule_enforced() {
        let src = r#"
class Base1:
    x: int

class Base2:
    y: int

trait Reusable:
    pass

class Child(Reusable, Base1, Base2):
    z: int
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module);
        assert!(err.is_err());
        assert!(err.unwrap_err().message.contains("multiple class parents"));
    }

    #[test]
    fn test_final_class_cannot_be_inherited() {
        let module =
            parse("final class Base:\n    pass\n\nclass Child(Base):\n    pass\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("final and cannot be inherited"));
    }

    #[test]
    fn test_final_methods_cannot_be_overridden() {
        let module = parse(
            "class Base:\n    final def save(self) -> int:\n        return 1\nclass Child(Base):\n    override def save(self) -> int:\n        return 2\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("final and cannot be overridden"));

        let module = parse(
            "class Base:\n    final classmethod make(cls) -> int:\n        return 1\nclass Child(Base):\n    override classmethod make(cls) -> int:\n        return 2\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("final and cannot be overridden"));

        let module = parse(
            "class Grandparent:\n    final def save(self) -> int:\n        return 1\nclass Parent(Grandparent):\n    pass\nclass Child(Parent):\n    override def save(self) -> int:\n        return 2\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("final and cannot be overridden"));
    }

    #[test]
    fn test_final_methods_must_have_bodies() {
        let module = parse("class Base:\n    final def save(self) -> int\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("final method 'Base.save' must have a body"));

        let module = parse("class Base:\n    final classmethod make(cls) -> int\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("final method 'Base.make' must have a body"));
    }

    #[test]
    fn test_final_obligation_fields_are_rejected() {
        let module = parse("trait Named:\n    final name: str\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err
            .message
            .contains("final member 'name' must have an implementation"));

        let module = parse("interface Named:\n    final name: str\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err
            .message
            .contains("final member 'name' must have an implementation"));
    }

    #[test]
    fn test_final_class_rule_does_not_depend_on_declaration_order() {
        let module =
            parse("class Child(Base):\n    pass\n\nfinal class Base:\n    pass\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("final and cannot be inherited"));
    }

    #[test]
    fn test_cyclic_class_inheritance_is_rejected() {
        let module =
            parse("class First(Second):\n    pass\n\nclass Second(First):\n    pass\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker
            .check_module(&module)
            .expect_err("cyclic inheritance must be rejected");
        assert!(err.message.contains("cyclic class inheritance"));
    }

    #[test]
    fn test_override_rule_does_not_depend_on_declaration_order() {
        let module = parse(
            "class Child(Base):\n    def value(self) -> int:\n        return 2\n\nclass Base:\n    def value(self) -> int:\n        return 1\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("overrides an inherited member"));
    }

    #[test]
    fn test_mutability_subtyping() {
        let checker = TypeChecker::new();
        let point_mutable = Type::Class {
            name: "Point".to_string(),
            type_args: Vec::new(),
            parent: None,
            traits: Vec::new(),
            interfaces: Vec::new(),
            fields: HashMap::new(),
            is_sealed: false,
        };

        let point_read_only = Type::View {
            mutability: MutabilityView::ReadOnly,
            inner: Box::new(point_mutable.clone()),
        };

        let point_immutable = Type::View {
            mutability: MutabilityView::Immutable,
            inner: Box::new(point_mutable.clone()),
        };

        // Point <: &Point (mutable can be passed to read-only parameter)
        assert!(point_mutable.is_subtype_of(&point_read_only, &checker.env));
        // !Point <: &Point (immutable can be passed to read-only parameter)
        assert!(point_immutable.is_subtype_of(&point_read_only, &checker.env));
        // &Point is NOT a subtype of Point (read-only cannot be passed where mutation is required)
        assert!(!point_read_only.is_subtype_of(&point_mutable, &checker.env));
    }

    #[test]
    fn test_exhaustive_match_on_union() {
        let src = r#"
type Shape = int | str

def render(s: Shape) -> int:
    match s:
        case int:
            return 1
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module);
        assert!(err.is_err());
        assert!(err.unwrap_err().message.contains("non-exhaustive match"));
        let guarded = parse(
            "type Shape = int | str\ndef render(s: Shape) -> int:\n    match s:\n        case int:\n            return 1\n        case _ if false:\n            return 0\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&guarded).unwrap_err();
        assert!(err.message.contains("non-exhaustive match"));
        let aliased = parse(
            "type Shape = int | str\ndef render(s: Shape) -> int:\n    match s as subject:\n        case int:\n            return subject\n        case str:\n            return 0\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        assert!(checker.check_module(&aliased).is_ok());
    }

    #[test]
    fn test_class_method_attribute_type() {
        let src = r#"
class Circle:
    radius: int

dispatch def area(c: Circle) -> int:
    return c.radius * c.radius
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        let res = checker.check_module(&module);
        assert!(res.is_ok(), "{:?}", res);

        let module = parse(
            "interface Base:\n    pass\ninterface Derived(Base):\n    pass\nclass Widget(Derived):\n    pass\nvalue: Base = Widget()\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        assert!(checker.check_module(&module).is_ok());

        let module = parse(
            "trait Label:\n    def label(self, prefix: str) -> str\n\ndef use(value: Label) -> str:\n    return value.label(prefix=1)\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error.message.contains("incompatible type"));

        let module = parse(
            "trait HasId:\n    id: int\n\ndef read(value: HasId) -> int:\n    return value.id\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        assert!(checker.check_module(&module).is_ok());

        let module = parse(
            "trait Mutable:\n    setter value(self, new_value: int):\n        pass\n\ndef set_value(target: Mutable):\n    target.value = \"wrong\"\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error.message.contains("setter 'value'"));

        let module =
            parse("class Closed:\n    value: int\nitem = Closed(1)\nresult = item.missing\n")
                .unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error.message.contains("has no member 'missing'"));
        let module =
            parse("class Closed:\n    value: int\nitem = Closed(1)\nitem.missing = 1\n").unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error.message.contains("no writable member 'missing'"));
        let module =
            parse("class Cache:\n    _entries: dict[str, int]\ncache = Cache({\"x\": 1})\ncache._entries = {\"y\": 2}\n").unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error.message.contains("member '_entries' is private"));
        let module =
            parse("class Cache:\n    _entries: dict[str, int]\n    def reset(self):\n        self._entries = {\"x\": 1}\ncache = Cache({\"x\": 1})\ncache.reset()\n").unwrap();
        let mut checker = TypeChecker::new();
        checker
            .check_module(&module)
            .expect("private field writes inside the declaring class should type check");
        let module = parse("value = 1\nresult = value.missing\n").unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error.message.contains("has no attribute 'missing'"));
        let module = parse("binary = str.bin(10)\n").unwrap();
        let mut checker = TypeChecker::new();
        checker
            .check_module(&module)
            .expect("string base factories should be statically callable");
        let module = parse("parts = \"a b\".split(\" \", \"extra\")\n").unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error
            .message
            .contains("invalid argument count for str.split"));
        let module = parse("value = \"x\".replace(1, \"y\")\n").unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error
            .message
            .contains("str.replace() argument has incompatible type"));
        let module = parse("a = float.inf\nb = float.nan\nc = int.inf\nd = complex.nan\n").unwrap();
        let mut checker = TypeChecker::new();
        checker
            .check_module(&module)
            .expect("numeric class constants should be statically visible");
        let module = parse("items = []\nitems.append()\n").unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error.message.contains("invalid argument count for append"));
    }

    #[test]
    fn test_super_attribute_is_type_checkable() {
        let src = r#"
class Base:
    def value(self) -> int:
        return 1

class Child(Base):
    override def value(self) -> int:
        return super.value() + 1
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        let result = checker.check_module(&module);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn test_classmethod_override_modifier_is_preserved() {
        let src = r#"
class Base:
    classmethod make(cls) -> int:
        return 1

class Child(Base):
    override classmethod make(cls) -> int:
        return super.make() + 1
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        assert!(checker.check_module(&module).is_ok());
    }

    #[test]
    fn test_let_destructuring_rejects_wrong_class() {
        let module =
            parse("class Pair:\n    left: int\n    right: int\nlet Pair(a, b) = 1\n").unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error.message.contains("cannot destructure"));
    }

    #[test]
    fn test_parameter_destructuring_binds_nested_names() {
        let module = parse("class Pair:\n    left: int\n    right: int\ndef total(Pair(a, b): Pair) -> int:\n    return a + b\n").unwrap();
        let mut checker = TypeChecker::new();
        let result = checker.check_module(&module);
        assert!(result.is_ok(), "{result:?}");
    }

    #[test]
    fn test_match_type_alias_preserves_result_types() {
        let module = parse("type Choice = match int:\n    case int: str\n    case _: none\nvalue: Choice = \"ok\"\n").unwrap();
        let mut checker = TypeChecker::new();
        assert!(checker.check_module(&module).is_ok());
    }

    #[test]
    fn test_conditionals_require_exact_bool() {
        let src = "if 1:\n    pass\n";
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("condition must be bool"));

        let len_only = parse("class SizedOnly:\n    def __len__(self) -> int:\n        return 1\nif SizedOnly():\n    pass\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&len_only).unwrap_err();
        assert!(err.message.contains("condition must be bool"));

        let truthy = parse("class Flag:\n    value: bool\n    factory __init__(cls, value: bool):\n        return construct(value)\n    def __bool__(self) -> bool:\n        return self.value\nif Flag(true):\n    pass\nwhile Flag(false):\n    pass\nitems = [x for x in [1] if Flag(true)]\n").unwrap();
        let mut checker = TypeChecker::new();
        checker
            .check_module(&truthy)
            .expect("__bool__ should satisfy condition positions");

        let module = parse("value: int = 1\nvalue += \"bad\"\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(
            err.message.contains("unsupported operands")
                || err.message.contains("augmented assignment")
        );
        let module = parse("result = 1()\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("not callable"));
        let module = parse("result = -\"bad\"\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("unsupported operand"));
        let module = parse("result = 1 in 2\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("not supported"));
        let module = parse("result = 1?\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("outside a function"));
        let module = parse("result = \"bad\" / 2\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("unsupported operands for /"));
    }

    #[test]
    fn arithmetic_rejects_bool_none_and_unrelated_same_types() {
        let valid = parse("number = 1 + 2\ntext = \"a\" + \"b\"\nfloaty = 1 + 2.0\n").unwrap();
        TypeChecker::new()
            .check_module(&valid)
            .expect("numeric arithmetic and string concatenation should type check");

        for source in [
            "value = true + true\n",
            "value = true - false\n",
            "value = true * false\n",
            "value = none + none\n",
            "value = {1} + {2}\n",
            "value = {\"a\": 1} + {\"b\": 2}\n",
        ] {
            let module = parse(source).unwrap();
            let err = TypeChecker::new().check_module(&module).unwrap_err();
            assert!(
                err.message.contains("unsupported operands"),
                "{source}: {}",
                err.message
            );
        }
    }

    #[test]
    fn anonymous_function_parameters_require_annotations_or_expected_type() {
        let no_expected = parse("f = def(x): x * 3\n").unwrap();
        let error = TypeChecker::new().check_module(&no_expected).unwrap_err();
        assert!(error
            .message
            .contains("needs an annotation or an expected function type"));

        let annotated_assignment = parse("f: (int) -> int = def(x): x * 3\nresult = f(4)\n")
            .unwrap();
        TypeChecker::new()
            .check_module(&annotated_assignment)
            .expect("annotation should supply anonymous parameter type");

        let call_site = parse(
            "def apply(f: (int) -> int) -> int:\n    return f(4)\nresult = apply(def(x): x * 3)\n",
        )
        .unwrap();
        TypeChecker::new()
            .check_module(&call_site)
            .expect("call parameter should supply anonymous parameter type");
    }

    #[test]
    fn floor_division_and_modulo_type_numeric_operands() {
        let module = parse(
            "ii_floor = 5 // 2\nff_floor = 5.0 // 2.0\nif_floor = 5 // 2.0\nfi_floor = 5.0 // 2\nii_mod = 5 % 2\nff_mod = 5.0 % 2.0\nif_mod = 5 % 2.0\nfi_mod = 5.0 % 2\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        checker
            .check_module(&module)
            .expect("numeric floor division and modulo should type check");

        for name in ["ii_floor", "ii_mod"] {
            assert_eq!(
                checker.env.variables.get(name).map(|(ty, _)| ty),
                Some(&Type::Int)
            );
        }

        for name in [
            "ff_floor", "if_floor", "fi_floor", "ff_mod", "if_mod", "fi_mod",
        ] {
            assert_eq!(
                checker.env.variables.get(name).map(|(ty, _)| ty),
                Some(&Type::Float)
            );
        }

        let module = parse("formatted = \"%s\" % \"value\"\n").unwrap();
        let err = TypeChecker::new().check_module(&module).unwrap_err();
        assert!(err.message.contains("unsupported operands for %"));
    }

    #[test]
    fn test_final_variable_cannot_be_reassigned() {
        let module = parse("final answer: int = 1\nanswer = 2\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("final variable 'answer'"));
    }

    #[test]
    fn test_final_field_cannot_be_reassigned() {
        let src = r#"
class User:
    final id: int

u = User(1)
u.id = 2
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("final field 'id'"));
    }

    #[test]
    fn test_private_member_access_is_checked() {
        let module =
            parse("class Secret:\n    _value: int\n\ns = Secret(1)\nresult = s._value\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("member '_value' is private"));
    }

    #[test]
    fn test_async_functions_require_await_for_result_type() {
        let good =
            parse("async def load() -> int:\n    return 1\nresult: int = await load()\n").unwrap();
        let mut checker = TypeChecker::new();
        assert!(checker.check_module(&good).is_ok());

        let bad = parse("async def load() -> int:\n    return 1\nresult: int = load()\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&bad).unwrap_err();
        assert!(err.message.contains("type mismatch"));

        let bad_await = parse("result = await 1\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&bad_await).unwrap_err();
        assert!(err.message.contains("expected Future"));
    }

    #[test]
    fn test_class_variable_type_is_checked_in_members() {
        let module = parse(
            "class Counter:\n    classvar count: int = 0\n    classmethod label(cls) -> str:\n        return cls.count\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("return type mismatch"));
    }

    #[test]
    fn test_required_trait_field_is_not_counted_as_implemented_by_trait() {
        let module =
            parse("trait HasId:\n    id: int\n\nclass Missing(HasId):\n    pass\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("required member 'id'"));

        let module = parse(
            "class Widget:\n    value: int\n\ntrait Label:\n    def label(self, prefix: str) -> str:\n        return prefix\n\nimplement Label for Widget:\n    def label(self, prefix: str) -> str:\n        return prefix\n\nw = Widget(1)\nresult = w.label(prefix=1)\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("incompatible type"));

        let module = parse(
            "trait Label:\n    def label(self, prefix: str) -> str:\n        return prefix\n\nclass Labeled(Label):\n    value: int\n\nitem = Labeled(1)\nresult = item.label(prefix=1)\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("incompatible type"));

        let module = parse(
            "trait Base:\n    def required(self) -> int\n\ntrait Derived(Base):\n    pass\n\nclass Missing(Derived):\n    pass\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("required member 'required'"));

        let module = parse(
            "interface Describable:\n    def describe(prefix: str) -> str\n\nclass Widget:\n    value: int\n\nimplement Describable for Widget:\n    def describe(self, prefix: str) -> str:\n        return prefix\n\nw = Widget(1)\nresult = w.describe(prefix=1)\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("incompatible type"));

        let module = parse(
            "interface Describable:\n    def describe(prefix: str) -> str\n\nclass Widget:\n    value: int\n\nimplement Describable for Widget:\n    def describe(self, prefix: str) -> str:\n        return prefix\n\nw: Describable = Widget(1)\nresult = w.describe(prefix=1)\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("incompatible type"));
    }

    #[test]
    fn test_delete_ends_name_lifetime() {
        let module = parse("value: int = 1\ndel value\nvalue = 2\n").unwrap();
        let mut checker = TypeChecker::new();
        assert!(checker.check_module(&module).is_ok());

        let bad = parse("value: int = 1\ndel value\nresult = value\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&bad).unwrap_err();
        assert!(err.message.contains("undefined variable 'value'"));
    }

    #[test]
    fn black_hole_assignment_target_is_not_readable() {
        let module = parse("_ = 1\nother = 2\n").unwrap();
        let mut checker = TypeChecker::new();
        checker
            .check_module(&module)
            .expect("discard assignment should not bind a readable name");

        for source in [
            "_ = 1\nvalue = _\n",
            "left, _ = [1, 2]\nvalue = _\n",
            "values = [_ for _ in [1, 2]]\n",
        ] {
            let module = parse(source).unwrap();
            let mut checker = TypeChecker::new();
            let err = checker.check_module(&module).unwrap_err();
            assert!(err.message.contains("black-hole assignment target"));
        }
    }

    #[test]
    fn test_contextmanager_requires_one_yield() {
        let module = parse("contextmanager def bad():\n    pass\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("must yield exactly once"));
        let module =
            parse("contextmanager def conditional(flag: bool):\n    if flag:\n        yield 1\n")
                .unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("on every path"));
        let module =
            parse("class Resource:\n    value: int\nwith Resource(1):\n    pass\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("not a contextmanager"));
        let module = parse("with 1:\n    pass\n").unwrap();
        let mut checker = TypeChecker::new();
        let err = checker.check_module(&module).unwrap_err();
        assert!(err.message.contains("must be a contextmanager"));
    }

    #[test]
    fn bytes_have_distinct_immutable_type_and_integer_indexing() {
        let module = parse("data: Bytes = b\"AB\"\nfirst: int = data[0]\n").unwrap();
        let mut checker = TypeChecker::new();
        assert!(checker.check_module(&module).is_ok());
        let expr = match &module.statements[0] {
            Stmt::VarDef {
                value: Some(expr), ..
            } => expr,
            _ => unreachable!(),
        };
        assert!(matches!(
            checker.type_of_expr(expr),
            Ok(Type::Class { ref name, .. }) if name == "Bytes"
        ));
    }

    #[test]
    fn boolean_literal_annotations_preserve_exact_value_and_widen_to_bool() {
        let module = parse("flag: true = true\n").unwrap();
        let mut checker = TypeChecker::new();
        checker
            .check_module(&module)
            .expect("literal bool should check");
        let annotation = match &module.statements[0] {
            Stmt::VarDef {
                type_annotation: Some(annotation),
                ..
            } => annotation,
            _ => unreachable!(),
        };
        assert_eq!(
            checker.resolve_type_expr(annotation).unwrap(),
            Type::LiteralBool(true)
        );
        assert!(Type::LiteralBool(true).is_subtype_of(&Type::Bool, &checker.env));
        assert!(!Type::LiteralBool(true).is_subtype_of(&Type::LiteralBool(false), &checker.env));
    }

    #[test]
    fn integer_literal_annotations_check_the_written_value() {
        let mut checker = TypeChecker::new();
        checker
            .check_module(&parse("answer: 42 = 42\n").unwrap())
            .expect("matching integer literal should check");

        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("answer: 42 = 41\n").unwrap())
            .expect_err("different integer literal must fail");
        assert!(error.message.contains("type mismatch"));
    }

    #[test]
    fn float_literal_annotations_check_the_written_value() {
        let mut checker = TypeChecker::new();
        checker
            .check_module(&parse("ratio: 1.5 = 1.5\n").unwrap())
            .expect("matching float literal should check");
        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("ratio: 1.5 = 2.5\n").unwrap())
            .expect_err("different float literal must fail");
        assert!(error.message.contains("type mismatch"));
    }

    #[test]
    fn string_literal_annotations_check_the_written_value() {
        let mut checker = TypeChecker::new();
        checker
            .check_module(&parse("status: \"ok\" = \"ok\"\n").unwrap())
            .expect("matching string literal should check");
        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("status: \"ok\" = \"error\"\n").unwrap())
            .expect_err("different string literal must fail");
        assert!(error.message.contains("type mismatch"));
    }

    #[test]
    fn literal_types_widen_for_primitive_and_trait_compatibility() {
        let checker = TypeChecker::new();
        assert!(Type::LiteralStr("ok".into()).is_subtype_of(&Type::Str, &checker.env));
        assert!(Type::LiteralBool(true).is_subtype_of(&Type::Bool, &checker.env));
        assert!(Type::LiteralFloat(1.5).is_subtype_of(&Type::Float, &checker.env));
        assert!(Type::LiteralInt(1).is_subtype_of(&Type::Int, &checker.env));
        assert!(!Type::LiteralStr("ok".into())
            .is_subtype_of(&Type::LiteralStr("no".into()), &checker.env));
    }

    #[test]
    fn indexed_assignment_enforces_buffer_mutability_and_element_types() {
        let mut checker = TypeChecker::new();
        let bytes = parse("data: Bytes = bytes([65])\ndata[0] = 66\n").unwrap();
        let error = checker
            .check_module(&bytes)
            .expect_err("immutable Bytes must reject indexed assignment");
        assert!(error.message.contains("immutable Bytes"));

        let mut checker = TypeChecker::new();
        let bad = parse("data = bytearray([65])\ndata[0] = \"x\"\n").unwrap();
        let error = checker
            .check_module(&bad)
            .expect_err("byte buffers must contain integer elements");
        assert!(error.message.contains("byte buffer elements must be int"));

        let mut checker = TypeChecker::new();
        let good = parse("data = bytearray([65])\ndata[0] = 66\n").unwrap();
        checker
            .check_module(&good)
            .expect("integer byte assignment should check");

        let mut checker = TypeChecker::new();
        let readonly = parse("data: ~list[int] = [65]\ndata[0] = 66\n").unwrap();
        let error = checker
            .check_module(&readonly)
            .expect_err("read-only views must reject indexed assignment");
        assert!(error.message.contains("read-only or immutable view"));
    }

    #[test]
    fn binary_conversion_builtins_require_supported_inputs() {
        let valid = parse(
            r#"
class Packet:
    storage: ByteArray
    def __buffer__(self) -> MemoryView:
        return memoryview(self.storage)

packet = Packet(bytearray([65]))
view: MemoryView = memoryview(packet)
raw: Bytes = bytes(view)
mutable: ByteArray = bytearray(packet)
from_text: Bytes = bytes("A")
"#,
        )
        .unwrap();
        TypeChecker::new()
            .check_module(&valid)
            .expect("buffer conversions should check");

        let mut checker = TypeChecker::new();
        let bad_bytes = parse("bad = bytes(1)\n").unwrap();
        let error = checker
            .check_module(&bad_bytes)
            .expect_err("bytes() should reject non-buffer scalars");
        assert!(error.message.contains("bytes() argument must be"));

        let mut checker = TypeChecker::new();
        let bad_memoryview = parse("bad = memoryview(\"abc\")\n").unwrap();
        let error = checker
            .check_module(&bad_memoryview)
            .expect_err("memoryview() should reject strings");
        assert!(error.message.contains("memoryview() argument must be a buffer"));

        let mut checker = TypeChecker::new();
        let bad_list = parse("bad = bytearray([\"x\"])\n").unwrap();
        let error = checker
            .check_module(&bad_list)
            .expect_err("bytearray() should reject non-integer lists");
        assert!(error.message.contains("bytearray() argument must be"));
    }

    #[test]
    fn test_intersection_and_negation_types_resolve() {
        let src = r#"
trait Named:
    name: str

trait Identified:
    id: int

def render(value: Named & Identified) -> none:
    pass

def reject(value: not int) -> none:
    pass
"#;
        let module = parse(src).unwrap();
        let mut checker = TypeChecker::new();
        assert!(checker.check_module(&module).is_ok());
        assert!(matches!(
            checker.resolve_type_expr(&TypeExpr::Named {
                name: "__not__".into(),
                args: vec![TypeExpr::Named {
                    name: "int".into(),
                    args: vec![],
                    span: Span::default()
                }],
                span: Span::default(),
            }),
            Ok(Type::Negation(_))
        ));
    }

    #[test]
    fn test_shape_types_preserve_exact_dimensions() {
        let checker = TypeChecker::new();
        let shape = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "typing.shape".into(),
                args: vec![
                    TypeExpr::Literal {
                        value: LiteralValue::Int(2),
                        span: Span::default(),
                    },
                    TypeExpr::Literal {
                        value: LiteralValue::Int(3),
                        span: Span::default(),
                    },
                ],
                span: Span::default(),
            })
            .unwrap();
        assert_eq!(shape, Type::Shape(vec![Some(2), Some(3)]));
        assert!(shape.is_subtype_of(&Type::Shape(vec![Some(2), None]), &checker.env));
        assert!(!shape.is_subtype_of(&Type::Shape(vec![Some(3), Some(2)]), &checker.env));
        let unknown = Type::Shape(vec![None, Some(3)]);
        assert!(unknown.is_subtype_of(&Type::Shape(vec![None, Some(3)]), &checker.env));
        assert!(!unknown.is_subtype_of(&Type::Shape(vec![Some(2), Some(3)]), &checker.env));
        let list = Type::Class {
            name: "list".into(),
            type_args: vec![],
            parent: None,
            traits: vec![],
            interfaces: vec![],
            fields: HashMap::new(),
            is_sealed: false,
        };
        assert!(!shape.is_subtype_of(&list, &checker.env));
        let immutable_list = Type::View {
            mutability: MutabilityView::Immutable,
            inner: Box::new(list.clone()),
        };
        assert!(shape.is_subtype_of(&immutable_list, &checker.env));
        let string_list = Type::Class {
            name: "list".into(),
            type_args: vec![Type::Str],
            parent: None,
            traits: vec![],
            interfaces: vec![],
            fields: HashMap::new(),
            is_sealed: false,
        };
        let immutable_string_list = Type::View {
            mutability: MutabilityView::Immutable,
            inner: Box::new(string_list),
        };
        assert!(!shape.is_subtype_of(&immutable_string_list, &checker.env));
    }

    #[test]
    fn test_shape_rejects_invalid_or_negative_dimensions() {
        let checker = TypeChecker::new();
        let invalid = TypeExpr::Named {
            name: "typing.shape".into(),
            args: vec![TypeExpr::Named {
                name: "str".into(),
                args: vec![],
                span: Span::default(),
            }],
            span: Span::default(),
        };
        let error = checker.resolve_type_expr(&invalid).unwrap_err();
        assert!(error
            .message
            .contains("shape dimensions must be int literals"));

        let negative = TypeExpr::Named {
            name: "typing.shape".into(),
            args: vec![TypeExpr::Literal {
                value: LiteralValue::Int(-1),
                span: Span::default(),
            }],
            span: Span::default(),
        };
        let error = checker.resolve_type_expr(&negative).unwrap_err();
        assert!(error.message.contains("cannot be negative"));
    }

    #[test]
    fn parameter_shape_variadic_zones_are_ordered() {
        let mut checker = TypeChecker::new();
        checker
            .check_module(
                &parse("type Shape = (c: int, /, a: int, int, ..., *, b: int, _: int, ...)\n")
                    .unwrap(),
            )
            .expect("documented parameter shape should check");

        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("type Bad = (int, ..., bool)\n").unwrap())
            .unwrap_err();
        assert!(error
            .message
            .contains("no fixed positional field may follow"));

        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("type Bad = (*, _: int, ..., b: int)\n").unwrap())
            .unwrap_err();
        assert!(error
            .message
            .contains("no record or parameter-shape field may follow"));
    }

    #[test]
    fn test_class_type_arguments_honor_definition_site_variance() {
        let mut checker = TypeChecker::new();
        let module = parse("class Animal:\n    pass\nclass Dog(Animal):\n    pass\nclass Box[+T: Animal]:\n    pass\ninterface Read[+T]:\n    pass\ntrait Sink[-T]:\n    pass\n").unwrap();
        checker.check_module(&module).unwrap();
        let box_dog = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Box".into(),
                args: vec![TypeExpr::Named {
                    name: "Dog".into(),
                    args: vec![],
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap();
        let box_animal = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Box".into(),
                args: vec![TypeExpr::Named {
                    name: "Animal".into(),
                    args: vec![],
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap();
        assert!(box_dog.is_subtype_of(&box_animal, &checker.env));
        assert!(!box_animal.is_subtype_of(&box_dog, &checker.env));
        let invalid = checker.resolve_type_expr(&TypeExpr::Named {
            name: "Box".into(),
            args: vec![TypeExpr::Named {
                name: "str".into(),
                args: vec![],
                span: Span::default(),
            }],
            span: Span::default(),
        });
        assert!(invalid
            .unwrap_err()
            .message
            .contains("does not satisfy its bound"));
        let wrong_class_arity = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Box".into(),
                args: vec![],
                span: Span::default(),
            })
            .unwrap_err();
        assert!(wrong_class_arity.message.contains("expects 1 argument"));
        let read_dog = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Read".into(),
                args: vec![TypeExpr::Named {
                    name: "Dog".into(),
                    args: vec![],
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap();
        let read_animal = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Read".into(),
                args: vec![TypeExpr::Named {
                    name: "Animal".into(),
                    args: vec![],
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap();
        assert!(read_dog.is_subtype_of(&read_animal, &checker.env));
        let sink_dog = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Sink".into(),
                args: vec![TypeExpr::Named {
                    name: "Dog".into(),
                    args: vec![],
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap();
        let sink_animal = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Sink".into(),
                args: vec![TypeExpr::Named {
                    name: "Animal".into(),
                    args: vec![],
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap();
        assert!(sink_animal.is_subtype_of(&sink_dog, &checker.env));
    }

    #[test]
    fn test_parameterized_type_aliases_substitute_arguments() {
        let mut checker = TypeChecker::new();
        let module = parse("type Box[T] = list[T]\n").unwrap();
        checker.check_module(&module).unwrap();
        let resolved = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Box".into(),
                args: vec![TypeExpr::Named {
                    name: "int".into(),
                    args: vec![],
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap();
        assert!(
            matches!(resolved, Type::Class { name, type_args, .. } if name == "list" && type_args == vec![Type::Int])
        );
        let wrong_arity = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Box".into(),
                args: vec![],
                span: Span::default(),
            })
            .unwrap_err();
        assert!(wrong_arity.message.contains("expects 1 argument"));
        checker
            .check_module(&parse("type Answer = int\n").unwrap())
            .unwrap();
        let unexpected_args = checker
            .resolve_type_expr(&TypeExpr::Named {
                name: "Answer".into(),
                args: vec![TypeExpr::Named {
                    name: "str".into(),
                    args: vec![],
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap_err();
        assert!(unexpected_args
            .message
            .contains("expects no type arguments"));
    }

    #[test]
    fn test_generic_function_infers_type_argument_for_return() {
        let mut checker = TypeChecker::new();
        let module =
            parse("def identity[T](x: T) -> T:\n    return x\nresult = identity(7)\n").unwrap();
        checker.check_module(&module).unwrap();
        let result = checker
            .type_of_expr(&Expr::Call {
                func: Box::new(Expr::Ident {
                    name: "identity".into(),
                    span: Span::default(),
                }),
                args: vec![Arg {
                    name: None,
                    value: Expr::Literal {
                        value: LiteralValue::Int(7),
                        span: Span::default(),
                    },
                    is_spread: false,
                    is_dict_spread: false,
                    is_gather_spread: false,
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap();
        assert_eq!(result, Type::Int);
        let named_generic =
            parse("def choose[T, U](first: T, second: U) -> T:\n    return first\n").unwrap();
        let mut named_generic_checker = TypeChecker::new();
        named_generic_checker.check_module(&named_generic).unwrap();
        let named_result = named_generic_checker
            .type_of_expr(&Expr::Call {
                func: Box::new(Expr::Ident {
                    name: "choose".into(),
                    span: Span::default(),
                }),
                args: vec![
                    Arg {
                        name: Some("second".into()),
                        value: Expr::Literal {
                            value: LiteralValue::Str("x".into()),
                            span: Span::default(),
                        },
                        is_spread: false,
                        is_dict_spread: false,
                        is_gather_spread: false,
                        span: Span::default(),
                    },
                    Arg {
                        name: Some("first".into()),
                        value: Expr::Literal {
                            value: LiteralValue::Int(1),
                            span: Span::default(),
                        },
                        is_spread: false,
                        is_dict_spread: false,
                        is_gather_spread: false,
                        span: Span::default(),
                    },
                ],
                span: Span::default(),
            })
            .unwrap();
        assert_eq!(named_result, Type::Int);
        let nested = parse("def first[T](xs: list[T]) -> T:\n    return xs[0]\n").unwrap();
        let mut nested_checker = TypeChecker::new();
        nested_checker.check_module(&nested).unwrap();
        nested_checker.env.variables.insert(
            "xs".into(),
            (
                Type::Class {
                    name: "list".into(),
                    type_args: vec![Type::Int],
                    parent: None,
                    traits: vec![],
                    interfaces: vec![],
                    fields: HashMap::new(),
                    is_sealed: false,
                },
                MutabilityView::Mutable,
            ),
        );
        let nested_result = nested_checker
            .type_of_expr(&Expr::Call {
                func: Box::new(Expr::Ident {
                    name: "first".into(),
                    span: Span::default(),
                }),
                args: vec![Arg {
                    name: None,
                    value: Expr::Ident {
                        name: "xs".into(),
                        span: Span::default(),
                    },
                    is_spread: false,
                    is_dict_spread: false,
                    is_gather_spread: false,
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap();
        assert_eq!(nested_result, Type::Int);
        let repeated = parse("def same[T](a: T, b: T) -> T:\n    return a\n").unwrap();
        let mut repeated_checker = TypeChecker::new();
        repeated_checker.check_module(&repeated).unwrap();
        let conflict = repeated_checker
            .type_of_expr(&Expr::Call {
                func: Box::new(Expr::Ident {
                    name: "same".into(),
                    span: Span::default(),
                }),
                args: vec![
                    Arg {
                        name: None,
                        value: Expr::Literal {
                            value: LiteralValue::Int(1),
                            span: Span::default(),
                        },
                        is_spread: false,
                        is_dict_spread: false,
                        is_gather_spread: false,
                        span: Span::default(),
                    },
                    Arg {
                        name: None,
                        value: Expr::Literal {
                            value: LiteralValue::Str("x".into()),
                            span: Span::default(),
                        },
                        is_spread: false,
                        is_dict_spread: false,
                        is_gather_spread: false,
                        span: Span::default(),
                    },
                ],
                span: Span::default(),
            })
            .unwrap_err();
        assert!(conflict.message.contains("conflicting type arguments"));
        let missing = repeated_checker
            .type_of_expr(&Expr::Call {
                func: Box::new(Expr::Ident {
                    name: "same".into(),
                    span: Span::default(),
                }),
                args: vec![],
                span: Span::default(),
            })
            .unwrap_err();
        assert!(missing.message.contains("requires at least 2 argument"));
        let missing_named = repeated_checker
            .type_of_expr(&Expr::Call {
                func: Box::new(Expr::Ident {
                    name: "same".into(),
                    span: Span::default(),
                }),
                args: vec![Arg {
                    name: Some("b".into()),
                    value: Expr::Literal {
                        value: LiteralValue::Int(2),
                        span: Span::default(),
                    },
                    is_spread: false,
                    is_dict_spread: false,
                    is_gather_spread: false,
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap_err();
        assert!(missing_named
            .message
            .contains("missing required argument 'a'"));
        let extra = repeated_checker
            .type_of_expr(&Expr::Call {
                func: Box::new(Expr::Ident {
                    name: "same".into(),
                    span: Span::default(),
                }),
                args: vec![
                    Arg {
                        name: None,
                        value: Expr::Literal {
                            value: LiteralValue::Int(1),
                            span: Span::default(),
                        },
                        is_spread: false,
                        is_dict_spread: false,
                        is_gather_spread: false,
                        span: Span::default(),
                    },
                    Arg {
                        name: None,
                        value: Expr::Literal {
                            value: LiteralValue::Int(2),
                            span: Span::default(),
                        },
                        is_spread: false,
                        is_dict_spread: false,
                        is_gather_spread: false,
                        span: Span::default(),
                    },
                    Arg {
                        name: None,
                        value: Expr::Literal {
                            value: LiteralValue::Int(3),
                            span: Span::default(),
                        },
                        is_spread: false,
                        is_dict_spread: false,
                        is_gather_spread: false,
                        span: Span::default(),
                    },
                ],
                span: Span::default(),
            })
            .unwrap_err();
        assert!(extra.message.contains("accepts at most 2 argument"));
        let unknown_name = repeated_checker
            .type_of_expr(&Expr::Call {
                func: Box::new(Expr::Ident {
                    name: "same".into(),
                    span: Span::default(),
                }),
                args: vec![Arg {
                    name: Some("missing".into()),
                    value: Expr::Literal {
                        value: LiteralValue::Int(1),
                        span: Span::default(),
                    },
                    is_spread: false,
                    is_dict_spread: false,
                    is_gather_spread: false,
                    span: Span::default(),
                }],
                span: Span::default(),
            })
            .unwrap_err();
        assert!(unknown_name
            .message
            .contains("no parameter named 'missing'"));
        let named = parse("def named(x: int, y: str) -> int:\n    return x\n").unwrap();
        let mut named_checker = TypeChecker::new();
        named_checker.check_module(&named).unwrap();
        let named_ok = named_checker
            .type_of_expr(&Expr::Call {
                func: Box::new(Expr::Ident {
                    name: "named".into(),
                    span: Span::default(),
                }),
                args: vec![
                    Arg {
                        name: Some("y".into()),
                        value: Expr::Literal {
                            value: LiteralValue::Str("ok".into()),
                            span: Span::default(),
                        },
                        is_spread: false,
                        is_dict_spread: false,
                        is_gather_spread: false,
                        span: Span::default(),
                    },
                    Arg {
                        name: Some("x".into()),
                        value: Expr::Literal {
                            value: LiteralValue::Int(1),
                            span: Span::default(),
                        },
                        is_spread: false,
                        is_dict_spread: false,
                        is_gather_spread: false,
                        span: Span::default(),
                    },
                ],
                span: Span::default(),
            })
            .unwrap();
        assert_eq!(named_ok, Type::Int);
        let bad_default = parse("def broken(x: int = \"wrong\") -> int:\n    return x\n").unwrap();
        let mut default_checker = TypeChecker::new();
        let error = default_checker.check_module(&bad_default).unwrap_err();
        assert!(error.message.contains("default value for parameter 'x'"));
        let bad_call =
            parse("def needs_int(x: int) -> int:\n    return x\nresult = needs_int(\"wrong\")\n")
                .unwrap();
        let mut call_checker = TypeChecker::new();
        let error = call_checker.check_module(&bad_call).unwrap_err();
        assert!(error
            .message
            .contains("argument 1 to 'needs_int' has incompatible type"));
        let bad_order = parse("def bad_order(x: int = 1, y: int) -> int:\n    return y\n").unwrap();
        let mut order_checker = TypeChecker::new();
        let error = order_checker.check_module(&bad_order).unwrap_err();
        assert!(error.message.contains("required parameter 'y' follows"));
        let duplicate = parse("def duplicate(x: int, x: int) -> int:\n    return x\n").unwrap();
        let mut duplicate_checker = TypeChecker::new();
        let error = duplicate_checker.check_module(&duplicate).unwrap_err();
        assert!(error.message.contains("duplicate parameter name 'x'"));

        let aggregate = parse("record = (x=1, y=\"ok\")\nrecord_x = record[\"x\"]\nrecord_attr = record.x\nmapping = {\"x\": 1}\nmapping_x = mapping[\"x\"]\n").unwrap();
        let mut aggregate_checker = TypeChecker::new();
        aggregate_checker.check_module(&aggregate).unwrap();
        assert!(matches!(
            aggregate_checker.env.variables.get("record"),
            Some((Type::Record { fields, .. }, _)) if fields.len() == 2
        ));
        assert!(matches!(
            aggregate_checker.env.variables.get("mapping"),
            Some((Type::Class { name, type_args, .. }, _))
                if name == "dict" && type_args.len() == 2
        ));
        assert!(matches!(
            aggregate_checker.env.variables.get("record_x"),
            Some((Type::Int, _))
        ));
        assert!(matches!(
            aggregate_checker.env.variables.get("record_attr"),
            Some((Type::Int, _))
        ));
        assert!(matches!(
            aggregate_checker.env.variables.get("mapping_x"),
            Some((Type::Int, _))
        ));
        let typed_dict_record = parse(
            "type Movie = {\"name\": str, \"year\": int}\nmovie: Movie = {\"name\": \"Paths of Glory\", \"year\": 1957}\nmovie[\"year\"] = 1958\nname: str = movie[\"name\"]\nyear: int = movie[\"year\"]\n",
        )
        .unwrap();
        let mut typed_dict_record_checker = TypeChecker::new();
        typed_dict_record_checker
            .check_module(&typed_dict_record)
            .unwrap();
        assert!(matches!(
            typed_dict_record_checker.env.variables.get("movie"),
            Some((Type::Record { fields, .. }, _)) if fields.len() == 2
        ));
        let bad_record_value =
            parse("type Movie = {\"name\": str, \"year\": int}\nmovie: Movie = {\"name\": 1957, \"year\": 1957}\n")
                .unwrap();
        let mut bad_record_value_checker = TypeChecker::new();
        let error = bad_record_value_checker
            .check_module(&bad_record_value)
            .unwrap_err();
        assert!(error.message.contains("record field 'name' expects"));
        let missing_record_field =
            parse("type Movie = {\"name\": str, \"year\": int}\nmovie: Movie = {\"name\": \"Paths\"}\n")
                .unwrap();
        let mut missing_record_field_checker = TypeChecker::new();
        let error = missing_record_field_checker
            .check_module(&missing_record_field)
            .unwrap_err();
        assert!(error.message.contains("record field 'year' is missing"));
        let extra_record_field =
            parse("type Movie = {\"name\": str, \"year\": int}\nmovie: Movie = {\"name\": \"Paths\", \"year\": 1957, \"director\": \"Kubrick\"}\n")
                .unwrap();
        let mut extra_record_field_checker = TypeChecker::new();
        let error = extra_record_field_checker
            .check_module(&extra_record_field)
            .unwrap_err();
        assert!(error.message.contains("record has no field 'director'"));
        let open_record_field =
            parse("type Movie = {\"name\": str, \"year\": int, ...}\nmovie: Movie = {\"name\": \"Paths\", \"year\": 1957, \"director\": \"Kubrick\"}\n")
                .unwrap();
        let mut open_record_field_checker = TypeChecker::new();
        open_record_field_checker.check_module(&open_record_field).unwrap();
        assert!(matches!(
            open_record_field_checker.env.variables.get("movie"),
            Some((Type::Record { is_open: true, .. }, _))
        ));
        let bad_record_index_assignment =
            parse("type Movie = {\"name\": str, \"year\": int}\nmovie: Movie = {\"name\": \"Paths\", \"year\": 1957}\nmovie[\"name\"] = 1957\n")
                .unwrap();
        let mut bad_record_index_assignment_checker = TypeChecker::new();
        let error = bad_record_index_assignment_checker
            .check_module(&bad_record_index_assignment)
            .unwrap_err();
        assert!(error.message.contains("record field of Str"));
        let structural =
            parse("type Point = (x: int, y: str)\np: Point = (x=1, y=\"ok\")\n").unwrap();
        let mut structural_checker = TypeChecker::new();
        structural_checker.check_module(&structural).unwrap();
        let structural_class = parse(
            "type Point = (x: int, y: str)\nclass Base:\n    x: int\nclass Child(Base):\n    y: str\np: Point = Child(1, \"ok\")\n",
        )
        .unwrap();
        let mut structural_class_checker = TypeChecker::new();
        structural_class_checker
            .check_module(&structural_class)
            .unwrap();
        let child = parse(
            "class Base:\n    x: int\nclass Child(Base):\n    y: str\nvalue = Child(1, \"ok\").x\n",
        )
        .unwrap();
        let mut child_checker = TypeChecker::new();
        child_checker.check_module(&child).unwrap();
        assert!(matches!(
            child_checker.env.variables.get("value"),
            Some((Type::Int, _))
        ));
        let duplicate_record = parse("type Broken = (x: int, x: str)\n").unwrap();
        let mut duplicate_record_checker = TypeChecker::new();
        let error = duplicate_record_checker
            .check_module(&duplicate_record)
            .unwrap_err();
        assert!(error.message.contains("duplicate record field 'x'"));
        let invalid_index = parse("values = [1, 2]\nresult = values[\"first\"]\n").unwrap();
        let mut invalid_index_checker = TypeChecker::new();
        let error = invalid_index_checker
            .check_module(&invalid_index)
            .unwrap_err();
        assert!(error.message.contains("sequence index must be int"));
        let invalid_key = parse("mapping = {\"x\": 1}\nresult = mapping[1]\n").unwrap();
        let mut invalid_key_checker = TypeChecker::new();
        let error = invalid_key_checker.check_module(&invalid_key).unwrap_err();
        assert!(error.message.contains("dictionary key has type"));
        let invalid_construct = parse("value = construct(1)\n").unwrap();
        let mut invalid_construct_checker = TypeChecker::new();
        let error = invalid_construct_checker
            .check_module(&invalid_construct)
            .unwrap_err();
        assert!(error
            .message
            .contains("construct(...) is only valid inside a class factory"));
        let invalid_field =
            parse("class Box:\n    value: int\nb = Box(1)\nb.value = \"wrong\"\n").unwrap();
        let mut invalid_field_checker = TypeChecker::new();
        let error = invalid_field_checker
            .check_module(&invalid_field)
            .unwrap_err();
        assert!(error.message.contains("cannot assign type"));
        let invalid_filter = parse("result = [x for x in [1, 2] if 1]\n").unwrap();
        let mut invalid_filter_checker = TypeChecker::new();
        let error = invalid_filter_checker
            .check_module(&invalid_filter)
            .unwrap_err();
        assert!(error
            .message
            .contains("list comprehension condition must be bool"));
        let invalid_iterator = parse("result = [x for x in missing_values]\n").unwrap();
        let mut invalid_iterator_checker = TypeChecker::new();
        let error = invalid_iterator_checker
            .check_module(&invalid_iterator)
            .unwrap_err();
        assert!(error
            .message
            .contains("undefined variable 'missing_values'"));
        for source in [
            "result = [x for x in 1]\n",
            "result = {x for x in 1}\n",
            "result = {x: x for x in 1}\n",
            "result = [x for x in \"abc\"]\n",
        ] {
            let mut checker = TypeChecker::new();
            let error = checker.check_module(&parse(source).unwrap()).unwrap_err();
            assert!(error.message.contains("not iterable"));
        }
        let invalid_loop = parse("for item in 1:\n    pass\n").unwrap();
        let mut invalid_loop_checker = TypeChecker::new();
        let error = invalid_loop_checker
            .check_module(&invalid_loop)
            .unwrap_err();
        assert!(error.message.contains("is not iterable"));
        TypeChecker::new()
            .check_module(&parse("class Box:\n    pass\nvalue = Box()\nis_class = value is class\nis_trait = value is trait\n").unwrap())
            .expect("declaration-kind instance checks should type-check");
        for (source, message) in [
            ("break\n", "break is only valid inside a loop"),
            ("continue\n", "continue is only valid inside a loop"),
            ("return 1\n", "return is only valid inside a function"),
            (
                "yield 1\n",
                "yield is only valid in a contextmanager definition",
            ),
        ] {
            let module = parse(source).unwrap();
            let mut checker = TypeChecker::new();
            let error = checker.check_module(&module).unwrap_err();
            assert!(error.message.contains(message));
        }
        let mut invalid_raise_checker = TypeChecker::new();
        let error = invalid_raise_checker
            .check_module(&parse("raise missing_error\n").unwrap())
            .unwrap_err();
        assert!(error.message.contains("undefined variable 'missing_error'"));
        let valid_control_flow = parse("while true:\n    continue\n").unwrap();
        TypeChecker::new()
            .check_module(&valid_control_flow)
            .expect("loop control should be accepted inside a loop");
        let invalid_user_iterator = parse(
            "class Counter:\n    def __iter__(self):\n        return self\n    def next(self) -> int:\n        return 1\nfor item in Counter():\n    item + \"wrong\"\n",
        )
        .unwrap();
        let mut invalid_user_iterator_checker = TypeChecker::new();
        let error = invalid_user_iterator_checker
            .check_module(&invalid_user_iterator)
            .unwrap_err();
        assert!(error.message.contains("unsupported operands"));
        let invalid_iterator_comprehension = parse(
            "class Counter:\n    def __iter__(self):\n        return self\n    def next(self) -> int:\n        return 1\nvalues = [item + \"wrong\" for item in Counter()]\n",
        )
        .unwrap();
        let mut invalid_iterator_comprehension_checker = TypeChecker::new();
        let error = invalid_iterator_comprehension_checker
            .check_module(&invalid_iterator_comprehension)
            .unwrap_err();
        assert!(error.message.contains("unsupported operands"));
        let invalid_literal = parse("result = [missing_value]\n").unwrap();
        let mut invalid_literal_checker = TypeChecker::new();
        let error = invalid_literal_checker
            .check_module(&invalid_literal)
            .unwrap_err();
        assert!(error.message.contains("undefined variable 'missing_value'"));
        let unhashable =
            parse("items = [\"x\"]\ninvalid_set = {items}\ninvalid_dict = {items: 1}\n").unwrap();
        let mut unhashable_checker = TypeChecker::new();
        let error = unhashable_checker.check_module(&unhashable).unwrap_err();
        assert!(error.message.contains("not hashable"));
        let invalid_anonymous = parse("f = def(x: typing.shape[\"bad\"]): x\n").unwrap();
        let mut invalid_anonymous_checker = TypeChecker::new();
        let error = invalid_anonymous_checker
            .check_module(&invalid_anonymous)
            .unwrap_err();
        assert!(error
            .message
            .contains("shape dimensions must be int literals"));
        let invalid_anonymous_body = parse("f = def(x: int) -> int: \"wrong\"\n").unwrap();
        let mut invalid_anonymous_body_checker = TypeChecker::new();
        let error = invalid_anonymous_body_checker
            .check_module(&invalid_anonymous_body)
            .unwrap_err();
        assert!(error.message.contains("return type mismatch"));
        let invalid_method = parse(
            "class Counter:\n    def add(self, amount: int) -> int:\n        return amount\nc = Counter()\nresult = c.add(amount=\"wrong\")\n",
        )
        .unwrap();
        let mut invalid_method_checker = TypeChecker::new();
        let error = invalid_method_checker
            .check_module(&invalid_method)
            .unwrap_err();
        assert!(error.message.contains("incompatible type"));
        let invalid_setter = parse(
            "class Box:\n    getter value(self) -> int:\n        return 1\n    setter value(self, new_value: int):\n        pass\nb = Box()\nread = b.value\nb.value = \"wrong\"\n",
        )
        .unwrap();
        let mut invalid_setter_checker = TypeChecker::new();
        let error = invalid_setter_checker
            .check_module(&invalid_setter)
            .unwrap_err();
        assert!(error.message.contains("setter 'value'"));
        let overloads = parse(
            "dispatch def choose(value: int) -> int:\n    return value\ndispatch def choose(value: str) -> str:\n    return value\nresult = choose(1)\n",
        )
        .unwrap();
        let mut overload_checker = TypeChecker::new();
        overload_checker.check_module(&overloads).unwrap();
        assert!(matches!(
            overload_checker.env.variables.get("result"),
            Some((Type::Int, _))
        ));
        let specificity = parse(
            "dispatch def select(value: object) -> str:\n    return \"wide\"\ndispatch def select(value: int) -> int:\n    return value\nresult = select(1)\n",
        )
        .unwrap();
        let mut specificity_checker = TypeChecker::new();
        specificity_checker.check_module(&specificity).unwrap();
        assert!(matches!(
            specificity_checker.env.variables.get("result"),
            Some((Type::Int, _))
        ));
        let ambiguous = parse(
            "class Left:\n    pass\nclass Right:\n    pass\nclass BothLeft(Left):\n    pass\nclass BothRight(Right):\n    pass\ndispatch def combine(left: Left, right: object) -> str:\n    return \"left\"\ndispatch def combine(left: object, right: Right) -> str:\n    return \"right\"\nresult = combine(BothLeft(), BothRight())\n",
        )
        .unwrap();
        let mut ambiguous_checker = TypeChecker::new();
        let error = ambiguous_checker.check_module(&ambiguous).unwrap_err();
        assert!(error.message.contains("ambiguous dispatch"));
        let ambiguous_operator = parse(
            "class Animal:\n    pass\nclass Cat(Animal):\n    pass\nclass Dog(Animal):\n    pass\ndispatch def __add__(lhs: Cat, rhs: Animal) -> str:\n    return \"cat\"\ndispatch def __add__(lhs: Animal, rhs: Dog) -> str:\n    return \"dog\"\nresult = Cat() + Dog()\n",
        )
        .unwrap();
        let mut ambiguous_operator_checker = TypeChecker::new();
        let error = ambiguous_operator_checker
            .check_module(&ambiguous_operator)
            .unwrap_err();
        assert!(error.message.contains("ambiguous dispatch"));
        let specific_operator = parse(
            "class Animal:\n    pass\nclass Cat(Animal):\n    pass\nclass Dog(Animal):\n    pass\ndispatch def __add__(lhs: Cat, rhs: Animal) -> str:\n    return \"cat\"\ndispatch def __add__(lhs: Cat, rhs: Dog) -> str:\n    return \"both\"\nresult = Cat() + Dog()\n",
        )
        .unwrap();
        let mut specific_operator_checker = TypeChecker::new();
        specific_operator_checker
            .check_module(&specific_operator)
            .unwrap();
        assert!(matches!(
            specific_operator_checker.env.variables.get("result"),
            Some((Type::Str, _))
        ));
        let invalid_overload = parse(
            "dispatch def choose(value: int) -> int:\n    return value\ndispatch def choose(value: str) -> str:\n    return value\nresult = choose(true)\n",
        )
        .unwrap();
        let mut invalid_overload_checker = TypeChecker::new();
        let error = invalid_overload_checker
            .check_module(&invalid_overload)
            .unwrap_err();
        assert!(error.message.contains("no overload"));
        let duplicate_function =
            parse("def duplicate() -> int:\n    return 1\ndef duplicate() -> int:\n    return 2\n")
                .unwrap();
        let mut duplicate_function_checker = TypeChecker::new();
        let error = duplicate_function_checker
            .check_module(&duplicate_function)
            .unwrap_err();
        assert!(error.message.contains("requires dispatch"));
        let default_constructor =
            parse("class Defaults:\n    value: int = 1\nd = Defaults()\n").unwrap();
        let mut default_constructor_checker = TypeChecker::new();
        default_constructor_checker
            .check_module(&default_constructor)
            .unwrap();
        let invalid_constructor =
            parse("class Point:\n    x: int\n    y: int\np = Point(x=\"bad\", y=2)\n").unwrap();
        let mut invalid_constructor_checker = TypeChecker::new();
        let error = invalid_constructor_checker
            .check_module(&invalid_constructor)
            .unwrap_err();
        assert!(error.message.contains("constructor field 'x'"));
    }

    #[test]
    fn test_literal_type_arithmetic_is_evaluated() {
        let checker = TypeChecker::new();
        let expr = TypeExpr::Named {
            name: "__shape_mul__".into(),
            args: vec![
                TypeExpr::Literal {
                    value: LiteralValue::Int(3),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(4),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::LiteralInt(12)
        );
        let overflow = TypeExpr::Named {
            name: "__shape_add__".into(),
            args: vec![
                TypeExpr::Literal {
                    value: LiteralValue::Int(i64::MAX),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(1),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        let error = checker.resolve_type_expr(&overflow).unwrap_err();
        assert!(error.message.contains("arithmetic overflow"));
        let invalid_primitive = TypeExpr::Named {
            name: "int".into(),
            args: vec![TypeExpr::Named {
                name: "str".into(),
                args: Vec::new(),
                span: Span::default(),
            }],
            span: Span::default(),
        };
        let error = checker.resolve_type_expr(&invalid_primitive).unwrap_err();
        assert!(error
            .message
            .contains("type 'int' expects no type arguments"));
    }

    #[test]
    fn test_complex_values_are_not_orderable() {
        let module = parse("z: complex = 1 + 2j\nresult = z < 3 + 4j\n").unwrap();
        let mut checker = TypeChecker::new();
        let error = checker.check_module(&module).unwrap_err();
        assert!(error.message.contains("complex values are not orderable"));
    }

    #[test]
    fn test_shape_index_resolves_literal_dimension() {
        let checker = TypeChecker::new();
        let expr = TypeExpr::Named {
            name: "__shape_index__".into(),
            args: vec![
                TypeExpr::Named {
                    name: "typing.shape".into(),
                    args: vec![
                        TypeExpr::Literal {
                            value: LiteralValue::Int(2),
                            span: Span::default(),
                        },
                        TypeExpr::Literal {
                            value: LiteralValue::Int(3),
                            span: Span::default(),
                        },
                    ],
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(-1),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::LiteralInt(3)
        );
    }

    #[test]
    fn test_shape_rank_resolves_concrete_shape_rank() {
        let checker = TypeChecker::new();
        let expr = TypeExpr::Named {
            name: "__shape_rank__".into(),
            args: vec![TypeExpr::Named {
                name: "typing.shape".into(),
                args: vec![
                    TypeExpr::Literal {
                        value: LiteralValue::Int(2),
                        span: Span::default(),
                    },
                    TypeExpr::Literal {
                        value: LiteralValue::Int(3),
                        span: Span::default(),
                    },
                ],
                span: Span::default(),
            }],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::LiteralInt(2)
        );
    }

    #[test]
    fn test_shape_product_resolves_literal_element_count() {
        let checker = TypeChecker::new();
        let shape = TypeExpr::Named {
            name: "typing.shape".into(),
            args: vec![
                TypeExpr::Literal {
                    value: LiteralValue::Int(2),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(3),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(4),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_product__".into(),
            args: vec![shape],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::LiteralInt(24)
        );
    }

    #[test]
    fn test_shape_product_reports_overflow() {
        let checker = TypeChecker::new();
        let shape = TypeExpr::Named {
            name: "typing.shape".into(),
            args: vec![
                TypeExpr::Literal {
                    value: LiteralValue::Int(i64::MAX),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(2),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_product__".into(),
            args: vec![shape],
            span: Span::default(),
        };
        assert!(checker
            .resolve_type_expr(&expr)
            .unwrap_err()
            .message
            .contains("overflow"));
    }

    #[test]
    fn test_shape_broadcast_dimension_folds_equal_or_one() {
        let checker = TypeChecker::new();
        let make = |left, right| TypeExpr::Named {
            name: "__shape_broadcast_dim__".into(),
            args: vec![
                TypeExpr::Literal {
                    value: LiteralValue::Int(left),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(right),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&make(3, 3)).unwrap(),
            Type::LiteralInt(3)
        );
        assert_eq!(
            checker.resolve_type_expr(&make(1, 7)).unwrap(),
            Type::LiteralInt(7)
        );
        assert_eq!(
            checker.resolve_type_expr(&make(5, 1)).unwrap(),
            Type::LiteralInt(5)
        );
        assert!(checker
            .resolve_type_expr(&make(2, 3))
            .unwrap_err()
            .message
            .contains("broadcast-compatible"));
    }

    #[test]
    fn test_shape_broadcast_aligns_from_the_right() {
        let checker = TypeChecker::new();
        let shape = |dims: &[i64]| TypeExpr::Named {
            name: "typing.shape".into(),
            args: dims
                .iter()
                .map(|value| TypeExpr::Literal {
                    value: LiteralValue::Int(*value),
                    span: Span::default(),
                })
                .collect(),
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_broadcast__".into(),
            args: vec![shape(&[2, 1, 4]), shape(&[3, 4])],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::Shape(vec![Some(2), Some(3), Some(4)])
        );
    }

    #[test]
    fn test_shape_matmul_broadcasts_batches_and_checks_inner_axes() {
        let checker = TypeChecker::new();
        let shape = |dims: &[i64]| TypeExpr::Named {
            name: "typing.shape".into(),
            args: dims
                .iter()
                .map(|value| TypeExpr::Literal {
                    value: LiteralValue::Int(*value),
                    span: Span::default(),
                })
                .collect(),
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_matmul__".into(),
            args: vec![shape(&[2, 1, 3, 4]), shape(&[5, 4, 6])],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::Shape(vec![Some(2), Some(5), Some(3), Some(6)])
        );
        let incompatible = TypeExpr::Named {
            name: "__shape_matmul__".into(),
            args: vec![shape(&[2, 3]), shape(&[4, 5])],
            span: Span::default(),
        };
        assert!(checker
            .resolve_type_expr(&incompatible)
            .unwrap_err()
            .message
            .contains("inner dimensions"));
    }

    #[test]
    fn test_shape_reshape_checks_concrete_element_counts() {
        let checker = TypeChecker::new();
        let shape = |dims: &[i64]| TypeExpr::Named {
            name: "typing.shape".into(),
            args: dims
                .iter()
                .map(|value| TypeExpr::Literal {
                    value: LiteralValue::Int(*value),
                    span: Span::default(),
                })
                .collect(),
            span: Span::default(),
        };
        let valid = TypeExpr::Named {
            name: "__shape_reshape__".into(),
            args: vec![shape(&[2, 3]), shape(&[3, 2])],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&valid).unwrap(),
            Type::Shape(vec![Some(3), Some(2)])
        );
        let invalid = TypeExpr::Named {
            name: "__shape_reshape__".into(),
            args: vec![shape(&[2, 3]), shape(&[5])],
            span: Span::default(),
        };
        assert!(checker
            .resolve_type_expr(&invalid)
            .unwrap_err()
            .message
            .contains("element counts"));
    }

    #[test]
    fn test_shape_concat_axis0_adds_leading_dimension() {
        let checker = TypeChecker::new();
        let shape = |dims: &[i64]| TypeExpr::Named {
            name: "typing.shape".into(),
            args: dims
                .iter()
                .map(|value| TypeExpr::Literal {
                    value: LiteralValue::Int(*value),
                    span: Span::default(),
                })
                .collect(),
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_concat_axis0__".into(),
            args: vec![shape(&[2, 3, 4]), shape(&[5, 3, 4])],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::Shape(vec![Some(7), Some(3), Some(4)])
        );
    }

    #[test]
    fn test_shape_reverse_preserves_symbolic_dimensions() {
        let checker = TypeChecker::new();
        let shape = TypeExpr::Named {
            name: "typing.shape".into(),
            args: vec![
                TypeExpr::Literal {
                    value: LiteralValue::Int(2),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(3),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(4),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_reverse__".into(),
            args: vec![shape],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::Shape(vec![Some(4), Some(3), Some(2)])
        );
    }

    #[test]
    fn test_shape_swap_last_two_axes() {
        let checker = TypeChecker::new();
        let shape = TypeExpr::Named {
            name: "typing.shape".into(),
            args: vec![
                TypeExpr::Literal {
                    value: LiteralValue::Int(2),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(3),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(4),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_swap_last2__".into(),
            args: vec![shape],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::Shape(vec![Some(2), Some(4), Some(3)])
        );
    }

    #[test]
    fn test_shape_stack_infers_operand_count_as_leading_axis() {
        let checker = TypeChecker::new();
        let shape = |dims: &[i64]| TypeExpr::Named {
            name: "typing.shape".into(),
            args: dims
                .iter()
                .map(|value| TypeExpr::Literal {
                    value: LiteralValue::Int(*value),
                    span: Span::default(),
                })
                .collect(),
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_stack__".into(),
            args: vec![shape(&[2, 3]), shape(&[2, 3]), shape(&[2, 3])],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::Shape(vec![Some(3), Some(2), Some(3)])
        );
    }

    #[test]
    fn test_shape_insert_at_supports_negative_index() {
        let checker = TypeChecker::new();
        let shape = TypeExpr::Named {
            name: "typing.shape".into(),
            args: vec![
                TypeExpr::Literal {
                    value: LiteralValue::Int(2),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(4),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_insert_at__".into(),
            args: vec![
                shape,
                TypeExpr::Literal {
                    value: LiteralValue::Int(-1),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(3),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::Shape(vec![Some(2), Some(3), Some(4)])
        );
    }

    #[test]
    fn test_shape_drop_at_supports_negative_index() {
        let checker = TypeChecker::new();
        let shape = TypeExpr::Named {
            name: "typing.shape".into(),
            args: vec![
                TypeExpr::Literal {
                    value: LiteralValue::Int(2),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(3),
                    span: Span::default(),
                },
                TypeExpr::Literal {
                    value: LiteralValue::Int(4),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        let expr = TypeExpr::Named {
            name: "__shape_drop_at__".into(),
            args: vec![
                shape,
                TypeExpr::Literal {
                    value: LiteralValue::Int(-1),
                    span: Span::default(),
                },
            ],
            span: Span::default(),
        };
        assert_eq!(
            checker.resolve_type_expr(&expr).unwrap(),
            Type::Shape(vec![Some(2), Some(3)])
        );
    }

    #[test]
    fn test_shape_slice_resolves_concrete_prefix() {
        let module = parse("type S = typing.shape[2, 3, 4]\ntype T = S[:-1]\n").unwrap();
        let mut checker = TypeChecker::new();
        checker.check_module(&module).unwrap();
        assert_eq!(
            checker.env.type_aliases.get("T"),
            Some(&Type::Shape(vec![Some(2), Some(3)]))
        );
        let mut reverse_checker = TypeChecker::new();
        reverse_checker
            .check_module(&parse("type S = typing.shape[2, 3, 4]\ntype T = S[::-1]\n").unwrap())
            .unwrap();
        assert_eq!(
            reverse_checker.env.type_aliases.get("T"),
            Some(&Type::Shape(vec![Some(4), Some(3), Some(2)]))
        );
        let mut bounded_reverse_checker = TypeChecker::new();
        bounded_reverse_checker
            .check_module(&parse("type S = typing.shape[2, 3, 4]\ntype T = S[2:0:-1]\n").unwrap())
            .unwrap();
        assert_eq!(
            bounded_reverse_checker.env.type_aliases.get("T"),
            Some(&Type::Shape(vec![Some(4), Some(3)]))
        );
        let error = checker
            .check_module(&parse("value = [1, 2][\"bad\":]\n").unwrap())
            .unwrap_err();
        assert!(error.message.contains("slice bound must be int"));
        let error = checker
            .check_module(&parse("value = 1[0]\n").unwrap())
            .unwrap_err();
        assert!(error.message.contains("not indexable"));
        let error = checker
            .check_module(&parse("value = [1, 2][::0]\n").unwrap())
            .unwrap_err();
        assert!(error.message.contains("slice step cannot be zero"));
    }

    #[test]
    fn test_trust_requires_object_operand() {
        let mut checker = TypeChecker::new();
        checker
            .check_module(&parse("raw: object = none\nvalue = trust[int](raw)\n").unwrap())
            .unwrap();
        let error = checker
            .check_module(&parse("value = trust[str](1)\n").unwrap())
            .unwrap_err();
        assert!(error
            .message
            .contains("trust[...] only accepts an object-typed operand"));
    }

    #[test]
    fn test_strings_are_not_iterable() {
        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("for ch in \"abc\":\n    pass\n").unwrap())
            .unwrap_err();
        assert!(error.message.contains("str is not Iterable"));

        checker
            .check_module(
                &parse("text = \"abc\"\nfor ch in text.chars:\n    ch.upper()\nfirst: str = text.chars[0]\npart = text.chars[1:]\nhas_b = \"b\" in text.chars\nmissing_pair = \"bc\" in text.chars\nchars: ~Sequence[str] = text.chars\nview_first: str = chars[0]\nview_has_b = \"b\" in chars\n").unwrap(),
            )
            .expect("str.chars should be iterable, indexable, and support membership");
        let error = TypeChecker::new()
            .check_module(&parse("text = \"abc\"\nchars: ~Sequence[str] = text.chars\nbad = 1 in chars\n").unwrap())
            .unwrap_err();
        assert!(error.message.contains("membership value"));
        let error = TypeChecker::new()
            .check_module(&parse("text = \"abc\"\ntext.chars.append(\"x\")\n").unwrap())
            .unwrap_err();
        assert!(error.message.contains("mutating method 'append'"));
    }

    #[test]
    fn test_function_types_satisfy_callable_trait() {
        let source = "def apply(fn: Callable, value: int) -> int:\n    return value\ndef twice(x: int) -> int:\n    return x * 2\nresult = apply(twice, 3)\n";
        let mut checker = TypeChecker::new();
        checker.check_module(&parse(source).unwrap()).unwrap();
    }

    #[test]
    fn test_without_eq_requires_explicit_equality_member() {
        let mut checker = TypeChecker::new();
        let source = "class Token without Eq:\n    value: int\na = Token(1)\nb = Token(1)\nresult = a == b\n";
        let error = checker.check_module(&parse(source).unwrap()).unwrap_err();
        assert!(error.message.contains("comparison requires Eq"));
    }

    #[test]
    fn test_without_hashable_rejects_hash() {
        let mut checker = TypeChecker::new();
        let source =
            "class Token without Hashable:\n    value: int\nt = Token(1)\nresult = hash(t)\n";
        let error = checker.check_module(&parse(source).unwrap()).unwrap_err();
        assert!(error.message.contains("hash() requires Hashable"));
    }

    #[test]
    fn class_value_semantics_options_drive_capabilities() {
        let mut checker = TypeChecker::new();
        checker
            .check_module(
                &parse(
                    "class Point(eq=true, order=true, hash=true):\n    x: int\n    y: int\np = Point(1, 2)\nq = Point(1, 3)\nequal = p == q\nless = p < q\ncode = hash(p)\n",
                )
                .unwrap(),
            )
            .expect("enabled value semantics options should check");

        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(
                &parse(
                    "class Point(order=false):\n    x: int\np = Point(1)\nq = Point(2)\nless = p < q\n",
                )
                .unwrap(),
            )
            .unwrap_err();
        assert!(error.message.contains("ordering requires Ord"));
    }

    #[test]
    fn test_builtin_contracts_reject_wrong_arity() {
        for (source, message) in [
            ("hash()\n", "requires at least 1"),
            ("locals(1)\n", "accepts at most 0"),
            ("getattr(1)\n", "requires at least 2"),
            ("setattr(1, \"x\")\n", "requires at least 3"),
            ("any()\n", "requires at least 1"),
            ("pow(1)\n", "requires at least 2"),
            ("sorted(1)\n", "argument must be iterable"),
            ("zip(1)\n", "arguments must be iterable"),
            ("abs()\n", "requires at least 1"),
            ("round(1, 2, 3)\n", "accepts at most 2"),
            ("sum([\"bad\"])\n", "elements must be numeric"),
            ("sum([true])\n", "elements must be numeric"),
            ("sum([1], \"bad\")\n", "start must be numeric"),
            ("min()\n", "requires at least 1"),
            ("max(1)\n", "argument must be iterable"),
            ("pow(\"x\", 2)\n", "arguments must be numeric"),
            ("pow(2, 3, 1.0)\n", "modulus must be int"),
            ("read_file()\n", "requires at least 1"),
            ("write_file(\"x\")\n", "requires at least 2"),
            ("range()\n", "requires at least 1"),
            ("time(1)\n", "accepts at most 0"),
            ("range(1, \"bad\")\n", "arguments must be int"),
            ("range(1, 2, 0)\n", "step cannot be zero"),
            ("slice()\n", "requires at least 1"),
            ("slice(1, \"bad\")\n", "bounds must be int or none"),
            ("map(1)\n", "requires at least 2"),
            ("map(1, [1])\n", "first argument must be callable"),
            (
                "def f(x):\n    return x\nmap(f, 1)\n",
                "iterable argument must be iterable",
            ),
            ("enumerate([1], \"bad\")\n", "start must be int"),
            ("reversed(1)\n", "not reversible"),
            ("complex(\"x\")\n", "arguments must be numeric"),
            ("complex(1, 2, 3)\n", "accepts at most 2"),
            ("list([], [])\n", "accepts at most 1"),
            ("set([], [])\n", "accepts at most 1"),
            ("help(1, 2)\n", "accepts at most 1"),
            ("fields()\n", "requires at least 1"),
            ("len(1)\n", "not sized"),
            ("ord(\"ab\")\n", "is not a bare builtin"),
            ("chr(0x110000)\n", "is not a bare builtin"),
        ] {
            let error = TypeChecker::new()
                .check_module(&parse(source).unwrap())
                .unwrap_err();
            assert!(
                error.message.contains(message),
                "{source}: {}",
                error.message
            );
        }
        for source in ["list(1)\n", "set(1)\n"] {
            let error = TypeChecker::new()
                .check_module(&parse(source).unwrap())
                .unwrap_err();
            assert!(error.message.contains("argument must be iterable"));
        }
        let error = TypeChecker::new()
            .check_module(&parse("dict(1)\n").unwrap())
            .unwrap_err();
        assert!(error.message.contains("mapping or iterable"));
        for (source, message) in [
            ("round(1.0, \"bad\")\n", "ndigits must be int"),
            ("sum(1)\n", "argument must be iterable"),
            ("abs(\"bad\")\n", "argument must be numeric"),
        ] {
            let error = TypeChecker::new()
                .check_module(&parse(source).unwrap())
                .unwrap_err();
            assert!(
                error.message.contains(message),
                "{source}: {}",
                error.message
            );
        }
        let mut checker = TypeChecker::new();
        checker
            .check_module(
                &parse("result = sorted([1])\nmapping = dict()\ncopy = dict({\"a\": 1})\nitems = set()\nsmall = min([1.0])\nlarge = max(1, 2)\n").unwrap(),
            )
            .unwrap();
        assert!(matches!(
            checker.env.variables.get("result").map(|(ty, _)| ty),
            Some(Type::Class { name, .. }) if name == "list"
        ));
        assert!(matches!(
            checker.env.variables.get("mapping").map(|(ty, _)| ty),
            Some(Type::Class { name, .. }) if name == "dict"
        ));
        assert!(matches!(
            checker.env.variables.get("copy").map(|(ty, _)| ty),
            Some(Type::Class { name, type_args, .. })
                if name == "dict" && type_args == &vec![Type::Str, Type::Int]
        ));
        assert!(matches!(
            checker.env.variables.get("small").map(|(ty, _)| ty),
            Some(Type::Float)
        ));
        assert!(matches!(
            checker.env.variables.get("large").map(|(ty, _)| ty),
            Some(Type::Int)
        ));
        let mut checker = TypeChecker::new();
        checker
            .check_module(&parse("values = list([1])\nunique = set({1})\n").unwrap())
            .unwrap();
        assert!(matches!(
            checker.env.variables.get("values").map(|(ty, _)| ty),
            Some(Type::Class { name, type_args, .. })
                if name == "list" && type_args == &vec![Type::Int]
        ));
        assert!(matches!(
            checker.env.variables.get("unique").map(|(ty, _)| ty),
            Some(Type::Class { name, type_args, .. })
                if name == "set" && type_args == &vec![Type::Int]
        ));
        let mut checker = TypeChecker::new();
        checker
            .check_module(&parse("a = abs(-1)\nb = abs(-1.0)\nc = round(1.5)\nd = round(1.5, 1)\ne = sum([1.0])\n").unwrap())
            .unwrap();
        assert!(matches!(
            checker.env.variables.get("a").map(|(ty, _)| ty),
            Some(Type::Int)
        ));
        assert!(matches!(
            checker.env.variables.get("b").map(|(ty, _)| ty),
            Some(Type::Float)
        ));
        assert!(matches!(
            checker.env.variables.get("c").map(|(ty, _)| ty),
            Some(Type::Int)
        ));
        assert!(matches!(
            checker.env.variables.get("d").map(|(ty, _)| ty),
            Some(Type::Float)
        ));
        assert!(matches!(
            checker.env.variables.get("e").map(|(ty, _)| ty),
            Some(Type::Float)
        ));
    }

    #[test]
    fn string_is_not_accepted_as_iterable_or_reversible() {
        for (source, message) in [
            ("letters = list(\"abc\")\n", "argument must be iterable"),
            (
                "def f(x: str) -> str:\n    return x\nletters = map(f, \"abc\")\n",
                "iterable argument must be iterable",
            ),
            ("pairs = zip(\"ab\", [1, 2])\n", "arguments must be iterable"),
            ("letters = reversed(\"abc\")\n", "not reversible"),
        ] {
            let error = TypeChecker::new()
                .check_module(&parse(source).unwrap())
                .expect_err("plain str must not satisfy iterable/reversible builtins");
            assert!(error.message.contains(message), "{}", error.message);
        }

        TypeChecker::new()
            .check_module(&parse("letters = list(\"abc\".chars)\n").unwrap())
            .expect("str.chars should satisfy iterable builtins");
    }

    #[test]
    fn test_yield_requires_contextmanager() {
        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("def generator() -> int:\n    yield 1\n").unwrap())
            .unwrap_err();
        assert!(error
            .message
            .contains("yield is only valid in a contextmanager"));
    }

    #[test]
    fn test_without_eq_is_checked_on_right_operand() {
        let mut checker = TypeChecker::new();
        let source =
            "class Token without Eq:\n    value: int\na = 1\nb = Token(1)\nresult = a == b\n";
        let error = checker.check_module(&parse(source).unwrap()).unwrap_err();
        assert!(error
            .message
            .contains("comparison requires Eq on both operands"));
    }

    #[test]
    fn test_private_imports_are_rejected_statically() {
        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("from module import _private\n").unwrap())
            .expect_err("private imports must fail static checking");
        assert!(error.message.contains("private name '_private'"));
        assert!(error.span.end > error.span.start);
    }

    #[test]
    fn test_private_exports_are_rejected_statically() {
        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("export _private = 1\n").unwrap())
            .expect_err("private exports must fail static checking");
        assert!(error.message.contains("cannot export private name"));
        assert!(error.span.end > error.span.start);
    }

    #[test]
    fn test_dunder_all_binding_is_rejected_statically() {
        for source in [
            "__all__ = [\"value\"]\nvalue = 1\n",
            "let __all__ = [\"value\"]\n",
            "def __all__() -> int:\n    return 1\n",
            "class __all__:\n    pass\n",
            "export __all__ = [\"value\"]\n",
        ] {
            let mut checker = TypeChecker::new();
            let error = checker
                .check_module(&parse(source).unwrap())
                .expect_err("__all__ binding must fail static checking");
            assert!(error.message.contains("__all__ is not supported"));
            assert!(error.span.end > error.span.start);
        }
    }

    #[test]
    fn test_delitem_member_is_rejected_statically() {
        for source in [
            "class Bag:\n    def __delitem__(self, index: int):\n        pass\n",
            "interface Removable:\n    def __delitem__(self, index: int) -> none\n",
            "trait Removable:\n    def __delitem__(self, index: int):\n        pass\n",
        ] {
            let mut checker = TypeChecker::new();
            let error = checker
                .check_module(&parse(source).unwrap())
                .expect_err("__delitem__ member must fail static checking");
            assert!(error.message.contains("__delitem__ is not supported"));
            assert!(error.span.end > error.span.start);
        }
    }

    #[test]
    fn test_removed_dynamic_attribute_hooks_are_rejected_statically() {
        for (source, message) in [
            (
                "class Hook:\n    def __getattr__(self, name: str) -> int:\n        return 1\n",
                "__getattr__ is not supported",
            ),
            (
                "class Hook:\n    def __getattribute__(self, name: str) -> int:\n        return 1\n",
                "__getattribute__ is not supported",
            ),
            (
                "class Hook:\n    def __setattr__(self, name: str, value: int):\n        pass\n",
                "__setattr__ is not supported",
            ),
            (
                "class Hook:\n    def __del__(self):\n        pass\n",
                "__del__ is not supported",
            ),
            (
                "interface Hook:\n    def __getattr__(self, name: str) -> int\n",
                "__getattr__ is not supported",
            ),
            (
                "trait Hook:\n    def __setattr__(self, name: str, value: int):\n        pass\n",
                "__setattr__ is not supported",
            ),
        ] {
            let mut checker = TypeChecker::new();
            let error = checker
                .check_module(&parse(source).unwrap())
                .expect_err("removed dynamic attribute hook must fail static checking");
            assert!(error.message.contains(message));
            assert!(error.span.end > error.span.start);
        }
    }

    #[test]
    fn test_removed_python_member_decorators_are_rejected_statically() {
        for (source, message) in [
            (
                "class Tools:\n    @staticmethod\n    def answer() -> int:\n        return 42\n",
                "staticmethod is not supported",
            ),
            (
                "class Circle:\n    @property\n    def area(self) -> int:\n        return 1\n",
                "property is not supported",
            ),
            (
                "@staticmethod\ndef answer() -> int:\n    return 42\n",
                "staticmethod is not supported",
            ),
            (
                "@overload\ndef parse(value: str) -> int:\n    return 1\n",
                "overload is not supported",
            ),
            (
                "@typing.overload\ndef parse(value: str) -> int:\n    return 1\n",
                "overload is not supported",
            ),
        ] {
            let mut checker = TypeChecker::new();
            let error = checker
                .check_module(&parse(source).unwrap())
                .expect_err("removed Python decorator must fail static checking");
            assert!(error.message.contains(message));
            assert!(error.span.end > error.span.start);
        }
    }

    #[test]
    fn test_removed_string_codepoint_builtins_are_rejected_statically() {
        for source in [
            "letter = chr(65)\n",
            "codepoint = ord(\"A\")\n",
            "print(chr(65))\n",
        ] {
            let mut checker = TypeChecker::new();
            let error = checker
                .check_module(&parse(source).unwrap())
                .expect_err("removed string codepoint builtin must fail static checking");
            assert!(error.message.contains("is not a bare builtin"));
            assert!(error.span.end > error.span.start);
        }
    }

    #[test]
    fn test_removed_builtins_have_specific_static_diagnostics() {
        for (source, message) in [
            ("value = tuple([1, 2])\n", "tuple is not supported"),
            ("value = property\n", "getter/setter"),
            ("value = staticmethod\n", "staticmethod is not supported"),
            ("value = NotImplemented\n", "NotImplemented is not supported"),
            ("value = isinstance(1, int)\n", "value is Type"),
            ("value = issubclass(int, object)\n", "declaration-kind"),
            ("value = frozenset([1, 2])\n", "immutable set literal"),
            ("value = eval(\"1 + 1\")\n", "checker can see"),
            ("exec(\"value = 1\")\n", "checker can see"),
            ("value = __import__(\"math\")\n", "static import"),
            ("value = vars()\n", "fields"),
            ("value = dir()\n", "fields"),
            ("value = next([1, 2])\n", "cursor.next"),
            ("value = ascii(\"é\")\n", "string.ascii"),
            (
                "value = filter(def(v: int) -> bool: true, [1])\n",
                "comprehension",
            ),
            ("value = globals()\n", "locals"),
            ("value = compile(\"1\", \"<x>\", \"eval\")\n", "compile() is not a bare builtin"),
            (
                "class P:\n    x: int\np = P(1)\ndelattr(p, \"x\")\n",
                "declared fields are fixed",
            ),
            ("value = open(\"a.txt\")\n", "Path.open"),
            ("value = bin(5)\n", "str.bin"),
            ("value = oct(5)\n", "str.oct"),
            ("value = hex(5)\n", "str.hex"),
        ] {
            let mut checker = TypeChecker::new();
            let error = checker
                .check_module(&parse(source).unwrap())
                .expect_err("removed Python builtin must fail static checking");
            assert!(
                error.message.contains(message),
                "{source}: {}",
                error.message
            );
            assert!(error.span.end > error.span.start);
        }
    }

    #[test]
    fn test_bare_skip_is_rejected_as_value() {
        for (source, message) in [
            ("value = skip\n", "assignment value"),
            ("def f():\n    return skip\n", "return value"),
            ("skip\n", "expression statement"),
            ("if skip:\n    pass\n", "if condition"),
            ("while skip:\n    pass\n", "while condition"),
        ] {
            let error = TypeChecker::new()
                .check_module(&parse(source).unwrap())
                .expect_err("bare skip must fail outside elision contexts");
            assert!(error.message.contains(message), "{}", error.message);
        }

        TypeChecker::new()
            .check_module(
                &parse(
                    "def f(value: int = 1) -> int:\n    return value\nx = f(skip)\nitems = [1, skip, 2]\nmapping = {\"a\": 1, \"b\": skip}\n",
                )
                .unwrap(),
            )
            .expect("skip should remain valid in call and collection elision contexts");
    }

    #[test]
    fn test_exact_class_instance_checks_reject_dead_trait_and_subclass_tests() {
        for (source, message) in [
            (
                "class HasLen:\n    def __len__(self) -> int:\n        return 1\nvalue = HasLen()\ncheck = value is Sized\n",
                "exact class 'HasLen'",
            ),
            (
                "class Animal:\n    pass\nclass Dog(Animal):\n    pass\nvalue = Animal()\ncheck = value is Dog\n",
                "exact class 'Animal'",
            ),
            (
                "class Widget:\n    pass\nvalue = Widget()\ncheck = value is str\n",
                "and Str can never succeed",
            ),
        ] {
            let error = TypeChecker::new()
                .check_module(&parse(source).unwrap())
                .expect_err("dead exact-class instance check must fail");
            assert!(error.message.contains(message), "{}", error.message);
        }

        TypeChecker::new()
            .check_module(
                &parse(
                    "class Declared(Sized):\n    def __len__(self) -> int:\n        return 1\nclass Retro:\n    pass\nimplement Sized for Retro:\n    def __len__(self) -> int:\n        return 1\ndeclared = Declared() is Sized\nretro = Retro() is Sized\n",
                )
                .unwrap(),
            )
            .expect("nominal and implemented trait checks should still type-check");
    }

    #[test]
    fn test_removed_inheritance_hooks_are_rejected_statically() {
        for (source, message) in [
            (
                "class Hook:\n    def __mro_entries__(self) -> int:\n        return 1\n",
                "__mro_entries__ is not supported",
            ),
            (
                "class Hook:\n    def __prepare__(self) -> int:\n        return 1\n",
                "__prepare__ is not supported",
            ),
            (
                "class Hook:\n    def __instancecheck__(self) -> bool:\n        return true\n",
                "__instancecheck__ is not supported",
            ),
            (
                "class Hook:\n    def __subclasscheck__(self) -> bool:\n        return true\n",
                "__subclasscheck__ is not supported",
            ),
        ] {
            let mut checker = TypeChecker::new();
            let error = checker
                .check_module(&parse(source).unwrap())
                .expect_err("removed inheritance hook must fail static checking");
            assert!(error.message.contains(message));
            assert!(error.span.end > error.span.start);
        }
    }

    #[test]
    fn test_removed_descriptor_hooks_are_rejected_statically() {
        for (source, message) in [
            (
                "class Descriptor:\n    def __get__(self, obj: object, owner: object) -> int:\n        return 1\n",
                "__get__ is not supported",
            ),
            (
                "class Descriptor:\n    def __set__(self, obj: object, value: int):\n        pass\n",
                "__set__ is not supported",
            ),
            (
                "class Descriptor:\n    def __delete__(self, obj: object):\n        pass\n",
                "__delete__ is not supported",
            ),
            (
                "class Descriptor:\n    def __set_name__(self, owner: object, name: str):\n        pass\n",
                "__set_name__ is not supported",
            ),
            (
                "interface Descriptor:\n    def __get__(self, obj: object, owner: object) -> int\n",
                "__get__ is not supported",
            ),
            (
                "trait Descriptor:\n    def __set__(self, obj: object, value: int):\n        pass\n",
                "__set__ is not supported",
            ),
        ] {
            let mut checker = TypeChecker::new();
            let error = checker
                .check_module(&parse(source).unwrap())
                .expect_err("removed descriptor hook must fail static checking");
            assert!(error.message.contains(message));
            assert!(error.span.end > error.span.start);
        }
    }

    #[test]
    fn test_private_module_attributes_are_rejected_statically() {
        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("import math\nvalue = math._internal\n").unwrap())
            .expect_err("private module attributes must fail static checking");
        assert!(error
            .message
            .contains("module attribute '_internal' is private"));
    }

    #[test]
    fn test_duplicate_import_bindings_are_rejected_statically() {
        let mut checker = TypeChecker::new();
        let error = checker
            .check_module(&parse("import one as shared\nimport two as shared\n").unwrap())
            .expect_err("duplicate imports must fail static checking");
        assert!(error
            .message
            .contains("duplicate imported binding 'shared'"));
    }

    #[test]
    fn canonical_types_flatten_sort_and_deduplicate_unions() {
        let ty = Type::Union(vec![
            Type::Int,
            Type::Union(vec![Type::Float, Type::Int]),
            Type::Float,
        ]);
        assert_eq!(ty.canonical(), Type::Union(vec![Type::Float, Type::Int]));
    }

    #[test]
    fn canonical_types_normalize_nested_function_views() {
        let ty = Type::Function {
            params: vec![Type::View {
                mutability: MutabilityView::ReadOnly,
                inner: Box::new(Type::Union(vec![Type::Int, Type::Float])),
            }],
            return_type: Box::new(Type::Future(Box::new(Type::Int))),
        };
        let canonical = ty.canonical();
        assert!(
            matches!(canonical, Type::Function { params, .. } if matches!(&params[0], Type::View { inner, .. } if matches!(inner.as_ref(), Type::Union(parts) if parts.len() == 2)))
        );
    }

    #[test]
    fn canonical_type_text_is_independent_of_class_member_order() {
        let first = Type::Class {
            name: "Box".into(),
            type_args: vec![],
            parent: None,
            traits: vec!["Sized".into(), "Iterable".into(), "Sized".into()],
            interfaces: vec!["Readable".into(), "Writable".into(), "Readable".into()],
            fields: HashMap::from([("z".into(), Type::Int), ("a".into(), Type::Float)]),
            is_sealed: false,
        };
        let second = Type::Class {
            name: "Box".into(),
            type_args: vec![],
            parent: None,
            traits: vec!["Iterable".into(), "Sized".into()],
            interfaces: vec!["Writable".into(), "Readable".into()],
            fields: HashMap::from([("a".into(), Type::Float), ("z".into(), Type::Int)]),
            is_sealed: false,
        };
        assert_eq!(first.canonical(), second.canonical());
        assert_eq!(first.canonical_string(), second.canonical_string());
    }

    #[test]
    fn assert_accepts_lazy_string_message_and_rejects_other_types() {
        let valid = parse("assert(false, def() -> str: \"failed\")\n").unwrap();
        let valid_result = TypeChecker::new().check_module(&valid);
        assert!(
            valid_result.is_ok(),
            "valid lazy assert failed: {:?}",
            valid_result.err()
        );

        let invalid = parse("assert(false, 42)\n").unwrap();
        let error = TypeChecker::new().check_module(&invalid).unwrap_err();
        assert!(error.message.contains("assert message must be str"));
    }

    #[test]
    fn instance_checks_reject_disjoint_closed_types() {
        let module = parse("value: none = none\nresult = value is int\n").unwrap();
        let error = TypeChecker::new().check_module(&module).unwrap_err();
        assert!(error.message.contains("can never succeed"));

        let valid = parse("value: int = 1\nresult = value is int\n").unwrap();
        assert!(TypeChecker::new().check_module(&valid).is_ok());

        let negated = parse("value: none = none\nresult = value is not int\n").unwrap();
        assert!(TypeChecker::new().check_module(&negated).is_ok());

        let callable = parse("def f() -> int:\n    return 1\nresult = f is Callable\n").unwrap();
        assert!(TypeChecker::new().check_module(&callable).is_ok());
    }

    #[test]
    fn recursive_anonymous_bindings_are_visible_inside_their_body() {
        let module = parse(
            "def make() -> (int) -> int:\n    fact = def(n: int) -> int: 1 if n == 0 else n * fact(n - 1)\n    return fact\nrun = make()\nresult = run(5)\n",
        )
        .unwrap();
        TypeChecker::new().check_module(&module).unwrap();

        let annotated = parse(
            "def make() -> (int) -> int:\n    fact: (int) -> int = def(n: int) -> int: 1 if n == 0 else n * fact(n - 1)\n    return fact\nrun = make()\nresult = run(5)\n",
        )
        .unwrap();
        TypeChecker::new().check_module(&annotated).unwrap();
    }

    #[test]
    fn unannotated_functions_infer_straight_line_return_type() {
        let module = parse(
            "def make():\n    fact = def(n: int) -> int: 1 if n == 0 else n * fact(n - 1)\n    return fact\nrun = make()\nresult = run(5)\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        checker.check_module(&module).unwrap();
        assert!(matches!(
            checker.env.variables.get("result"),
            Some((Type::Int, _))
        ));

        let value = parse("def value():\n    temp = 40 + 2\n    return temp\nresult = value()\n")
            .unwrap();
        let mut checker = TypeChecker::new();
        checker.check_module(&value).unwrap();
        assert!(matches!(
            checker.env.variables.get("result"),
            Some((Type::Int, _))
        ));

        let recoverable = parse(
            "def step_one(x: int):\n    if x > 0:\n        return x * 2\n    return \"error\"\ndef pipeline(x: int):\n    value = step_one(x)?\n    return value + 10\nresult = pipeline(5)\n",
        )
        .unwrap();
        let mut checker = TypeChecker::new();
        checker.check_module(&recoverable).unwrap();
        assert!(matches!(
            checker.env.variables.get("result"),
            Some((Type::Int, _))
        ));
    }

    #[test]
    fn gather_parameter_must_be_final() {
        let module = parse(
            "class Arguments:\n    vpargs: list[int]\n    kwargs: dict[str, int]\ndef invalid(***rest: Arguments, tail: int) -> int:\n    return tail\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&module).unwrap_err();
        assert!(error.message.contains("must be the function's final parameter"));
    }

    #[test]
    fn method_gather_parameter_must_be_final() {
        let module = parse(
            "class Arguments:\n    vpargs: list[int]\n    kwargs: dict[str, int]\nclass Worker:\n    def invalid(self, ***rest: Arguments, tail: int) -> int:\n        return tail\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&module).unwrap_err();
        assert!(error.message.contains("must be the method's final parameter"));
    }

    #[test]
    fn method_calls_reject_surplus_arguments() {
        let module = parse(
            "class Worker:\n    def run(self, value: int) -> int:\n        return value\nw = Worker()\nresult = w.run(1, 2)\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&module).unwrap_err();
        assert!(error.message.contains("method accepts fewer arguments"));

        let gather = parse(
            "class Arguments:\n    vpargs: list[int]\n    kwargs: dict[str, int]\nclass Worker:\n    def run(self, ***rest: Arguments) -> int:\n        return len(rest.vpargs)\nw = Worker()\nresult = w.run(1, 2)\n",
        )
        .unwrap();
        assert!(TypeChecker::new().check_module(&gather).is_ok());

    }

    #[test]
    fn gather_parameter_accepts_leftover_keyword_arguments() {
        let module = parse(
            "class Arguments:\n    vpargs: list[int]\n    kwargs: dict[str, int]\ndef f(left: int, right: int, ***rest: Arguments) -> int:\n    return left + right + len(rest.vpargs) + len(rest.kwargs)\nresult = f(1, 2, 3, x=4)\n",
        )
        .unwrap();
        assert!(TypeChecker::new().check_module(&module).is_ok());

        let no_gather = parse(
            "def f(left: int, right: int) -> int:\n    return left + right\nresult = f(1, 2, x=4)\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&no_gather).unwrap_err();
        assert!(error.message.contains("no parameter named 'x'"));
    }

    #[test]
    fn parameters_after_positional_only_marker_accept_keywords() {
        let module = parse(
            "def combine(left: int, /, right: int) -> int:\n    return left + right\nresult = combine(1, right=2)\n",
        )
        .unwrap();
        assert!(TypeChecker::new().check_module(&module).is_ok());
    }

    #[test]
    fn named_calls_reject_duplicate_and_late_positional_arguments() {
        let duplicate = parse(
            "def f(value: int) -> int:\n    return value\nresult = f(value=1, value=2)\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&duplicate).unwrap_err();
        assert!(error.message.contains("passed more than once"));

        let late = parse(
            "def f(left: int, right: int) -> int:\n    return left + right\nresult = f(left=1, 2)\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&late).unwrap_err();
        assert!(error.message.contains("positional argument follows named"));

        let late_spread = parse(
            "def f(left: int, right: int, tail: int) -> int:\n    return left + right + tail\nresult = f(tail=3, *[1, 2])\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&late_spread).unwrap_err();
        assert!(error.message.contains("positional argument follows named"));

        let spread_before_keyword = parse(
            "def f(left: int, right: int, tail: int) -> int:\n    return left + right + tail\nresult = f(*[1, 2], tail=3)\n",
        )
        .unwrap();
        assert!(TypeChecker::new()
            .check_module(&spread_before_keyword)
            .is_ok());

        let late_gather_spread = parse(
            "class Arguments:\n    vpargs: list[int]\n    kwargs: dict[str, int]\ndef f(left: int, right: int, ***rest: Arguments) -> int:\n    return left + right\nresult = f(left=1, ***missing)\n",
        )
        .unwrap();
        let error = TypeChecker::new()
            .check_module(&late_gather_spread)
            .unwrap_err();
        assert!(error.message.contains("positional argument follows named"));

        let positional_only = parse(
            "def f(value: int, /) -> int:\n    return value\nresult = f(value=1)\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&positional_only).unwrap_err();
        assert!(error.message.contains("positional-only argument"));

        let keyword_only = parse(
            "def f(*, value: int) -> int:\n    return value\nresult = f(1)\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&keyword_only).unwrap_err();
        assert!(error.message.contains("keyword-only argument"));

        let too_many = parse(
            "def f(value: int) -> int:\n    return value\nresult = f(1, 2)\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&too_many).unwrap_err();
        assert!(error.message.contains("accepts at most"));

        let too_few = parse(
            "def f(left: int, right: int) -> int:\n    return left + right\nresult = f(1)\n",
        )
        .unwrap();
        let error = TypeChecker::new().check_module(&too_few).unwrap_err();
        assert!(error.message.contains("requires at least") || error.message.contains("required argument"));
    }
}
