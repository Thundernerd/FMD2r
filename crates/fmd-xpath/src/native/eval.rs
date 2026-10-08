//! Evaluates an [`Expr`] the way internettools does: XPath 3.1 with lenient typing
//! (`strictTypeChecking` off), the `case-insensitive-clever` default collation, JSONiq
//! objects and arrays, and the PXP JSON extensions (child and descendant steps match object
//! properties; internettools data/xquery.pas:592-594, :8376-8414).

use std::rc::Rc;

use rust_decimal::Decimal;

use super::dom::NodeKind;
use super::functions;
use super::syntax::{Axis, BinOp, CmpOp, Expr, Key, NodeTest, SeqType};
use super::value::{
    Cmp, Item, NodeRef, Object, Seq, XResult, compare, ebv, err, parse_decimal, parse_double,
    seq_string, to_decimal, trim,
};

/// The focus: context item, position and size.
#[derive(Clone)]
pub(crate) struct Focus {
    pub(crate) item: Option<Item>,
    pub(crate) position: usize,
    pub(crate) size: usize,
}

impl Focus {
    pub(crate) fn of(item: Option<Item>) -> Focus {
        Focus {
            item,
            position: 1,
            size: 1,
        }
    }

    pub(crate) fn item(&self) -> XResult<&Item> {
        match &self.item {
            Some(item) => Ok(item),
            None => err("XPDY0002: no context item"),
        }
    }
}

/// Variables in scope; the innermost binding wins.
#[derive(Default)]
pub(crate) struct Evaluator {
    vars: Vec<(String, Seq)>,
}

impl Evaluator {
    pub(crate) fn eval(&mut self, expr: &Expr, focus: &Focus) -> XResult<Seq> {
        match expr {
            Expr::Str(s) => Ok(vec![Item::str(s.as_str())]),
            Expr::Int(i) => Ok(vec![Item::Int(*i)]),
            Expr::Dec(d) => match parse_decimal(d) {
                Some(d) => Ok(vec![Item::Dec(d)]),
                None => err(format!("FOAR0002: {d} is out of range")),
            },
            Expr::Dbl(f) => Ok(vec![Item::Dbl(*f)]),
            Expr::Bool(b) => Ok(vec![Item::Bool(*b)]),
            Expr::Null => Ok(vec![Item::Null]),
            Expr::Var(name) => match self.vars.iter().rev().find(|(n, _)| n == name) {
                Some((_, value)) => Ok(value.clone()),
                None => err(format!("XPST0008: unknown variable ${name}")),
            },
            Expr::Context => Ok(vec![focus.item()?.clone()]),
            Expr::Root => match focus.item()? {
                Item::Node(node) => Ok(vec![Item::Node(node.with_id(0))]),
                _ => err("XPTY0020: the context item is not a node"),
            },
            Expr::Sequence(items) => {
                let mut result = Vec::new();
                for item in items {
                    result.extend(self.eval(item, focus)?);
                }
                Ok(result)
            }
            Expr::Path(left, right) => {
                // `//name` on a JSON value: internettools searches the descendants.
                if let (
                    Expr::Step(Axis::DescendantOrSelf, NodeTest::Node, dos),
                    Expr::Step(Axis::Child, test, predicates),
                ) = (left.as_ref(), right.as_ref())
                    && dos.is_empty()
                    && let Some(item @ (Item::Object(_) | Item::Array(_))) = &focus.item
                {
                    let mut items = json_step(item, Axis::Descendant, test)?;
                    for predicate in predicates {
                        items = self.filter(items, predicate)?;
                    }
                    return Ok(items);
                }
                let items = self.eval(left, focus)?;
                let size = items.len();
                let mut result = Vec::new();
                for (i, item) in items.into_iter().enumerate() {
                    let inner = Focus {
                        item: Some(item),
                        position: i + 1,
                        size,
                    };
                    result.extend(self.eval(right, &inner)?);
                }
                Ok(document_order(result))
            }
            Expr::Step(axis, test, predicates) => {
                let item = focus.item()?;
                let mut items = step(item, *axis, test)?;
                for predicate in predicates {
                    items = self.filter(items, predicate)?;
                }
                if axis.is_reverse() {
                    items.reverse();
                }
                Ok(items)
            }
            Expr::Filter(base, predicate) => {
                let items = self.eval(base, focus)?;
                self.filter(items, predicate)
            }
            Expr::Call(name, args) => self.call(name, args, focus),
            Expr::DynCall(base, args) => {
                let target = self.eval(base, focus)?;
                let mut values = Vec::new();
                for arg in args {
                    values.push(self.eval(arg, focus)?);
                }
                let mut result = Vec::new();
                for item in &target {
                    result.extend(dynamic_call(item, &values)?);
                }
                Ok(result)
            }
            Expr::Lookup(base, key) => {
                let target = self.eval(base, focus)?;
                let keys = self.keys(key, focus)?;
                let mut result = Vec::new();
                for item in &target {
                    result.extend(lookup(item, keys.as_deref())?);
                }
                Ok(result)
            }
            Expr::UnaryLookup(key) => {
                let keys = self.keys(key, focus)?;
                lookup(focus.item()?, keys.as_deref())
            }
            Expr::Property(base, name) => {
                let target = self.eval(base, focus)?;
                Ok(target
                    .iter()
                    .flat_map(|item| property(item, name))
                    .collect())
            }
            Expr::SimpleMap(left, right) => {
                let items = self.eval(left, focus)?;
                let size = items.len();
                let mut result = Vec::new();
                for (i, item) in items.into_iter().enumerate() {
                    let inner = Focus {
                        item: Some(item),
                        position: i + 1,
                        size,
                    };
                    result.extend(self.eval(right, &inner)?);
                }
                Ok(result)
            }
            Expr::Binary(op, left, right) => self.binary(*op, left, right, focus),
            Expr::Neg(operand) => {
                let value = self.eval(operand, focus)?;
                match atomize_single(&value)? {
                    None => Ok(Vec::new()),
                    Some(item) => Ok(vec![arithmetic(BinOp::Sub, &Item::Int(0), &item)?]),
                }
            }
            Expr::If(condition, then, otherwise) => {
                if ebv(&self.eval(condition, focus)?)? {
                    self.eval(then, focus)
                } else {
                    self.eval(otherwise, focus)
                }
            }
            Expr::For(name, source, body) => {
                let items = self.eval(source, focus)?;
                let mut result = Vec::new();
                for item in items {
                    result.extend(self.with_var(name, vec![item], |e| e.eval(body, focus))?);
                }
                Ok(result)
            }
            Expr::Let(name, value, body) => {
                let value = self.eval(value, focus)?;
                self.with_var(name, value, |e| e.eval(body, focus))
            }
            Expr::Quantified(every, name, source, body) => {
                let items = self.eval(source, focus)?;
                for item in items {
                    let satisfied =
                        ebv(&self.with_var(name, vec![item], |e| e.eval(body, focus))?)?;
                    if satisfied != *every {
                        return Ok(vec![Item::Bool(satisfied)]);
                    }
                }
                Ok(vec![Item::Bool(*every)])
            }
            Expr::Object(entries) => {
                let mut object = Object::new();
                for (key, value) in entries {
                    let key = self.eval(key, focus)?;
                    let key = match atomize_single(&key)? {
                        Some(key) => key.string(),
                        None => return err("XPTY0004: an empty object key"),
                    };
                    let value = self.eval(value, focus)?;
                    object.insert(key, value);
                }
                Ok(vec![Item::Object(Rc::new(object))])
            }
            // JSONiq arrays: every item of every member expression is a member.
            Expr::SquareArray(members) => {
                let mut values = Vec::new();
                for member in members {
                    values.extend(self.eval(member, focus)?.into_iter().map(|i| vec![i]));
                }
                Ok(vec![Item::Array(Rc::new(values))])
            }
            Expr::CurlyArray(inner) => {
                let items = self.eval(inner, focus)?;
                Ok(vec![Item::Array(Rc::new(
                    items.into_iter().map(|i| vec![i]).collect(),
                ))])
            }
            Expr::InstanceOf(operand, ty) => {
                let value = self.eval(operand, focus)?;
                Ok(vec![Item::Bool(instance_of(&value, ty))])
            }
            Expr::Cast(operand, ty) => {
                let value = self.eval(operand, focus)?;
                match atomize_single(&value)? {
                    None if ty.occurrence == Some('?') => Ok(Vec::new()),
                    None => err("XPTY0004: casting the empty sequence"),
                    Some(item) => Ok(vec![cast(&item, &ty.name)?]),
                }
            }
            Expr::Castable(operand, ty) => {
                let value = self.eval(operand, focus)?;
                let castable = match atomize_single(&value) {
                    Ok(None) => ty.occurrence == Some('?'),
                    Ok(Some(item)) => cast(&item, &ty.name).is_ok(),
                    Err(_) => false,
                };
                Ok(vec![Item::Bool(castable)])
            }
        }
    }

    fn with_var<T>(
        &mut self,
        name: &str,
        value: Seq,
        body: impl FnOnce(&mut Self) -> XResult<T>,
    ) -> XResult<T> {
        self.vars.push((name.to_owned(), value));
        let result = body(self);
        self.vars.pop();
        result
    }

    /// Keeps the items `predicate` accepts: a number selects by position, anything else by
    /// its effective boolean value.
    fn filter(&mut self, items: Seq, predicate: &Expr) -> XResult<Seq> {
        if let Expr::Int(n) = predicate {
            let n = *n;
            return Ok(match usize::try_from(n) {
                Ok(n) if n >= 1 => items.into_iter().nth(n - 1).into_iter().collect(),
                _ => Vec::new(),
            });
        }
        let size = items.len();
        let mut kept = Vec::new();
        for (i, item) in items.into_iter().enumerate() {
            let focus = Focus {
                item: Some(item),
                position: i + 1,
                size,
            };
            let value = self.eval(predicate, &focus)?;
            let keep = match value.as_slice() {
                [n] if n.is_numeric() => n.to_double() == (i + 1) as f64,
                _ => ebv(&value)?,
            };
            if keep && let Some(item) = focus.item {
                kept.push(item);
            }
        }
        Ok(kept)
    }

    fn keys(&mut self, key: &Key, focus: &Focus) -> XResult<Option<Vec<Item>>> {
        Ok(match key {
            Key::Name(name) => Some(vec![Item::str(name.as_str())]),
            Key::Int(i) => Some(vec![Item::Int(*i)]),
            Key::Wildcard => None,
            Key::Expr(expr) => Some(self.eval(expr, focus)?.iter().map(Item::atomize).collect()),
        })
    }

    fn call(&mut self, name: &str, args: &[Expr], focus: &Focus) -> XResult<Seq> {
        let mut values = Vec::with_capacity(args.len());
        for arg in args {
            values.push(self.eval(arg, focus)?);
        }
        functions::call(self, name, values, focus)
    }

    fn binary(&mut self, op: BinOp, left: &Expr, right: &Expr, focus: &Focus) -> XResult<Seq> {
        match op {
            BinOp::Or => {
                let result = ebv(&self.eval(left, focus)?)? || ebv(&self.eval(right, focus)?)?;
                return Ok(vec![Item::Bool(result)]);
            }
            BinOp::And => {
                let result = ebv(&self.eval(left, focus)?)? && ebv(&self.eval(right, focus)?)?;
                return Ok(vec![Item::Bool(result)]);
            }
            _ => {}
        }
        let a = self.eval(left, focus)?;
        let b = self.eval(right, focus)?;
        match op {
            BinOp::General(cmp) => Ok(vec![Item::Bool(general_compare(&a, &b, cmp)?)]),
            BinOp::Value(cmp) => {
                let (Some(x), Some(y)) = (atomize_single(&a)?, atomize_single(&b)?) else {
                    return Ok(Vec::new());
                };
                let result = compare(&x, &y, true)?;
                Ok(vec![Item::Bool(accepts(cmp, result))])
            }
            BinOp::Is | BinOp::Precedes | BinOp::Follows => {
                let (Some(x), Some(y)) = (single_node(&a)?, single_node(&b)?) else {
                    return Ok(Vec::new());
                };
                let result = match op {
                    BinOp::Is => x.same(&y),
                    BinOp::Precedes => x.order(&y).is_lt(),
                    _ => x.order(&y).is_gt(),
                };
                Ok(vec![Item::Bool(result)])
            }
            // Each side's items joined, like internettools' non-strict string conversion.
            BinOp::Concat => Ok(vec![Item::str(seq_string(&a) + &seq_string(&b))]),
            BinOp::Range => {
                let (Some(x), Some(y)) = (atomize_single(&a)?, atomize_single(&b)?) else {
                    return Ok(Vec::new());
                };
                let (x, y) = (to_int(&x)?, to_int(&y)?);
                if y.saturating_sub(x) > 100_000_000 {
                    return err("FOAR0002: range too large");
                }
                Ok((x..=y).map(Item::Int).collect())
            }
            BinOp::Union | BinOp::Intersect | BinOp::Except => {
                let x = nodes(a)?;
                let y = nodes(b)?;
                let result: Vec<NodeRef> = match op {
                    BinOp::Union => x.into_iter().chain(y).collect(),
                    BinOp::Intersect => x
                        .into_iter()
                        .filter(|n| y.iter().any(|m| m.same(n)))
                        .collect(),
                    _ => x
                        .into_iter()
                        .filter(|n| !y.iter().any(|m| m.same(n)))
                        .collect(),
                };
                Ok(document_order(result.into_iter().map(Item::Node).collect()))
            }
            _ => {
                let (Some(x), Some(y)) = (atomize_single(&a)?, atomize_single(&b)?) else {
                    return Ok(Vec::new());
                };
                Ok(vec![arithmetic(op, &x, &y)?])
            }
        }
    }
}

/// Sorts a sequence of nodes into document order without duplicates; other sequences stay
/// as they are.
pub(crate) fn document_order(mut items: Seq) -> Seq {
    if items.len() < 2 || !items.iter().all(|i| matches!(i, Item::Node(_))) {
        return items;
    }
    items.sort_by(|a, b| match (a, b) {
        (Item::Node(x), Item::Node(y)) => x.order(y),
        _ => std::cmp::Ordering::Equal,
    });
    items.dedup_by(|a, b| match (a, b) {
        (Item::Node(x), Item::Node(y)) => x.same(y),
        _ => false,
    });
    items
}

fn nodes(items: Seq) -> XResult<Vec<NodeRef>> {
    items
        .into_iter()
        .map(|item| match item {
            Item::Node(node) => Ok(node),
            _ => err("XPTY0004: a set operation on a non-node"),
        })
        .collect()
}

fn single_node(items: &Seq) -> XResult<Option<NodeRef>> {
    match items.as_slice() {
        [] => Ok(None),
        [Item::Node(node)] => Ok(Some(node.clone())),
        _ => err("XPTY0004: expected one node"),
    }
}

/// Atomizes a sequence expected to hold at most one item (arrays count as their members).
pub(crate) fn atomize_single(items: &Seq) -> XResult<Option<Item>> {
    let mut atoms = atomize(items);
    match atoms.len() {
        0 => Ok(None),
        1 => Ok(atoms.pop()),
        _ => err("XPTY0004: expected a single value"),
    }
}

/// Atomizes a sequence: nodes become untyped strings, arrays their members.
pub(crate) fn atomize(items: &[Item]) -> Seq {
    let mut atoms = Vec::new();
    for item in items {
        match item {
            Item::Array(members) => {
                for member in members.iter() {
                    atoms.extend(atomize(member));
                }
            }
            other => atoms.push(other.atomize()),
        }
    }
    atoms
}

fn accepts(op: CmpOp, result: Cmp) -> bool {
    match op {
        CmpOp::Eq => result == Cmp::Equal,
        CmpOp::Ne => matches!(result, Cmp::Less | Cmp::Greater),
        CmpOp::Lt => result == Cmp::Less,
        CmpOp::Le => matches!(result, Cmp::Less | Cmp::Equal),
        CmpOp::Gt => result == Cmp::Greater,
        CmpOp::Ge => matches!(result, Cmp::Greater | Cmp::Equal),
    }
}

/// A general comparison: true when any pair of items compares as asked
/// (`compareGeneral`, internettools data/xquery.pas:7394-7432).
fn general_compare(a: &[Item], b: &[Item], op: CmpOp) -> XResult<bool> {
    let a = atomize(a);
    let b = atomize(b);
    for x in &a {
        for y in &b {
            if accepts(op, compare(x, y, false)?) {
                return Ok(true);
            }
        }
    }
    Ok(false)
}

/// Converts an operand of arithmetic to a number: strings (non-strict typing) become an
/// integer, decimal or double.
fn to_number(item: &Item) -> XResult<Item> {
    match item {
        Item::Int(_) | Item::Dec(_) | Item::Dbl(_) => Ok(item.clone()),
        Item::Str(s, _) => {
            let s = trim(s);
            Ok(if let Ok(i) = s.parse::<i64>() {
                Item::Int(i)
            } else if let Some(d) = parse_decimal(s) {
                Item::Dec(d)
            } else {
                Item::Dbl(parse_double(s).unwrap_or(f64::NAN))
            })
        }
        _ => err("XPTY0004: not a number"),
    }
}

pub(crate) fn to_int(item: &Item) -> XResult<i64> {
    match to_number(item)? {
        Item::Int(i) => Ok(i),
        _ => err("XPTY0004: expected an integer"),
    }
}

/// Arithmetic on two atomic values. Results that would raise a floating point exception in
/// FMD2 (overflow, division by zero, invalid operations) are errors, as on FMD2's threads
/// (crates/xpath-fpc/README.md, "Behaviour worth knowing").
pub(crate) fn arithmetic(op: BinOp, a: &Item, b: &Item) -> XResult<Item> {
    let (a, b) = (to_number(a)?, to_number(b)?);
    if let (Item::Dbl(_), _) | (_, Item::Dbl(_)) = (&a, &b) {
        let (x, y) = (a.to_double(), b.to_double());
        let result = match op {
            BinOp::Add => x + y,
            BinOp::Sub => x - y,
            BinOp::Mul => x * y,
            BinOp::Div => x / y,
            BinOp::Mod => x % y,
            BinOp::IDiv => {
                if y == 0.0 {
                    return err("FOAR0001: division by zero");
                }
                let q = (x / y).trunc();
                return match Decimal::try_from(q) {
                    Ok(q) => integer_or_decimal(q),
                    Err(_) => err("FOAR0002: overflow"),
                };
            }
            _ => return err("XPTY0004: not an arithmetic operator"),
        };
        // Division by zero gives INF or NaN; overflow raises.
        if op != BinOp::Div && !result.is_finite() && x.is_finite() && y.is_finite() {
            return err("FOAR0002: floating point exception");
        }
        return Ok(Item::Dbl(result));
    }
    if let (Item::Int(x), Item::Int(y)) = (&a, &b) {
        let (x, y) = (*x, *y);
        let result = match op {
            BinOp::Add => x.checked_add(y),
            BinOp::Sub => x.checked_sub(y),
            BinOp::Mul => x.checked_mul(y),
            BinOp::IDiv if y != 0 => x.checked_div(y),
            BinOp::Mod if y != 0 => x.checked_rem(y),
            _ => None,
        };
        if let Some(result) = result {
            return Ok(Item::Int(result));
        }
    }
    let (Some(x), Some(y)) = (to_decimal(&a), to_decimal(&b)) else {
        return err("FOAR0002: overflow");
    };
    let zero_divisor = || err("FOAR0001: division by zero");
    let result = match op {
        BinOp::Add => x.checked_add(y),
        BinOp::Sub => x.checked_sub(y),
        BinOp::Mul => x.checked_mul(y),
        BinOp::Div => {
            if y.is_zero() {
                return zero_divisor();
            }
            // internettools divides decimals to 18 fractional digits.
            x.checked_div(y).map(|q| q.round_dp(18))
        }
        BinOp::IDiv => {
            if y.is_zero() {
                return zero_divisor();
            }
            x.checked_div(y).map(|q| q.trunc())
        }
        BinOp::Mod => {
            if y.is_zero() {
                return zero_divisor();
            }
            x.checked_rem(y)
        }
        _ => return err("XPTY0004: not an arithmetic operator"),
    };
    match result {
        Some(d) if matches!(op, BinOp::IDiv) => integer_or_decimal(d),
        Some(d) => Ok(Item::Dec(d)),
        None => err("FOAR0002: overflow"),
    }
}

fn integer_or_decimal(d: Decimal) -> XResult<Item> {
    use rust_decimal::prelude::ToPrimitive;
    Ok(match d.to_i64() {
        Some(i) => Item::Int(i),
        None => Item::Dec(d),
    })
}

/// The nodes (or JSON values) an axis step selects from `item`, in axis order.
fn step(item: &Item, axis: Axis, test: &NodeTest) -> XResult<Seq> {
    match item {
        Item::Node(node) => Ok(axis_nodes(node, axis)
            .into_iter()
            .filter(|&id| matches_test(node, id, axis, test))
            .map(|id| Item::Node(node.with_id(id)))
            .collect()),
        Item::Object(_) | Item::Array(_) => match test {
            NodeTest::Name(_) | NodeTest::AnyName | NodeTest::Node => json_step(item, axis, test),
            _ => err("XPTY0020: a kind test on a JSON value"),
        },
        _ => err("XPTY0020: an axis step on a value that is not a node"),
    }
}

/// Node ids along `axis` from `node`, in axis order.
fn axis_nodes(node: &NodeRef, axis: Axis) -> Vec<usize> {
    let dom = &node.dom;
    let id = node.id;
    let n = dom.node(id);
    let is_attr = |i: usize| matches!(dom.node(i).kind, NodeKind::Attribute { .. });
    match axis {
        Axis::Child => n.children.clone(),
        Axis::Attribute => n.attributes.clone(),
        Axis::SelfNode => vec![id],
        Axis::Descendant => (id + 1..=n.last).filter(|&i| !is_attr(i)).collect(),
        Axis::DescendantOrSelf => std::iter::once(id)
            .chain((id + 1..=n.last).filter(|&i| !is_attr(i)))
            .collect(),
        Axis::Parent => n.parent.into_iter().collect(),
        Axis::Ancestor | Axis::AncestorOrSelf => {
            let mut result = Vec::new();
            if axis == Axis::AncestorOrSelf {
                result.push(id);
            }
            let mut current = n.parent;
            while let Some(p) = current {
                result.push(p);
                current = dom.node(p).parent;
            }
            result
        }
        Axis::FollowingSibling | Axis::PrecedingSibling => {
            if is_attr(id) {
                return Vec::new();
            }
            let Some(parent) = n.parent else {
                return Vec::new();
            };
            let siblings = &dom.node(parent).children;
            let index = siblings.iter().position(|&s| s == id).unwrap_or(0);
            if axis == Axis::FollowingSibling {
                siblings[index + 1..].to_vec()
            } else {
                siblings[..index].iter().rev().copied().collect()
            }
        }
        Axis::Following => {
            let start = if is_attr(id) {
                n.parent.map(|p| p + 1).unwrap_or(id + 1)
            } else {
                n.last + 1
            };
            let end = dom.node(0).last;
            (start..=end).filter(|&i| !is_attr(i)).collect()
        }
        Axis::Preceding => {
            let anchor = if is_attr(id) {
                n.parent.unwrap_or(id)
            } else {
                id
            };
            let mut ancestors = Vec::new();
            let mut current = dom.node(anchor).parent;
            while let Some(p) = current {
                ancestors.push(p);
                current = dom.node(p).parent;
            }
            (1..anchor)
                .rev()
                .filter(|&i| !is_attr(i) && !ancestors.contains(&i))
                .collect()
        }
    }
}

fn matches_test(node: &NodeRef, id: usize, axis: Axis, test: &NodeTest) -> bool {
    let kind = &node.dom.node(id).kind;
    let principal = |kind: &NodeKind| {
        if axis == Axis::Attribute {
            matches!(kind, NodeKind::Attribute { .. })
        } else {
            matches!(kind, NodeKind::Element(_))
        }
    };
    let name_is = |name: &str| node.dom.name(id).eq_ignore_ascii_case(name);
    match test {
        NodeTest::Name(name) => principal(kind) && name_is(name),
        NodeTest::AnyName => principal(kind),
        NodeTest::Node => true,
        NodeTest::Text => matches!(kind, NodeKind::Text(_)),
        NodeTest::Element(name) => {
            matches!(kind, NodeKind::Element(_)) && name.as_deref().is_none_or(name_is)
        }
        NodeTest::AttributeTest(name) => {
            matches!(kind, NodeKind::Attribute { .. }) && name.as_deref().is_none_or(name_is)
        }
        NodeTest::Document => matches!(kind, NodeKind::Document),
        NodeTest::Nothing => false,
    }
}

/// An axis step on a JSON object or array (the PXP JSON extensions): `child::name` reads a
/// property (of every object in an array), `descendant::name` reads it at any depth.
fn json_step(item: &Item, axis: Axis, test: &NodeTest) -> XResult<Seq> {
    match axis {
        Axis::SelfNode => Ok(match test {
            NodeTest::Node => vec![item.clone()],
            _ => Vec::new(),
        }),
        Axis::Child => Ok(json_children(item, test)),
        Axis::Descendant => {
            let mut result = Vec::new();
            json_descendants(item, test, &mut result);
            Ok(result)
        }
        // internettools has no `descendant-or-self::node()` on JSON; `//name` is a
        // descendant search (see `Expr::Path`).
        Axis::DescendantOrSelf => err("XPTY0020: descendant-or-self on a JSON value"),
        _ => Ok(Vec::new()),
    }
}

fn key_matches(test: &NodeTest, key: &str) -> bool {
    match test {
        NodeTest::Name(name) => name == key,
        _ => true,
    }
}

/// The property values of an object, or of the objects in an array.
fn json_children(item: &Item, test: &NodeTest) -> Seq {
    match item {
        Item::Object(object) => object
            .iter()
            .filter(|(key, _)| key_matches(test, key))
            .flat_map(|(_, value)| value.iter().cloned())
            .collect(),
        Item::Array(members) => members
            .iter()
            .flatten()
            .flat_map(|member| json_children(member, test))
            .collect(),
        _ => Vec::new(),
    }
}

/// The property values at any depth, each before the values inside it; arrays are
/// transparent.
fn json_descendants(item: &Item, test: &NodeTest, result: &mut Seq) {
    match item {
        Item::Object(object) => {
            for (key, value) in object.iter() {
                for child in value {
                    if key_matches(test, key) {
                        result.push(child.clone());
                    }
                    json_descendants(child, test, result);
                }
            }
        }
        Item::Array(members) => {
            for member in members.iter().flatten() {
                json_descendants(member, test, result);
            }
        }
        _ => {}
    }
}

/// Dot notation `.name`: the property of an object; nothing for anything else.
fn property(item: &Item, name: &str) -> Seq {
    match item {
        Item::Object(object) => object.get(name).cloned().unwrap_or_default(),
        _ => Vec::new(),
    }
}

/// Calling an object or array (JSONiq): `$o("key")` reads a property, `$o()` lists the keys,
/// `$a(2)` reads a member, `$a()` lists the members.
fn dynamic_call(item: &Item, args: &[Seq]) -> XResult<Seq> {
    match (item, args) {
        (Item::Object(object), []) => Ok(object.keys().map(|k| Item::str(k.as_str())).collect()),
        (Item::Object(object), [key]) => Ok(match atomize_single(key)? {
            Some(key) => object.get(&key.string()).cloned().unwrap_or_default(),
            None => Vec::new(),
        }),
        (Item::Array(members), []) => Ok(members.iter().flatten().cloned().collect()),
        (Item::Array(members), [index]) => Ok(match atomize_single(index)? {
            Some(Item::Int(index)) => member(members, index),
            _ => Vec::new(),
        }),
        // internettools calls anything else to no result.
        _ => Ok(Vec::new()),
    }
}

fn member(members: &[Seq], index: i64) -> Seq {
    usize::try_from(index)
        .ok()
        .and_then(|i| i.checked_sub(1))
        .and_then(|i| members.get(i))
        .cloned()
        .unwrap_or_default()
}

/// The `?` lookup; `keys` is `None` for `?*`.
fn lookup(item: &Item, keys: Option<&[Item]>) -> XResult<Seq> {
    match item {
        Item::Object(object) => Ok(match keys {
            None => object.values().flatten().cloned().collect(),
            Some(keys) => keys
                .iter()
                .flat_map(|k| object.get(&k.string()).cloned().unwrap_or_default())
                .collect(),
        }),
        Item::Array(members) => match keys {
            None => Ok(members.iter().flatten().cloned().collect()),
            Some(keys) => {
                let mut result = Vec::new();
                for key in keys {
                    let Item::Int(i) = key else {
                        return err("XPTY0004: an array key must be an integer");
                    };
                    match usize::try_from(*i).ok().and_then(|i| i.checked_sub(1)) {
                        Some(i) if i < members.len() => result.extend(members[i].iter().cloned()),
                        _ => return err("FOAY0001: array index out of bounds"),
                    }
                }
                Ok(result)
            }
        },
        _ => err("XPTY0004: a lookup on a value that is neither an object nor an array"),
    }
}

/// `instance of` for the types modules can meet.
fn instance_of(value: &[Item], ty: &SeqType) -> bool {
    let count_ok = match ty.occurrence {
        None => value.len() == 1,
        Some('?') => value.len() <= 1,
        Some('+') => !value.is_empty(),
        _ => true,
    };
    count_ok && value.iter().all(|item| item_is(item, &ty.name))
}

fn item_is(item: &Item, name: &str) -> bool {
    let name = name.strip_prefix("xs:").unwrap_or(name);
    match name {
        "item" => true,
        "anyAtomicType" => !matches!(
            item,
            Item::Node(_) | Item::Object(_) | Item::Array(_) | Item::Null
        ),
        "node" => matches!(item, Item::Node(_)),
        "element" => matches!(item, Item::Node(n) if matches!(n.kind(), NodeKind::Element(_))),
        "attribute" => {
            matches!(item, Item::Node(n) if matches!(n.kind(), NodeKind::Attribute { .. }))
        }
        "text" => matches!(item, Item::Node(n) if matches!(n.kind(), NodeKind::Text(_))),
        "document-node" => matches!(item, Item::Node(n) if matches!(n.kind(), NodeKind::Document)),
        "string" => matches!(item, Item::Str(_, super::value::StrType::String)),
        "untypedAtomic" => matches!(item, Item::Str(_, super::value::StrType::Untyped)),
        "anyURI" => matches!(item, Item::Str(_, super::value::StrType::AnyUri)),
        "integer" | "int" | "long" => matches!(item, Item::Int(_)),
        "decimal" => matches!(item, Item::Int(_) | Item::Dec(_)),
        "double" => matches!(item, Item::Dbl(_)),
        "numeric" => item.is_numeric(),
        "boolean" => matches!(item, Item::Bool(_)),
        "object" | "map" | "json-item" if matches!(item, Item::Object(_)) => true,
        "array" | "json-item" if matches!(item, Item::Array(_)) => true,
        "null" => matches!(item, Item::Null),
        _ => false,
    }
}

/// Casts an atomic value to `xs:string`, `xs:integer`, `xs:decimal`, `xs:double`,
/// `xs:boolean` or `xs:untypedAtomic`.
pub(crate) fn cast(item: &Item, ty: &str) -> XResult<Item> {
    let ty = ty.strip_prefix("xs:").unwrap_or(ty);
    match ty {
        "string" => Ok(Item::str(item.string())),
        "untypedAtomic" => Ok(Item::untyped(item.string())),
        "anyURI" => Ok(Item::Str(
            item.string().into(),
            super::value::StrType::AnyUri,
        )),
        "double" | "float" => match item {
            Item::Bool(b) => Ok(Item::Dbl(f64::from(u8::from(*b)))),
            Item::Int(_) | Item::Dec(_) | Item::Dbl(_) => Ok(Item::Dbl(item.to_double())),
            _ => match parse_double(&item.string()) {
                Some(f) => Ok(Item::Dbl(f)),
                None => err(format!(
                    "FORG0001: cannot cast {:?} to xs:double",
                    item.string()
                )),
            },
        },
        "decimal" => match item {
            Item::Bool(b) => Ok(Item::Dec(Decimal::from(u8::from(*b)))),
            Item::Int(_) | Item::Dec(_) | Item::Dbl(_) => match to_decimal(item) {
                Some(d) => Ok(Item::Dec(d)),
                None => err("FOCA0002: cannot cast to xs:decimal"),
            },
            _ => match parse_decimal(&item.string()) {
                Some(d) => Ok(Item::Dec(d)),
                None => err(format!(
                    "FORG0001: cannot cast {:?} to xs:decimal",
                    item.string()
                )),
            },
        },
        "integer" | "int" | "long" => match item {
            Item::Bool(b) => Ok(Item::Int(i64::from(*b))),
            Item::Int(_) => Ok(item.clone()),
            Item::Dec(d) => integer_or_decimal(d.trunc()),
            Item::Dbl(f) if f.is_finite() => match Decimal::try_from(f.trunc()) {
                Ok(d) => integer_or_decimal(d),
                Err(_) => err("FOCA0003: too large for xs:integer"),
            },
            Item::Dbl(_) => err("FOCA0002: cannot cast to xs:integer"),
            _ => {
                let s = item.string();
                let t = trim(&s);
                match t.strip_prefix('+').unwrap_or(t).parse::<i64>() {
                    Ok(i) => Ok(Item::Int(i)),
                    Err(_) => err(format!("FORG0001: cannot cast {s:?} to xs:integer")),
                }
            }
        },
        "boolean" => match item {
            Item::Bool(_) => Ok(item.clone()),
            Item::Int(_) | Item::Dec(_) | Item::Dbl(_) => Ok(Item::Bool(item.ebv()?)),
            _ => Ok(Item::Bool(super::value::string_to_bool(&item.string())?)),
        },
        _ => err(format!("XPST0051: unknown type {ty}")),
    }
}
