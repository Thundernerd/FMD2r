//! The function library: the XPath functions modules use, internettools' extensions
//! (`json`, `css`, `join`, `split-equal`, `is-nth`, ...) and the JSONiq `jn:` functions.
//!
//! Without strict type checking, internettools hands string parameters a sequence's
//! concatenated string (`contains(//li, "x")` searches all items' text joined), so most
//! functions here take `text(arg)`.

use std::rc::Rc;

use rust_decimal::Decimal;
use rust_decimal::prelude::ToPrimitive;

use super::css;
use super::dom::NodeKind;
use super::eval::{Evaluator, Focus, arithmetic, atomize, atomize_single, cast, document_order};
use super::json;
use super::syntax::BinOp;
use super::value::{
    Cmp, Item, NodeRef, Seq, XResult, collation_compare, collation_find, compare, ebv, err,
    seq_string, trim,
};

/// A string parameter: the argument's items' strings, concatenated.
fn text(arg: &[Item]) -> String {
    seq_string(arg)
}

fn boolean(b: bool) -> XResult<Seq> {
    Ok(vec![Item::Bool(b)])
}

fn string(s: impl Into<Rc<str>>) -> XResult<Seq> {
    Ok(vec![Item::str(s)])
}

fn integer(i: usize) -> XResult<Seq> {
    Ok(vec![Item::Int(i64::try_from(i).unwrap_or(i64::MAX))])
}

/// The context item, for functions whose argument defaults to it.
fn context(focus: &Focus) -> XResult<Seq> {
    Ok(vec![focus.item()?.clone()])
}

/// The node of a one-node argument; `None` for the empty sequence.
fn node_arg(arg: &[Item]) -> XResult<Option<NodeRef>> {
    match arg {
        [] => Ok(None),
        [Item::Node(node)] => Ok(Some(node.clone())),
        _ => err("XPTY0004: expected one node"),
    }
}

/// A number parameter: atomized and converted; `None` for the empty sequence.
fn number_arg(arg: &[Item]) -> XResult<Option<Item>> {
    match atomize_single(&arg.to_vec())? {
        None => Ok(None),
        Some(item) if item.is_numeric() => Ok(Some(item)),
        Some(Item::Str(s, _)) => Ok(Some(Item::Dbl(
            super::value::parse_double(&s).unwrap_or(f64::NAN),
        ))),
        Some(_) => err("XPTY0004: expected a number"),
    }
}

fn double_arg(arg: &[Item]) -> XResult<f64> {
    Ok(number_arg(arg)?.map_or(f64::NAN, |n| n.to_double()))
}

/// Calls the function `name` with evaluated arguments.
pub(crate) fn call(ev: &mut Evaluator, name: &str, args: Vec<Seq>, focus: &Focus) -> XResult<Seq> {
    let local = name
        .strip_prefix("fn:")
        .or_else(|| name.strip_prefix("pxp:"))
        .or_else(|| name.strip_prefix("x:"))
        .unwrap_or(name);
    let args = args.as_slice();
    match (local, args) {
        // Focus.
        ("position", []) => integer(focus.position),
        ("last", []) => integer(focus.size),
        ("true", []) => boolean(true),
        ("false", []) => boolean(false),

        // Sequences.
        ("count", [a]) => integer(a.len()),
        ("exists", [a]) => boolean(!a.is_empty()),
        ("empty", [a]) => boolean(a.is_empty()),
        ("not", [a]) => boolean(!ebv(a)?),
        ("boolean", [a]) => boolean(ebv(a)?),
        ("data", [a]) => Ok(atomize(a)),
        ("data", []) => Ok(atomize(&context(focus)?)),
        ("reverse", [a]) => Ok(a.iter().rev().cloned().collect()),
        ("head", [a]) => Ok(a.iter().take(1).cloned().collect()),
        ("tail", [a]) => Ok(a.iter().skip(1).cloned().collect()),
        ("unordered" | "trace", [a, ..]) => Ok(a.clone()),
        ("zero-or-one", [a]) if a.len() <= 1 => Ok(a.clone()),
        ("one-or-more", [a]) if !a.is_empty() => Ok(a.clone()),
        ("exactly-one", [a]) if a.len() == 1 => Ok(a.clone()),
        ("subsequence", [a, start]) => subsequence(a, double_arg(start)?, f64::INFINITY),
        ("subsequence", [a, start, length]) => {
            subsequence(a, double_arg(start)?, double_arg(length)?)
        }
        // internettools data/xquery__functions.pas:2660.
        ("insert-before", [a, position, inserts]) => {
            let position = double_arg(position)?.max(1.0) as usize;
            let at = (position - 1).min(a.len());
            let mut result = a[..at].to_vec();
            result.extend(inserts.iter().cloned());
            result.extend(a[at..].iter().cloned());
            Ok(result)
        }
        ("remove", [a, position]) => {
            let position = double_arg(position)?;
            Ok(a.iter()
                .enumerate()
                .filter(|(i, _)| (*i + 1) as f64 != position)
                .map(|(_, item)| item.clone())
                .collect())
        }
        ("distinct-values", [a]) => {
            let mut result: Seq = Vec::new();
            for item in atomize(a) {
                if !result.iter().any(|kept| same_value(kept, &item)) {
                    result.push(item);
                }
            }
            Ok(result)
        }
        ("index-of", [a, searched]) => {
            let Some(searched) = atomize_single(searched)? else {
                return Ok(Vec::new());
            };
            Ok(atomize(a)
                .iter()
                .enumerate()
                .filter(|(_, item)| same_value(item, &searched))
                .map(|(i, _)| Item::Int(i64::try_from(i + 1).unwrap_or(i64::MAX)))
                .collect())
        }

        // Numbers.
        ("number", []) => number(&context(focus)?),
        ("number", [a]) => number(a),
        ("sum", [a]) => sum(a, Item::Int(0)),
        ("sum", [a, zero]) => match zero.first() {
            Some(zero) if a.is_empty() => Ok(vec![zero.clone()]),
            _ => sum(a, Item::Int(0)),
        },
        ("avg", [a]) => {
            if a.is_empty() {
                return Ok(Vec::new());
            }
            let total = sum(a, Item::Int(0))?;
            let count = Item::Int(i64::try_from(a.len()).unwrap_or(i64::MAX));
            match total.first() {
                Some(total) => Ok(vec![arithmetic(BinOp::Div, total, &count)?]),
                None => Ok(Vec::new()),
            }
        }
        ("min" | "max", [a]) => {
            let mut best: Option<Item> = None;
            for item in atomize(a) {
                let item = match item {
                    Item::Str(s, super::value::StrType::Untyped) => {
                        Item::Dbl(super::value::parse_double(&s).unwrap_or(f64::NAN))
                    }
                    other => other,
                };
                best = Some(match best {
                    None => item,
                    Some(current) => {
                        let order = compare(&item, &current, true)?;
                        let better = if local == "min" {
                            order == Cmp::Less
                        } else {
                            order == Cmp::Greater
                        };
                        if better { item } else { current }
                    }
                });
            }
            Ok(best.into_iter().collect())
        }
        ("abs" | "ceiling" | "floor" | "round" | "round-half-to-even", [a, rest @ ..]) => {
            let Some(n) = number_arg(a)? else {
                return Ok(Vec::new());
            };
            let precision = match rest {
                [p] => number_arg(p)?.map_or(0.0, |p| p.to_double()) as i32,
                _ => 0,
            };
            Ok(vec![rounding(local, &n, precision)?])
        }

        // Strings.
        ("string", []) => string(
            context(focus)?
                .first()
                .map(Item::string)
                .unwrap_or_default(),
        ),
        ("string", [a]) => string(text(a)),
        ("concat", args) if args.len() >= 2 => {
            let mut result = String::new();
            for arg in args {
                if let Some(item) = atomize_single(arg)? {
                    result.push_str(&item.string());
                }
            }
            string(result)
        }
        ("string-join", [a]) => string(join(a, "")),
        ("string-join", [a, separator]) => string(join(a, &text(separator))),
        // internettools data/xquery__functions.pas:1520-1537.
        ("join", [a]) => string(join(a, " ")),
        ("join", [a, separator]) => string(join(a, &text(separator))),
        ("string-length", []) => integer(text(&context(focus)?).chars().count()),
        ("string-length", [a]) => integer(text(a).chars().count()),
        ("normalize-space", []) => string(normalize_space(&text(&context(focus)?))),
        ("normalize-space", [a]) => string(normalize_space(&text(a))),
        ("upper-case", [a]) => string(text(a).to_uppercase()),
        ("lower-case", [a]) => string(text(a).to_lowercase()),
        // contains, starts-with, ends-with, substring-before/after use the default collation
        // (internettools data/xquery__functions.pas:1691-1746).
        ("contains", [a, b]) => boolean(collation_find(&text(a), &text(b)).is_some()),
        ("starts-with", [a, b]) => {
            let (a, b) = (text(a), text(b));
            boolean(
                a.len() >= b.len() && a.as_bytes()[..b.len()].eq_ignore_ascii_case(b.as_bytes()),
            )
        }
        ("ends-with", [a, b]) => {
            let (a, b) = (text(a), text(b));
            boolean(
                a.len() >= b.len()
                    && a.as_bytes()[a.len() - b.len()..].eq_ignore_ascii_case(b.as_bytes()),
            )
        }
        ("substring-before", [a, b]) => {
            let (a, b) = (text(a), text(b));
            if b.is_empty() {
                return string("");
            }
            string(match collation_find(&a, &b) {
                Some(at) => &a[..at],
                None => "",
            })
        }
        ("substring-after", [a, b]) => {
            let (a, b) = (text(a), text(b));
            if b.is_empty() {
                return string(a);
            }
            string(match collation_find(&a, &b) {
                Some(at) => &a[at + b.len()..],
                None => "",
            })
        }
        // internettools data/xquery__functions.pas:1539.
        ("substring", [a, start]) => string(substring(&text(a), double_arg(start)?, f64::INFINITY)),
        ("substring", [a, start, length]) => {
            string(substring(&text(a), double_arg(start)?, double_arg(length)?))
        }
        ("translate", [a, from, to]) => {
            let from: Vec<char> = text(from).chars().collect();
            let to: Vec<char> = text(to).chars().collect();
            string(
                text(a)
                    .chars()
                    .filter_map(|c| match from.iter().position(|&f| f == c) {
                        Some(i) => to.get(i).copied(),
                        None => Some(c),
                    })
                    .collect::<String>(),
            )
        }
        ("compare", [a, b, ..]) => {
            if a.is_empty() || b.is_empty() {
                return Ok(Vec::new());
            }
            Ok(vec![Item::Int(
                match collation_compare(&text(a), &text(b)) {
                    std::cmp::Ordering::Less => -1,
                    std::cmp::Ordering::Equal => 0,
                    std::cmp::Ordering::Greater => 1,
                },
            )])
        }
        ("codepoints-to-string", [a]) => {
            let mut s = String::new();
            for item in atomize(a) {
                let code = u32::try_from(super::eval::to_int(&item)?).ok();
                match code.and_then(char::from_u32) {
                    Some(c) => s.push(c),
                    None => return err("FOCH0001: not a character"),
                }
            }
            string(s)
        }
        ("string-to-codepoints", [a]) => Ok(text(a)
            .chars()
            .map(|c| Item::Int(i64::from(u32::from(c))))
            .collect()),
        ("split-equal", [list, searched, rest @ ..]) => {
            let separator = match rest {
                [s] => text(s),
                _ => " ".to_owned(),
            };
            let (list, searched) = (text(list), text(searched));
            boolean(
                list.split(separator.as_str())
                    .any(|part| collation_compare(part, &searched).is_eq()),
            )
        }
        ("contains-token", [a, token, ..]) => {
            let token = trim(&text(token)).to_owned();
            if token.is_empty() {
                return boolean(false);
            }
            boolean(atomize(a).iter().any(|item| {
                item.string()
                    .split([' ', '\t', '\n', '\r'])
                    .any(|part| collation_compare(part, &token).is_eq())
            }))
        }
        ("matches", [a, pattern, rest @ ..]) => {
            let regex = regex(&text(pattern), rest)?;
            boolean(regex.is_match(&text(a)))
        }
        ("replace", [a, pattern, replacement, rest @ ..]) => {
            let regex = regex(&text(pattern), rest)?;
            let replacement = replacement_template(&text(replacement))?;
            string(
                regex
                    .replace_all(&text(a), replacement.as_str())
                    .into_owned(),
            )
        }
        ("tokenize", [a]) => Ok(normalize_space(&text(a))
            .split(' ')
            .filter(|s| !s.is_empty())
            .map(Item::str)
            .collect()),
        ("tokenize", [a, pattern, rest @ ..]) => {
            let input = text(a);
            if input.is_empty() {
                return Ok(Vec::new());
            }
            let regex = regex(&text(pattern), rest)?;
            Ok(regex.split(&input).map(Item::str).collect())
        }
        ("encode-for-uri" | "uri-encode", [a]) => string(encode_uri(&text(a), false)),
        ("iri-to-uri" | "escape-html-uri", [a]) => string(encode_uri(&text(a), true)),
        ("uri-decode" | "decode-uri", [a]) => string(decode_uri(&text(a))?),
        ("normalize-unicode", [a, ..]) => string(text(a)),
        // internettools data/xquery__functions.pas:2471; without a base, FMD2 would use its
        // working directory (crates/fmd-xpath/README.md, "Known differences").
        ("resolve-uri", [relative, base @ ..]) => {
            if relative.is_empty() {
                return Ok(Vec::new());
            }
            let base = base.first().map(|b| text(b)).unwrap_or_default();
            string(resolve_uri(&base, &text(relative)))
        }
        ("static-base-uri" | "base-uri", _) => Ok(Vec::new()),

        // Nodes.
        ("name" | "local-name", []) => {
            node_name(local, focus.item.as_ref().and_then(Item::as_node))
        }
        ("name" | "local-name", [a]) => node_name(local, node_arg(a)?.as_ref()),
        ("root", []) => root(&context(focus)?),
        ("root", [a]) => root(a),
        // `deep-text($separator)` of the context node (internettools
        // data/xquery__functions.pas, `xqFunctionDeep_Node_Text`).
        ("deep-text", []) => string(deep_text(&context(focus)?, "")),
        ("deep-text", [separator]) => string(deep_text(&context(focus)?, &text(separator))),
        ("inner-text", []) => string(node_text(&context(focus)?, NodeRef::inner_text)),
        ("inner-text", [a]) => string(node_text(a, NodeRef::inner_text)),
        ("inner-html" | "inner-xml", []) => {
            string(node_text(&context(focus)?, NodeRef::inner_html))
        }
        ("inner-html" | "inner-xml", [a]) => string(node_text(a, NodeRef::inner_html)),
        ("outer-html" | "outer-xml", []) => {
            string(node_text(&context(focus)?, NodeRef::outer_html))
        }
        ("outer-html" | "outer-xml", [a]) => string(node_text(a, NodeRef::outer_html)),

        // CSS and internettools helpers.
        ("css", [selector]) => {
            let expr = css::translate(&text(selector))?;
            let result = ev.eval(&expr, &Focus::of(focus.item.clone()))?;
            Ok(document_order(result))
        }
        ("is-nth", [i, a, b]) => {
            let (i, a, b) = (int_arg(i)?, int_arg(a)?, int_arg(b)?);
            boolean(if a == 0 {
                i == b
            } else {
                let n = (i - b) / a;
                n >= 0 && i == a * n + b
            })
        }
        ("error", _) => err("FOER0000: fn:error"),

        // JSON.
        ("json" | "parse-json" | "jn:parse-json", [a, ..]) => {
            if local != "json" && a.is_empty() {
                return Ok(Vec::new());
            }
            json::parse(&text(a))
        }
        ("jn:keys" | "map:keys", [a]) => {
            let mut keys: Vec<String> = Vec::new();
            for item in a {
                if let Item::Object(object) = item {
                    for key in object.keys() {
                        if !keys.contains(key) {
                            keys.push(key.clone());
                        }
                    }
                }
            }
            Ok(keys.into_iter().map(Item::str).collect())
        }
        ("jn:members" | "array:flatten", [a]) => {
            let mut result = Vec::new();
            for item in a {
                if let Item::Array(members) = item {
                    result.extend(members.iter().flatten().cloned());
                } else if local == "array:flatten" {
                    result.push(item.clone());
                }
            }
            Ok(result)
        }
        ("jn:size" | "array:size" | "map:size", [a]) => match a.as_slice() {
            [] => Ok(Vec::new()),
            [Item::Array(members)] => integer(members.len()),
            [Item::Object(object)] => integer(object.len()),
            _ => err("XPTY0004: expected an array"),
        },
        // `object((key, value, ...))` (internettools data/xquery__functions.pas:1969-1990).
        ("object", [pairs]) => {
            if pairs.len() % 2 == 1 {
                return err("pxp:OBJECT: an odd number of items");
            }
            let mut object = super::value::Object::new();
            for pair in pairs.chunks(2) {
                match pair {
                    [Item::Str(key, _), value] => {
                        object.insert(key.to_string(), vec![value.clone()]);
                    }
                    _ => return err("pxp:OBJECT: property names must be strings"),
                }
            }
            Ok(vec![Item::Object(Rc::new(object))])
        }
        // `jn:object($objects...)` merges objects; a repeated key is an error
        // (internettools data/xquery_json.pas:68-90).
        ("jn:object", objects) => {
            let mut merged = super::value::Object::new();
            for item in objects.iter().flatten() {
                let Item::Object(object) = item else {
                    return err("XPTY0004: jn:object expects objects");
                };
                for (key, value) in object.iter() {
                    if merged.insert(key.clone(), value.clone()).is_some() {
                        return err(format!("jerr:JNDY0003: duplicate key {key}"));
                    }
                }
            }
            Ok(vec![Item::Object(Rc::new(merged))])
        }
        ("jn:null", []) => Ok(vec![Item::Null]),
        ("jn:is-null", [a]) => boolean(matches!(a.as_slice(), [Item::Null])),
        ("map:get", [map, key]) => match (map.as_slice(), atomize_single(key)?) {
            ([Item::Object(object)], Some(key)) => {
                Ok(object.get(&key.string()).cloned().unwrap_or_default())
            }
            _ => err("XPTY0004: expected a map"),
        },
        ("map:contains", [map, key]) => match (map.as_slice(), atomize_single(key)?) {
            ([Item::Object(object)], Some(key)) => boolean(object.contains_key(&key.string())),
            _ => err("XPTY0004: expected a map"),
        },
        ("array:get", [array, index]) => match (array.as_slice(), atomize_single(index)?) {
            ([Item::Array(members)], Some(index)) => {
                let i = super::eval::to_int(&index)?;
                match usize::try_from(i)
                    .ok()
                    .and_then(|i| i.checked_sub(1))
                    .and_then(|i| members.get(i))
                {
                    Some(member) => Ok(member.clone()),
                    None => err("FOAY0001: array index out of bounds"),
                }
            }
            _ => err("XPTY0004: expected an array"),
        },

        // Constructor functions.
        (ty, [a]) if ty.starts_with("xs:") => match atomize_single(a)? {
            None => Ok(Vec::new()),
            Some(item) => Ok(vec![cast(&item, ty)?]),
        },

        _ => err(format!("XPST0017: unknown function {name}#{}", args.len())),
    }
}

/// Equality for `distinct-values` and `index-of`: only comparable types can be equal
/// (`comparableTypes`, internettools data/xquery.pas:7452-7470), strings by the collation.
fn same_value(a: &Item, b: &Item) -> bool {
    let comparable = (a.is_numeric() && b.is_numeric())
        || matches!(
            (a, b),
            (Item::Str(..), Item::Str(..)) | (Item::Bool(_), Item::Bool(_))
        );
    comparable && compare(a, b, true).is_ok_and(|c| c == Cmp::Equal)
}

fn int_arg(arg: &[Item]) -> XResult<i64> {
    match atomize_single(&arg.to_vec())? {
        Some(item) => super::eval::to_int(&item),
        None => err("XPTY0004: expected an integer"),
    }
}

fn subsequence(a: &[Item], start: f64, length: f64) -> XResult<Seq> {
    let start = start.round();
    let end = start + length.round();
    Ok(a.iter()
        .enumerate()
        .filter(|(i, _)| {
            let p = (*i + 1) as f64;
            p >= start && p < end
        })
        .map(|(_, item)| item.clone())
        .collect())
}

/// `fn:number`: a double, NaN when not convertible.
fn number(a: &[Item]) -> XResult<Seq> {
    let value = match atomize(a).as_slice() {
        [] => f64::NAN,
        [item] => item.to_double(),
        _ => return err("XPTY0004: expected one value"),
    };
    Ok(vec![Item::Dbl(value)])
}

fn sum(a: &[Item], zero: Item) -> XResult<Seq> {
    let mut total = zero;
    for item in atomize(a) {
        let item = match item {
            Item::Str(s, super::value::StrType::Untyped) => {
                Item::Dbl(super::value::parse_double(&s).unwrap_or(f64::NAN))
            }
            Item::Str(..) => return err("FORG0006: sum of strings"),
            other => other,
        };
        total = arithmetic(BinOp::Add, &total, &item)?;
    }
    Ok(vec![total])
}

fn rounding(name: &str, n: &Item, precision: i32) -> XResult<Item> {
    Ok(match n {
        Item::Int(i) => match name {
            "abs" => Item::Int(
                i.checked_abs()
                    .ok_or_else(|| super::value::XPathError("FOAR0002: overflow".into()))?,
            ),
            "round" | "round-half-to-even" if precision < 0 => {
                let d = round_decimal(name, Decimal::from(*i), precision);
                d.to_i64().map_or(Item::Dec(d), Item::Int)
            }
            _ => Item::Int(*i),
        },
        Item::Dec(d) => Item::Dec(match name {
            "abs" => d.abs(),
            "ceiling" => d.ceil(),
            "floor" => d.floor(),
            _ => round_decimal(name, *d, precision),
        }),
        Item::Dbl(f) => Item::Dbl(match name {
            "abs" => f.abs(),
            "ceiling" => f.ceil(),
            "floor" => f.floor(),
            _ if !f.is_finite() => *f,
            _ => {
                let scale = 10f64.powi(precision);
                let scaled = f * scale;
                let rounded = if name == "round" {
                    (scaled + 0.5).floor()
                } else {
                    let r = scaled.round();
                    if (scaled - scaled.trunc()).abs() == 0.5 {
                        2.0 * (scaled / 2.0).round()
                    } else {
                        r
                    }
                };
                rounded / scale
            }
        }),
        _ => return err("XPTY0004: expected a number"),
    })
}

fn round_decimal(name: &str, d: Decimal, precision: i32) -> Decimal {
    use rust_decimal::RoundingStrategy;
    let strategy = if name == "round" {
        if d.is_sign_negative() {
            RoundingStrategy::MidpointTowardZero
        } else {
            RoundingStrategy::MidpointAwayFromZero
        }
    } else {
        RoundingStrategy::MidpointNearestEven
    };
    if precision >= 0 {
        d.round_dp_with_strategy(u32::try_from(precision).unwrap_or(0), strategy)
    } else {
        let factor = Decimal::from(10i64.pow(precision.unsigned_abs().min(18)));
        (d / factor).round_dp_with_strategy(0, strategy) * factor
    }
}

/// `string-join`: the atomized items' strings joined with `separator`.
fn join(a: &[Item], separator: &str) -> String {
    atomize(a)
        .iter()
        .map(Item::string)
        .collect::<Vec<_>>()
        .join(separator)
}

/// XPath's `normalize-space`: whitespace runs become one space, trimmed.
fn normalize_space(s: &str) -> String {
    s.split([' ', '\t', '\n', '\r'])
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join(" ")
}

/// `fn:substring` with XPath's rounding of positions.
fn substring(s: &str, start: f64, length: f64) -> String {
    let round = |x: f64| (x + 0.5).floor();
    let first = round(start);
    let end = first + round(length);
    s.chars()
        .enumerate()
        .filter(|(i, _)| {
            let p = (*i + 1) as f64;
            p >= first && p < end
        })
        .map(|(_, c)| c)
        .collect()
}

/// Compiles an XPath regular expression with its flags (`s`, `m`, `i`, `x`, `q`).
fn regex(pattern: &str, flags: &[Seq]) -> XResult<regex::Regex> {
    let flags = flags.first().map(|f| text(f)).unwrap_or_default();
    let mut builder = if flags.contains('q') {
        regex::RegexBuilder::new(&regex::escape(pattern))
    } else {
        regex::RegexBuilder::new(pattern)
    };
    builder
        .case_insensitive(flags.contains('i'))
        .dot_matches_new_line(flags.contains('s'))
        .multi_line(flags.contains('m'))
        .ignore_whitespace(flags.contains('x'));
    builder
        .build()
        .map_err(|e| super::value::XPathError(format!("FORX0002: {e}")))
}

/// An XPath replacement string (`$N`, `\$`, `\\`) in the regex crate's syntax.
fn replacement_template(replacement: &str) -> XResult<String> {
    let mut out = String::new();
    let mut chars = replacement.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\\' => match chars.next() {
                Some('$') => out.push_str("$$"),
                Some('\\') => out.push('\\'),
                _ => return err("FORX0004: bad escape in a replacement"),
            },
            '$' => {
                let mut digits = String::new();
                while let Some(d) = chars.peek().filter(|d| d.is_ascii_digit()) {
                    digits.push(*d);
                    chars.next();
                }
                if digits.is_empty() {
                    return err("FORX0004: `$` without a group number");
                }
                out.push_str(&format!("${{{digits}}}"));
            }
            c => out.push(c),
        }
    }
    Ok(out)
}

/// Percent-encodes `s` like `fn:encode-for-uri` (or, with `lenient`, `fn:iri-to-uri`).
fn encode_uri(s: &str, lenient: bool) -> String {
    let mut out = String::new();
    for byte in s.bytes() {
        let keep = byte.is_ascii_alphanumeric()
            || matches!(byte, b'-' | b'_' | b'.' | b'~')
            || (lenient
                && byte.is_ascii_graphic()
                && !matches!(
                    byte,
                    b'<' | b'>' | b'"' | b'{' | b'}' | b'|' | b'\\' | b'^' | b'`'
                ));
        if keep {
            out.push(char::from(byte));
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

/// Decodes `%XX` escapes and `+` as a space; a `%` without two hex digits is an error
/// (`urlHexDecode`, internettools data/xquery.internals.common.pas:1653-1676).
fn decode_uri(s: &str) -> XResult<String> {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while let Some(&b) = bytes.get(i) {
        match b {
            b'%' => {
                let hex = bytes
                    .get(i + 1..i + 3)
                    .and_then(|h| std::str::from_utf8(h).ok());
                match hex.and_then(|h| u8::from_str_radix(h, 16).ok()) {
                    Some(decoded) => out.push(decoded),
                    None => return err("pxp:uri: invalid escape"),
                }
                i += 2;
            }
            b'+' => out.push(b' '),
            b => out.push(b),
        }
        i += 1;
    }
    Ok(String::from_utf8_lossy(&out).into_owned())
}

/// Resolves `relative` against `base`; without a base, `relative` stays as it is.
fn resolve_uri(base: &str, relative: &str) -> String {
    let has_scheme = |s: &str| {
        s.split_once(':').is_some_and(|(scheme, _)| {
            !scheme.is_empty()
                && scheme
                    .chars()
                    .all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.'))
        })
    };
    if base.is_empty() || has_scheme(relative) {
        return relative.to_owned();
    }
    let Some((scheme, rest)) = base.split_once("://") else {
        return relative.to_owned();
    };
    let (host, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    if let Some(r) = relative.strip_prefix("//") {
        return format!("{scheme}://{r}");
    }
    if relative.starts_with('/') {
        return format!("{scheme}://{host}{relative}");
    }
    let path = path.split(['?', '#']).next().unwrap_or("/");
    if relative.starts_with('?') || relative.starts_with('#') || relative.is_empty() {
        return format!("{scheme}://{host}{path}{relative}");
    }
    let directory = &path[..path.rfind('/').map_or(0, |i| i + 1)];
    let mut segments: Vec<&str> = Vec::new();
    let joined = format!("{directory}{relative}");
    for segment in joined.split('/').skip(1) {
        match segment {
            "." => {}
            ".." => {
                segments.pop();
            }
            s => segments.push(s),
        }
    }
    format!("{scheme}://{host}/{}", segments.join("/"))
}

fn node_name(which: &str, node: Option<&NodeRef>) -> XResult<Seq> {
    let Some(node) = node else {
        return string("");
    };
    let name = node.dom.name(node.id);
    string(match which {
        "local-name" => name.rsplit(':').next().unwrap_or(name),
        _ => name,
    })
}

/// `fn:root` of the first node.
fn root(a: &[Item]) -> XResult<Seq> {
    Ok(a.iter()
        .find_map(Item::as_node)
        .map(|node| Item::Node(node.with_id(0)))
        .into_iter()
        .collect())
}

/// `deep-text`: the first node's text nodes joined with `separator`, trimmed.
fn deep_text(a: &[Item], separator: &str) -> String {
    let text = match a.iter().find_map(Item::as_node) {
        Some(node) => match node.kind() {
            NodeKind::Element(_) | NodeKind::Document => {
                node.dom.text_nodes(node.id).join(separator)
            }
            _ => node.dom.deep_text(node.id),
        },
        None => String::new(),
    };
    trim(&text).to_owned()
}

/// A serialization of the first node; empty for anything else.
fn node_text(a: &[Item], f: fn(&NodeRef) -> String) -> String {
    a.iter().find_map(Item::as_node).map(f).unwrap_or_default()
}
