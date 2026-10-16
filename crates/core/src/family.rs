//! Families: BIMCraft's own parametric components (never Autodesk's `.rfa`). A family has
//! parameters, types that set its type parameters, and forms, each a profile swept across a
//! sketch plane and sized by the parameters; a component places one of its types. Formulas read
//! as Revit's do: `Width / 2`, `if(Depth > 600, 4, 2)`.

use std::collections::{BTreeMap, BTreeSet, HashMap};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::geom::P2;
use crate::model::{MAX_COORD, MAX_TEXT};
use crate::view3d::P3;

/// What a family is: how IFC names its components.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Category {
    #[default]
    Furniture,
    Casework,
    Plumbing,
    Lighting,
    Generic,
}

/// What a parameter holds.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    #[default]
    Length,
    Number,
    Angle,
    YesNo,
    Material,
}

/// A parameter: a type's (each type its own value) or, `instance`, each component's.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Parameter {
    pub name: String,
    #[serde(default)]
    pub kind: Kind,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub instance: bool,
    /// Where no type or component sets it: a number (yes/no, true or false), or a material's name.
    #[serde(default)]
    pub value: Value,
    /// Computed from the others, as Revit's formulas are.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub formula: Option<String>,
}

/// A number, or a formula over the family's parameters.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum Expr {
    Number(f64),
    Formula(String),
}

impl Default for Expr {
    fn default() -> Expr {
        Expr::Number(0.0)
    }
}

/// Where a form's profile is drawn: across the family's x and y, swept up; or upright, across x
/// and z, swept along y; or across y and z, swept along x.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Plane {
    #[default]
    Xy,
    Xz,
    Yz,
}

/// A round profile.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Circle {
    pub centre: [Expr; 2],
    pub radius: Expr,
}

/// A solid of a family, in its own axes (x along the component's angle, y to its left, z up from
/// its base): a profile drawn in `plane` and swept across it from `from` to `to`; or triangles, as
/// a file brings them. A material's name, or a material parameter's, says what it's made of;
/// `visible` (a yes/no parameter, or a formula) whether it's there.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Form {
    #[serde(default)]
    pub plane: Plane,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub profile: Vec<[Expr; 2]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub circle: Option<Circle>,
    #[serde(default)]
    pub from: Expr,
    #[serde(default)]
    pub to: Expr,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub material: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub visible: Option<Expr>,
    /// Shared, so an undo step's copy of the model doesn't copy it.
    #[serde(default, skip_serializing_if = "no_mesh")]
    pub mesh: Arc<Vec<[P3; 3]>>,
}

fn no_mesh(mesh: &Arc<Vec<[P3; 3]>>) -> bool {
    mesh.is_empty()
}

/// A family: its parameters, its types (each its type parameters' values where they aren't the
/// parameters' own) and its forms.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Family {
    #[serde(default)]
    pub category: Category,
    #[serde(default)]
    pub parameters: Vec<Parameter>,
    #[serde(default)]
    pub types: BTreeMap<String, BTreeMap<String, Value>>,
    #[serde(default)]
    pub forms: Vec<Form>,
}

/// A parameter's value, worked out: a number (yes/no, 1 or 0), or a material's name.
#[derive(Clone, Debug, PartialEq)]
pub enum Val {
    Number(f64),
    Text(String),
}

/// A form's solid in the family's own axes: its triangles, its caps' first; its outline seen from
/// above, from its bottom to its top, what it's made of; a round one's sides read as one surface.
#[derive(Clone, Debug, PartialEq)]
pub struct Piece {
    pub tris: Vec<[P3; 3]>,
    pub caps: usize,
    pub outline: Vec<P2>,
    pub bottom: f64,
    pub top: f64,
    pub material: String,
    pub round: bool,
}

/// How deep a formula may nest.
const MAX_DEPTH: usize = 64;
/// Sides a circle's profile is drawn with.
const SEGMENTS: usize = 32;
/// Corners one profile may have.
pub const MAX_PROFILE: usize = 64;
/// Formulas one may take from in a row: `A` from `B` from `C`…
pub const MAX_CHAIN: usize = 16;
/// Triangles a family's types may build together.
pub const MAX_BUILT: usize = 400_000;
/// Triangles one form, as a file brings it, may have.
pub const MAX_TRIANGLES: usize = 100_000;

/// Parameters, forms and types one family may have.
pub const MAX_PARAMETERS: usize = 64;
pub const MAX_FORMS: usize = 256;
pub const MAX_TYPES: usize = 1000;
/// The longest formula, in characters.
pub const MAX_FORMULA: usize = 1024;

/// A formula, read.
enum Node {
    Number(f64),
    Name(String),
    Minus(Box<Node>),
    /// A sum's or a product's first term, then each operator and term after it: flat, so a long one
    /// doesn't nest.
    Chain(Box<Node>, Vec<(char, Node)>),
    Op(char, Box<Node>, Box<Node>),
    Call(String, Vec<Node>),
}

/// Reads a formula: comparisons over sums over products over powers, which are right-associative.
struct Reader {
    chars: Vec<char>,
    at: usize,
    depth: usize,
}

impl Reader {
    fn peek(&mut self) -> Option<char> {
        while self.chars.get(self.at).is_some_and(|c| c.is_whitespace()) {
            self.at += 1;
        }
        self.chars.get(self.at).copied()
    }

    fn eat(&mut self, c: char) -> bool {
        let found = self.peek() == Some(c);
        self.at += usize::from(found);
        found
    }

    /// One level deeper; refused past `MAX_DEPTH`, so a hostile formula can't exhaust the stack.
    fn deeper(&mut self) -> Result<(), String> {
        self.depth += 1;
        if self.depth > MAX_DEPTH { Err(format!("it's nested too deep (more than {MAX_DEPTH} levels)")) } else { Ok(()) }
    }

    fn compare(&mut self) -> Result<Node, String> {
        self.deeper()?;
        let a = self.sum()?;
        let op = match self.peek() {
            Some('<') => {
                self.at += 1;
                if self.eat('=') { 'l' } else { '<' }
            }
            Some('>') => {
                self.at += 1;
                if self.eat('=') { 'g' } else { '>' }
            }
            Some('=') => {
                self.at += 1;
                '='
            }
            _ => {
                self.depth -= 1;
                return Ok(a);
            }
        };
        let b = self.sum()?;
        self.depth -= 1;
        Ok(Node::Op(op, Box::new(a), Box::new(b)))
    }

    fn sum(&mut self) -> Result<Node, String> {
        let first = self.product()?;
        let mut rest = Vec::new();
        while let Some(op @ ('+' | '-')) = self.peek() {
            self.at += 1;
            rest.push((op, self.product()?));
        }
        Ok(if rest.is_empty() { first } else { Node::Chain(Box::new(first), rest) })
    }

    fn product(&mut self) -> Result<Node, String> {
        let first = self.power()?;
        let mut rest = Vec::new();
        while let Some(op @ ('*' | '/')) = self.peek() {
            self.at += 1;
            rest.push((op, self.power()?));
        }
        Ok(if rest.is_empty() { first } else { Node::Chain(Box::new(first), rest) })
    }

    fn power(&mut self) -> Result<Node, String> {
        let a = self.unary()?;
        if !self.eat('^') {
            return Ok(a);
        }
        self.deeper()?;
        let b = self.power()?;
        self.depth -= 1;
        Ok(Node::Op('^', Box::new(a), Box::new(b)))
    }

    fn unary(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some(sign @ ('-' | '+')) => {
                self.at += 1;
                self.deeper()?;
                let a = self.unary()?;
                self.depth -= 1;
                Ok(if sign == '-' { Node::Minus(Box::new(a)) } else { a })
            }
            _ => self.atom(),
        }
    }

    fn atom(&mut self) -> Result<Node, String> {
        match self.peek() {
            Some('(') => {
                self.at += 1;
                let a = self.compare()?;
                if self.eat(')') { Ok(a) } else { Err("a `(` isn't closed".into()) }
            }
            Some(c) if c.is_ascii_digit() || c == '.' => {
                let start = self.at;
                while self.chars.get(self.at).is_some_and(|c| c.is_ascii_digit() || *c == '.') {
                    self.at += 1;
                }
                // An exponent: 1e6, 2.5E-3.
                if self.chars.get(self.at).is_some_and(|c| matches!(c, 'e' | 'E')) {
                    let sign = usize::from(self.chars.get(self.at + 1).is_some_and(|c| matches!(c, '+' | '-')));
                    if self.chars.get(self.at + 1 + sign).is_some_and(char::is_ascii_digit) {
                        self.at += 1 + sign;
                        while self.chars.get(self.at).is_some_and(char::is_ascii_digit) {
                            self.at += 1;
                        }
                    }
                }
                let text: String = self.chars.get(start..self.at).unwrap_or_default().iter().collect();
                text.parse().map(Node::Number).map_err(|_| format!("`{text}` isn't a number"))
            }
            // A name may hold spaces, as Revit's do: `Leg Height`.
            Some(c) if c.is_alphabetic() || c == '_' => {
                let start = self.at;
                while self.chars.get(self.at).is_some_and(|c| c.is_alphanumeric() || *c == '_' || *c == ' ') {
                    self.at += 1;
                }
                let name: String = self.chars.get(start..self.at).unwrap_or_default().iter().collect::<String>().trim_end().to_string();
                if !self.eat('(') {
                    return Ok(Node::Name(name));
                }
                let mut args = Vec::new();
                if !self.eat(')') {
                    loop {
                        args.push(self.compare()?);
                        if self.eat(')') {
                            break;
                        }
                        if !self.eat(',') {
                            return Err(format!("`{name}(` needs a `,` or a `)`"));
                        }
                    }
                }
                Ok(Node::Call(name, args))
            }
            Some(c) => Err(format!("`{c}` can't start a value there")),
            None => Err("something's missing at its end".into()),
        }
    }
}

fn truth(b: bool) -> f64 {
    if b { 1.0 } else { 0.0 }
}

/// Node `n`'s value: its names' from `get`; an `if` works out only the branch it takes.
fn value(n: &Node, get: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    Ok(match n {
        Node::Number(x) => *x,
        Node::Name(name) => get(name)?,
        Node::Minus(a) => -value(a, get)?,
        Node::Chain(first, rest) => {
            let mut x = value(first, get)?;
            for (op, n) in rest {
                let y = value(n, get)?;
                x = match op {
                    '+' => x + y,
                    '-' => x - y,
                    '*' => x * y,
                    _ => x / y,
                };
            }
            x
        }
        Node::Op(op, a, b) => {
            let (x, y) = (value(a, get)?, value(b, get)?);
            match op {
                '+' => x + y,
                '-' => x - y,
                '*' => x * y,
                '/' => x / y,
                '^' => x.powf(y),
                '<' => truth(x < y),
                '>' => truth(x > y),
                'l' => truth(x <= y),
                'g' => truth(x >= y),
                // Equal to within float noise, as Revit compares.
                _ => truth((x - y).abs() <= 1e-9 * x.abs().max(y.abs()).max(1.0)),
            }
        }
        Node::Call(f, args) => {
            let one = |get: &mut dyn FnMut(&str) -> Result<f64, String>| match args.as_slice() {
                [a] => value(a, get),
                _ => Err(format!("`{f}` takes one value, not {}", args.len())),
            };
            let deg = |x: f64| x.to_radians();
            match f.to_lowercase().as_str() {
                "if" => match args.as_slice() {
                    [c, a, b] => value(if value(c, get)? != 0.0 { a } else { b }, get)?,
                    _ => return Err(format!("`{f}` takes a condition and two values, not {} values", args.len())),
                },
                name @ ("and" | "or" | "min" | "max") => {
                    if args.is_empty() {
                        return Err(format!("`{f}` takes one value or more"));
                    }
                    let xs = args.iter().map(|a| value(a, get)).collect::<Result<Vec<f64>, String>>()?;
                    match name {
                        "and" => truth(xs.iter().all(|x| *x != 0.0)),
                        "or" => truth(xs.iter().any(|x| *x != 0.0)),
                        "min" => xs.iter().copied().fold(f64::INFINITY, f64::min),
                        _ => xs.iter().copied().fold(f64::NEG_INFINITY, f64::max),
                    }
                }
                "not" => truth(one(get)? == 0.0),
                "abs" => one(get)?.abs(),
                "sqrt" => one(get)?.sqrt(),
                "round" => one(get)?.round(),
                "roundup" => one(get)?.ceil(),
                "rounddown" => one(get)?.floor(),
                "sin" => deg(one(get)?).sin(),
                "cos" => deg(one(get)?).cos(),
                "tan" => deg(one(get)?).tan(),
                "asin" => one(get)?.asin().to_degrees(),
                "acos" => one(get)?.acos().to_degrees(),
                "atan" => one(get)?.atan().to_degrees(),
                "exp" => one(get)?.exp(),
                "ln" => one(get)?.ln(),
                "log" => one(get)?.log10(),
                "pi" if args.is_empty() => std::f64::consts::PI,
                _ => return Err(format!("`{f}` isn't a function: there are {FUNCTIONS}")),
            }
        }
    })
}

/// The functions a formula calls.
const FUNCTIONS: &str =
    "if, and, or, not, min, max, abs, sqrt, round, roundup, rounddown, sin, cos, tan, asin, acos, atan, exp, ln, log and pi()";

/// The names `formula` reads, in every branch of its `if`s; or why it can't be worked out: it
/// doesn't read, or calls a function there isn't or with the wrong number of values.
pub fn names(formula: &str) -> Result<Vec<String>, String> {
    fn walk(n: &Node, out: &mut Vec<String>) -> Result<(), String> {
        match n {
            Node::Number(_) => {}
            Node::Name(name) if !out.contains(name) => out.push(name.clone()),
            Node::Name(_) => {}
            Node::Minus(a) => walk(a, out)?,
            Node::Chain(first, rest) => {
                walk(first, out)?;
                for (_, n) in rest {
                    walk(n, out)?;
                }
            }
            Node::Op(_, a, b) => {
                walk(a, out)?;
                walk(b, out)?;
            }
            Node::Call(f, args) => {
                let fits = match f.to_lowercase().as_str() {
                    "if" => args.len() == 3,
                    "and" | "or" | "min" | "max" => !args.is_empty(),
                    "pi" => args.is_empty(),
                    "not" | "abs" | "sqrt" | "round" | "roundup" | "rounddown" | "sin" | "cos" | "tan" | "asin" | "acos" | "atan"
                    | "exp" | "ln" | "log" => args.len() == 1,
                    _ => return Err(format!("`{f}` isn't a function: there are {FUNCTIONS}")),
                };
                if !fits {
                    return Err(format!("`{f}` doesn't take {} values", args.len()));
                }
                for a in args {
                    walk(a, out)?;
                }
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(&parse(formula)?, &mut out).map_err(|e| format!("formula `{formula}`: {e}"))?;
    Ok(out)
}

/// `formula` worked out, its parameters' values from `get`: numbers, lengths in mm and angles in
/// degrees; a comparison is 1 when it holds, else 0.
pub fn eval(formula: &str, get: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    worked(formula, &parse(formula)?, get)
}

/// `formula`, read.
fn parse(formula: &str) -> Result<Node, String> {
    if formula.trim().is_empty() {
        return Err("a formula is empty".into());
    }
    let mut r = Reader { chars: formula.chars().collect(), at: 0, depth: 0 };
    let node = r.compare().map_err(|e| format!("formula `{formula}`: {e}"))?;
    match r.peek() {
        Some(c) => Err(format!("formula `{formula}`: `{c}` follows where it should end")),
        None => Ok(node),
    }
}

/// `formula`, read as `node`, worked out: a number, or why it isn't one.
fn worked(formula: &str, node: &Node, get: &mut dyn FnMut(&str) -> Result<f64, String>) -> Result<f64, String> {
    let said = |e: String| format!("formula `{formula}`: {e}");
    let x = value(node, get).map_err(said)?;
    if x.is_finite() { Ok(x) } else { Err(said(format!("it comes to {x}, which isn't a number"))) }
}

/// Whether `name` is one a formula can say: a letter or `_` first, then letters, digits, `_` and
/// spaces, none at its end; up to `MAX_TEXT` long.
pub fn nameable(name: &str) -> bool {
    name.chars().next().is_some_and(|c| c.is_alphabetic() || c == '_')
        && name.chars().all(|c| c.is_alphanumeric() || c == '_' || c == ' ')
        && !name.ends_with(' ')
        && name.chars().count() <= MAX_TEXT
}

/// A stored value read as parameter `p` holds it.
pub(crate) fn read(p: &Parameter, v: &Value) -> Result<Val, String> {
    let bad = |what: &str| format!("parameter `{}` holds {what}, not {v}", p.name);
    match p.kind {
        Kind::Material => match v {
            Value::String(s) => Ok(Val::Text(s.clone())),
            Value::Null => Ok(Val::Text(String::new())),
            _ => Err(bad("a material's name")),
        },
        Kind::YesNo => match v {
            Value::Bool(b) => Ok(Val::Number(truth(*b))),
            Value::Null => Ok(Val::Number(0.0)),
            v => v.as_f64().filter(|x| x.is_finite()).map(|x| Val::Number(truth(x != 0.0))).ok_or_else(|| bad("yes or no")),
        },
        Kind::Length | Kind::Number | Kind::Angle => match v {
            Value::Null => Ok(Val::Number(0.0)),
            v => v.as_f64().filter(|x| x.is_finite()).map(Val::Number).ok_or_else(|| bad("a number")),
        },
    }
}

impl Family {
    /// Whether `name` is one of its material parameters.
    pub fn material_parameter(&self, name: &str) -> bool {
        self.parameters.iter().any(|p| p.name == name && p.kind == Kind::Material)
    }

    /// The materials it names: its material parameters' values, its types', and its forms' own.
    pub fn materials(&self) -> Vec<&str> {
        let mut out = Vec::new();
        for p in self.parameters.iter().filter(|p| p.kind == Kind::Material) {
            out.extend(p.value.as_str());
            out.extend(self.types.values().filter_map(|t| t.get(&p.name)).filter_map(Value::as_str));
        }
        out.extend(self.forms.iter().filter(|f| !self.material_parameter(&f.material)).map(|f| f.material.as_str()));
        out.retain(|m| !m.is_empty());
        out
    }

    /// Material `from`, renamed `to` wherever it names it.
    pub fn rename_material(&mut self, from: &str, to: &str) {
        let params: Vec<String> = self.parameters.iter().filter(|p| p.kind == Kind::Material).map(|p| p.name.clone()).collect();
        let swap = |v: &mut Value| {
            if v.as_str() == Some(from) {
                *v = Value::String(to.into());
            }
        };
        self.parameters.iter_mut().filter(|p| p.kind == Kind::Material).for_each(|p| swap(&mut p.value));
        self.types.values_mut().flat_map(|t| t.iter_mut()).filter(|(k, _)| params.contains(k)).for_each(|(_, v)| swap(v));
        self.forms.iter_mut().filter(|f| f.material == from && !params.contains(&f.material)).for_each(|f| f.material = to.into());
    }

    /// Its parameters' values for type `type_name`, a component's `instance` values over them, and
    /// its formulas worked out.
    pub fn values(&self, type_name: &str, instance: &BTreeMap<String, Value>) -> Result<BTreeMap<String, Val>, String> {
        let set = self.types.get(type_name).ok_or_else(|| {
            let names: Vec<&str> = self.types.keys().map(String::as_str).collect();
            format!("type `{type_name}` doesn't exist (there are: {})", names.join(", "))
        })?;
        let mut done = BTreeMap::new();
        for p in &self.parameters {
            self.resolve(&p.name, set, instance, &mut done, &mut Vec::new())?;
        }
        Ok(done)
    }

    /// Parameter `name`'s value, and those it takes from, into `done`; `path`, the formulas being
    /// worked out, catches one that comes back to itself.
    fn resolve(
        &self,
        name: &str,
        set: &BTreeMap<String, Value>,
        instance: &BTreeMap<String, Value>,
        done: &mut BTreeMap<String, Val>,
        path: &mut Vec<String>,
    ) -> Result<Val, String> {
        if let Some(v) = done.get(name) {
            return Ok(v.clone());
        }
        let p = self.parameters.iter().find(|p| p.name == name).ok_or_else(|| format!("`{name}` isn't one of its parameters"))?;
        if path.iter().any(|n| n == name) {
            return Err(format!("`{name}`'s formula comes back to itself"));
        }
        let v = match &p.formula {
            Some(_) if path.len() >= MAX_CHAIN => {
                return Err(format!("`{name}` takes from formulas taking from more than {MAX_CHAIN} others in a row: nested too deep"));
            }
            Some(f) => {
                path.push(name.to_string());
                let x = eval(f, &mut |n: &str| match self.resolve(n, set, instance, done, path)? {
                    Val::Number(x) => Ok(x),
                    Val::Text(_) => Err(format!("`{n}` is a material, not a number")),
                });
                path.pop();
                Val::Number(x.map_err(|e| format!("parameter `{name}`: {e}"))?)
            }
            None => read(p, if p.instance { instance.get(name) } else { set.get(name) }.unwrap_or(&p.value))?,
        };
        done.insert(name.to_string(), v.clone());
        Ok(v)
    }

    /// Whether it's sound: its parameters named once each, so formulas can say them, and holding
    /// what their kinds hold; its types setting only the type parameters no formula works out; its
    /// materials in the library (`has_material`); and every type building.
    pub fn check(&self, has_material: &dyn Fn(&str) -> bool) -> Result<(), String> {
        for (what, n, most) in [
            ("parameters", self.parameters.len(), MAX_PARAMETERS),
            ("forms", self.forms.len(), MAX_FORMS),
            ("types", self.types.len(), MAX_TYPES),
        ] {
            if n > most {
                return Err(format!("it has more than {most} {what}"));
            }
        }
        if self.types.is_empty() {
            return Err("it needs a type: add one to `types`".into());
        }
        let material_ok = |who: &str, m: &str| {
            if m.is_empty() || has_material(m) {
                Ok(())
            } else {
                Err(format!("{who}: material `{m}` isn't in the library; add it with `material`"))
            }
        };
        let long = |f: &str| f.chars().count() > MAX_FORMULA;
        let mut names = BTreeSet::new();
        for p in &self.parameters {
            let who = format!("parameter `{}`", p.name);
            if !nameable(&p.name) {
                return Err(format!("{who}: a name starts with a letter and holds letters, digits, `_` and spaces, up to {MAX_TEXT}"));
            }
            if !names.insert(p.name.as_str()) {
                return Err(format!("{who} is named twice"));
            }
            match (&p.formula, p.kind) {
                (Some(_), Kind::Material) => return Err(format!("{who}: a material takes no formula")),
                (Some(f), _) if long(f) => return Err(format!("{who}: its formula is longer than {MAX_FORMULA} characters")),
                _ => {}
            }
            if let Val::Text(m) = read(p, &p.value)? {
                material_ok(&who, &m)?;
            }
        }
        for (t, set) in &self.types {
            let who = format!("type `{t}`");
            if t.trim().is_empty() || t.chars().count() > MAX_TEXT {
                return Err(format!("{who}: a type needs a name, up to {MAX_TEXT} characters"));
            }
            for (k, v) in set {
                let p = self.parameters.iter().find(|p| p.name == *k).ok_or_else(|| format!("{who}: `{k}` isn't one of its parameters"))?;
                if p.instance {
                    return Err(format!("{who}: `{k}` is an instance parameter; each component sets its own"));
                }
                if p.formula.is_some() {
                    return Err(format!("{who}: `{k}` is worked out by its formula"));
                }
                if let Val::Text(m) = read(p, v)? {
                    material_ok(&who, &m)?;
                }
            }
        }
        for (i, form) in self.forms.iter().enumerate() {
            let who = format!("form {}", i + 1);
            let ways = usize::from(!form.profile.is_empty()) + usize::from(form.circle.is_some()) + usize::from(!form.mesh.is_empty());
            if ways != 1 {
                return Err(format!("{who}: it takes a `profile`, a `circle` or a `mesh`, one of them"));
            }
            if !self.parameters.iter().any(|p| p.name == form.material && p.kind == Kind::Material) {
                material_ok(&who, &form.material)?;
            }
            let circle = form.circle.iter().flat_map(|c| c.centre.iter().chain([&c.radius]));
            let mut exprs = form.profile.iter().flatten().chain([&form.from, &form.to]).chain(&form.visible).chain(circle);
            if exprs.any(|e| matches!(e, Expr::Formula(f) if long(f))) {
                return Err(format!("{who}: a formula of it is longer than {MAX_FORMULA} characters"));
            }
        }
        // Each formula read once across the types; past what a family may build, refused.
        let (mut read, mut built) = (HashMap::new(), 0);
        for t in self.types.keys() {
            let pieces =
                self.values(t, &BTreeMap::new()).and_then(|v| self.build(&v, &mut read)).map_err(|e| format!("type `{t}`: {e}"))?;
            built += pieces.iter().map(|p| p.tris.len()).sum::<usize>();
            if built > MAX_BUILT {
                return Err(format!("its types build more than {MAX_BUILT} triangles together"));
            }
        }
        Ok(())
    }

    /// Its visible forms' solids, sized by `values`.
    pub fn pieces(&self, values: &BTreeMap<String, Val>) -> Result<Vec<Piece>, String> {
        self.build(values, &mut HashMap::new())
    }

    /// Its visible forms' solids, sized by `values`; `read`, its formulas as read already.
    fn build<'a>(
        &'a self,
        values: &BTreeMap<String, Val>,
        read: &mut HashMap<&'a str, Result<Node, String>>,
    ) -> Result<Vec<Piece>, String> {
        let mut num = |e: &'a Expr| -> Result<f64, String> {
            let x = match e {
                Expr::Number(x) => *x,
                Expr::Formula(f) => match read.entry(f.as_str()).or_insert_with(|| parse(f)) {
                    Ok(node) => worked(f, node, &mut |n: &str| match values.get(n) {
                        Some(Val::Number(x)) => Ok(*x),
                        Some(Val::Text(_)) => Err(format!("`{n}` is a material, not a number")),
                        None => Err(format!("`{n}` isn't one of its parameters")),
                    })?,
                    Err(e) => return Err(e.clone()),
                },
            };
            if x.is_finite() && x.abs() <= MAX_COORD { Ok(x) } else { Err(format!("{x} is not a usable number")) }
        };
        let mut out = Vec::new();
        for (i, form) in self.forms.iter().enumerate() {
            let what = format!("form {}", i + 1);
            let said = |e: String| format!("{what}: {e}");
            if let Some(v) = &form.visible
                && num(v).map_err(said)? == 0.0
            {
                continue;
            }
            let material = match values.get(&form.material) {
                Some(Val::Text(m)) => m.clone(),
                _ => form.material.clone(),
            };
            if !form.mesh.is_empty() {
                out.push(mesh(&form.mesh, material).map_err(said)?);
                continue;
            }
            let ring: Vec<P2> = match &form.circle {
                Some(c) => {
                    let (x, y, r) = (num(&c.centre[0]).map_err(said)?, num(&c.centre[1]).map_err(said)?, num(&c.radius).map_err(said)?);
                    if r <= 0.0 {
                        return Err(said(format!("its circle's radius is {r}; it needs one above zero")));
                    }
                    (0..SEGMENTS)
                        .map(|k| (k as f64 * std::f64::consts::TAU / SEGMENTS as f64).sin_cos())
                        .map(|(s, c)| P2(x + r * c, y + r * s))
                        .collect()
                }
                None if form.profile.len() > MAX_PROFILE => {
                    return Err(said(format!("its profile has more than {MAX_PROFILE} corners")));
                }
                None => form.profile.iter().map(|[a, b]| Ok(P2(num(a)?, num(b)?))).collect::<Result<_, String>>().map_err(said)?,
            };
            let (from, to) = (num(&form.from).map_err(said)?, num(&form.to).map_err(said)?);
            if ring.len() < 3 || crate::geom::area(&ring).abs() < 1.0 {
                return Err(said("its profile has no area: it needs three corners or more, or a circle".into()));
            }
            if to <= from {
                return Err(format!("{what} sweeps nothing: from {from} to {to}"));
            }
            let ring = if crate::geom::area(&ring) < 0.0 { ring.into_iter().rev().collect() } else { ring };
            let (tris, caps) = sweep(&ring, from, to, form.plane).map_err(said)?;
            let span = |k: usize| {
                ring.iter()
                    .map(|p| if k == 0 { p.0 } else { p.1 })
                    .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), x| (lo.min(x), hi.max(x)))
            };
            let ((a0, a1), (b0, b1)) = (span(0), span(1));
            let (outline, bottom, top) = match form.plane {
                Plane::Xy => (ring.clone(), from, to),
                Plane::Xz => (vec![P2(a0, from), P2(a1, from), P2(a1, to), P2(a0, to)], b0, b1),
                Plane::Yz => (vec![P2(from, a0), P2(to, a0), P2(to, a1), P2(from, a1)], b0, b1),
            };
            out.push(Piece { tris, caps, outline, bottom, top, material, round: form.circle.is_some() });
        }
        Ok(out)
    }
}

/// `ring`, counter-clockwise in `plane`, swept across it from `from` to `to`: its caps, then its
/// sides, every triangle turned out; and how many the caps take.
fn sweep(ring: &[P2], from: f64, to: f64, plane: Plane) -> Result<(Vec<[P3; 3]>, usize), String> {
    let at = |p: P2, t: f64| -> P3 {
        match plane {
            Plane::Xy => [p.0, p.1, t],
            Plane::Xz => [p.0, t, p.1],
            Plane::Yz => [t, p.0, p.1],
        }
    };
    let pt = |i: usize| ring.get(i).copied().unwrap_or_default();
    let caps = crate::geom::triangulate(ring);
    let covered: f64 = caps.iter().map(|[a, b, c]| (pt(*b) - pt(*a)).cross(pt(*c) - pt(*a)) / 2.0).sum();
    let area = crate::geom::area(ring);
    if (covered - area).abs() > 1e-6 * area + 1e-3 {
        return Err("its profile crosses itself".into());
    }
    let mut tris = Vec::with_capacity(caps.len() * 2 + ring.len() * 2);
    for &[a, b, c] in &caps {
        tris.push([at(pt(a), to), at(pt(b), to), at(pt(c), to)]);
        tris.push([at(pt(a), from), at(pt(c), from), at(pt(b), from)]);
    }
    for (a, b) in crate::geom::ring_edges(ring) {
        let (a0, b0, a1, b1) = (at(a, from), at(b, from), at(a, to), at(b, to));
        tris.push([a0, b0, b1]);
        tris.push([a0, b1, a1]);
    }
    // Across x and z the sweep runs along y: a mirror image of the others' axes, so its triangles
    // turn the other way to face out.
    if plane == Plane::Xz {
        tris.iter_mut().for_each(|t| t.swap(1, 2));
    }
    Ok((tris, caps.len() * 2))
}

/// A form a file brought as triangles: its outline seen from above, the hull round them.
fn mesh(tris: &[[P3; 3]], material: String) -> Result<Piece, String> {
    if tris.len() > MAX_TRIANGLES {
        return Err(format!("it has more than {MAX_TRIANGLES} triangles"));
    }
    if tris.iter().flatten().flatten().any(|x| !x.is_finite() || x.abs() > MAX_COORD) {
        return Err("a corner of it isn't a usable number".into());
    }
    let (bottom, top) = tris.iter().flatten().fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), p| (lo.min(p[2]), hi.max(p[2])));
    // ponytail: the convex hull stands for its outline; trace its silhouette if hulls mislead.
    let outline = crate::geom::hull(tris.iter().flatten().map(|p| P2(p[0], p[1])).collect());
    Ok(Piece { tris: tris.to_vec(), caps: tris.len(), outline, bottom, top, material, round: false })
}

/// The families a new project starts with, BIMCraft's own: tables, chairs, beds, sofas, desks,
/// wardrobes, base cabinets, WCs, washbasins and bathtubs, named in `language`, their formulas in
/// its words. A table or a chair stands on its centre, its back (a chair's) to its +y; the rest
/// stand with their backs on their x axis, their fronts toward -y, so they go against a wall.
pub fn library(language: crate::language::Language) -> BTreeMap<String, Family> {
    use serde_json::json;
    let w = |s: &str| language.word(s).to_string();
    let (width, depth, height, length) = (w("Width"), w("Depth"), w("Height"), w("Length"));
    let (thick, leg, seat, mat) = (w("Thickness"), w("Leg Width"), w("Seat Height"), w("Material"));
    let (timber, fabric, porcelain, concrete) = (w("Timber"), w("Fabric"), w("Porcelain"), w("Concrete"));
    // A box: x from `x0` to `x1`, y from `y0` to `y1`, up from `z0` to `z1`, of `m`.
    let block = |[x0, y0, x1, y1]: [Value; 4], z0: Value, z1: Value, m: &str| json!({"profile": [[x0, y0], [x1.clone(), y0], [x1, y1.clone()], [x0, y1]], "from": z0, "to": z1, "material": m});
    let f = |e: String| Value::String(e);
    let n = |x: f64| json!(x);
    let size = |name: &str, value: f64| json!({"name": name, "value": value});
    let material = |name: &str, value: &str| json!({"name": name, "kind": "material", "value": value});
    let (half_w, half_l) = (format!("{width} / 2"), format!("{length} / 2"));
    let neg = |e: &str| f(format!("-({e})"));
    let mut families = Vec::new();
    // Four legs at the corners of a top `w` by `d`, 50 in from its edges, up to `z`.
    let legs = |w: &str, d: &str, z: &str| -> Vec<Value> {
        [(true, true), (true, false), (false, true), (false, false)]
            .into_iter()
            .map(|(east, north)| {
                let x = if east {
                    [f(format!("({w}) / 2 - 50 - {leg}")), f(format!("({w}) / 2 - 50"))]
                } else {
                    [f(format!("-({w}) / 2 + 50")), f(format!("-({w}) / 2 + 50 + {leg}"))]
                };
                let y = if north {
                    [f(format!("({d}) / 2 - 50 - {leg}")), f(format!("({d}) / 2 - 50"))]
                } else {
                    [f(format!("-({d}) / 2 + 50")), f(format!("-({d}) / 2 + 50 + {leg}"))]
                };
                let [x0, x1] = x;
                let [y0, y1] = y;
                block([x0, y0, x1, y1], n(0.0), f(z.to_string()), &mat)
            })
            .collect()
    };
    let top = format!("{height} - {thick}");
    let mut forms = vec![block(
        [neg(&half_w), f(format!("-{depth} / 2")), f(half_w.clone()), f(format!("{depth} / 2"))],
        f(top.clone()),
        f(height.clone()),
        &mat,
    )];
    forms.extend(legs(&width, &depth, &top));
    families.push((
        w("Table"),
        json!({
            "parameters": [size(&width, 1600.0), size(&depth, 900.0), size(&height, 750.0), size(&thick, 40.0), size(&leg, 60.0), material(&mat, &timber)],
            "types": {"1600 x 900": {}, "1200 x 800": {&width: 1200, &depth: 800}, "2000 x 1000": {&width: 2000, &depth: 1000}},
            "forms": forms
        }),
    ));
    // A chair: its seat, its back over the seat's rear edge, and its legs.
    let under = format!("{seat} - {thick}");
    let mut forms = vec![
        block(
            [neg(&half_w), f(format!("-{depth} / 2")), f(half_w.clone()), f(format!("{depth} / 2"))],
            f(under.clone()),
            f(seat.clone()),
            &mat,
        ),
        json!({"plane": "xz", "profile": [[neg(&half_w), seat], [half_w, seat], [half_w, height], [neg(&half_w), height]],
               "from": format!("{depth} / 2 - {thick}"), "to": format!("{depth} / 2"), "material": mat}),
    ];
    forms.extend(legs(&format!("{width} + 100"), &format!("{depth} + 100"), &under));
    families.push((
        w("Chair"),
        json!({
            "parameters": [size(&width, 450.0), size(&depth, 450.0), size(&seat, 450.0), size(&height, 850.0), size(&thick, 40.0), size(&leg, 40.0), material(&mat, &timber)],
            "types": {"450 x 450": {}},
            "forms": forms
        }),
    ));
    // A bed against a wall: its base, its mattress, its headboard, and a pillow, two on a double.
    let head = w("Headboard Height");
    let pillow = |x0: String, x1: String, shown: &str| {
        let mut b = block([f(x0), n(-500.0), f(x1), n(-100.0)], f(height.clone()), f(format!("{height} + 120")), &fabric);
        b["visible"] = json!(shown);
        b
    };
    families.push((
        w("Bed"),
        json!({
            "parameters": [size(&width, 1600.0), size(&length, 2000.0), size(&height, 450.0), size(&head, 900.0), material(&mat, &timber)],
            "types": {"1600 x 2000": {}, "900 x 2000": {&width: 900}, "1800 x 2000": {&width: 1800}},
            "forms": [
                block([neg(&half_w), neg(&length), f(half_w.clone()), n(0.0)], n(0.0), n(300.0), &mat),
                block([f(format!("-({width}) / 2 + 20")), f(format!("-({length}) + 20")), f(format!("{width} / 2 - 20")), n(-60.0)], n(300.0), f(height.clone()), &fabric),
                {"plane": "xz", "profile": [[neg(&half_w), 0], [half_w, 0], [half_w, head], [neg(&half_w), head]], "from": -60, "to": 0, "material": mat},
                pillow(format!("-({width}) / 2 + 80"), "-40".into(), &format!("{width} > 1200")),
                pillow("40".into(), format!("{width} / 2 - 80"), &format!("{width} > 1200")),
                pillow(format!("-({width}) / 2 + 80"), format!("{width} / 2 - 80"), &format!("{width} <= 1200"))
            ]
        }),
    ));
    // A sofa: its seat between its arms, its back.
    let arm = w("Arm Width");
    families.push((
        w("Sofa"),
        json!({
            "parameters": [size(&width, 2000.0), size(&depth, 900.0), size(&seat, 420.0), size(&height, 800.0), size(&arm, 180.0), material(&mat, &fabric)],
            "types": {"2000 x 900": {}, "1600 x 900": {&width: 1600}},
            "forms": [
                block([f(format!("-({width}) / 2 + {arm}")), neg(&depth), f(format!("{width} / 2 - {arm}")), n(-200.0)], n(0.0), f(seat.clone()), &mat),
                block([neg(&half_w), n(-200.0), f(half_w.clone()), n(0.0)], n(0.0), f(height.clone()), &mat),
                block([neg(&half_w), neg(&depth), f(format!("-({width}) / 2 + {arm}")), n(-200.0)], n(0.0), n(600.0), &mat),
                block([f(format!("{width} / 2 - {arm}")), neg(&depth), f(half_w.clone()), n(-200.0)], n(0.0), n(600.0), &mat)
            ]
        }),
    ));
    // A desk: its top on two side panels, a modesty panel between them.
    families.push((
        w("Desk"),
        json!({
            "parameters": [size(&width, 1400.0), size(&depth, 700.0), size(&height, 740.0), size(&thick, 30.0), material(&mat, &timber)],
            "types": {"1400 x 700": {}, "1200 x 600": {&width: 1200, &depth: 600}},
            "forms": [
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], f(top.clone()), f(height.clone()), &mat),
                block([neg(&half_w), f(format!("-({depth}) + 20")), f(format!("-({width}) / 2 + 25")), n(-20.0)], n(0.0), f(top.clone()), &mat),
                block([f(format!("{width} / 2 - 25")), f(format!("-({depth}) + 20")), f(half_w.clone()), n(-20.0)], n(0.0), f(top.clone()), &mat),
                block([f(format!("-({width}) / 2 + 25")), n(-60.0), f(format!("{width} / 2 - 25")), n(-40.0)], n(300.0), f(top.clone()), &mat)
            ]
        }),
    ));
    // A wardrobe on its plinth.
    families.push((
        w("Wardrobe"),
        json!({
            "category": "casework",
            "parameters": [size(&width, 1200.0), size(&depth, 600.0), size(&height, 2100.0), material(&mat, &timber)],
            "types": {"1200 x 600": {}, "900 x 600": {&width: 900}},
            "forms": [
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], n(80.0), f(height.clone()), &mat),
                block([f(format!("-({width}) / 2 + 50")), f(format!("-({depth}) + 50")), f(format!("{width} / 2 - 50")), n(0.0)], n(0.0), n(80.0), &mat)
            ]
        }),
    ));
    // A base cabinet: its plinth, set back for toes; its carcass; its worktop over the front.
    let worktop = w("Worktop");
    families.push((
        w("Base Cabinet"),
        json!({
            "category": "casework",
            "parameters": [size(&width, 1800.0), size(&depth, 600.0), size(&height, 900.0), material(&mat, &timber), material(&worktop, &concrete)],
            "types": {"1800 x 600": {}, "1200 x 600": {&width: 1200}, "600 x 600": {&width: 600}},
            "forms": [
                block([neg(&half_w), f(format!("-({depth}) + 60")), f(half_w.clone()), n(0.0)], n(0.0), n(100.0), &mat),
                block([neg(&half_w), f(format!("-({depth}) + 20")), f(half_w.clone()), n(0.0)], n(100.0), f(format!("{height} - 40")), &mat),
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], f(format!("{height} - 40")), f(height.clone()), &worktop)
            ]
        }),
    ));
    // A WC: its cistern against the wall, its bowl.
    families.push((
        w("WC"),
        json!({
            "category": "plumbing",
            "types": {"400 x 650": {}},
            "forms": [
                block([n(-200.0), n(-180.0), n(200.0), n(0.0)], n(380.0), n(800.0), &porcelain),
                block([n(-160.0), n(-380.0), n(160.0), n(-150.0)], n(0.0), n(400.0), &porcelain),
                {"circle": {"centre": [0, -470], "radius": 180}, "from": 0, "to": 400, "material": porcelain}
            ]
        }),
    ));
    // A washbasin on its pedestal.
    families.push((
        w("Washbasin"),
        json!({
            "category": "plumbing",
            "parameters": [size(&width, 600.0), size(&depth, 450.0), size(&height, 850.0)],
            "types": {"600 x 450": {}, "500 x 400": {&width: 500, &depth: 400}},
            "forms": [
                block([neg(&half_w), neg(&depth), f(half_w.clone()), n(0.0)], f(format!("{height} - 160")), f(height.clone()), &porcelain),
                {"circle": {"centre": [0, -150], "radius": 90}, "from": 0, "to": format!("{height} - 160"), "material": porcelain}
            ]
        }),
    ));
    // A bathtub along a wall: its bottom, and its four sides round the tub.
    let rim = height.clone();
    let inner = format!("-({width}) + 60");
    families.push((
        w("Bathtub"),
        json!({
            "category": "plumbing",
            "parameters": [size(&length, 1700.0), size(&width, 750.0), size(&height, 560.0)],
            "types": {"1700 x 750": {}, "1500 x 700": {&length: 1500, &width: 700}},
            "forms": [
                block([neg(&half_l), neg(&width), f(half_l.clone()), n(0.0)], n(0.0), n(120.0), &porcelain),
                block([neg(&half_l), n(-60.0), f(half_l.clone()), n(0.0)], n(120.0), f(rim.clone()), &porcelain),
                block([neg(&half_l), neg(&width), f(half_l.clone()), f(inner.clone())], n(120.0), f(rim.clone()), &porcelain),
                block([neg(&half_l), f(inner.clone()), f(format!("-({length}) / 2 + 60")), n(-60.0)], n(120.0), f(rim.clone()), &porcelain),
                block([f(format!("{length} / 2 - 60")), f(inner), f(half_l), n(-60.0)], n(120.0), f(rim), &porcelain)
            ]
        }),
    ));
    families.into_iter().filter_map(|(name, v)| serde_json::from_value(v).ok().map(|f| (name, f))).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn with<'a>(values: &'a [(&'a str, f64)]) -> impl FnMut(&str) -> Result<f64, String> + 'a {
        move |n: &str| values.iter().find(|(k, _)| *k == n).map(|(_, v)| *v).ok_or_else(|| format!("no `{n}`"))
    }

    #[test]
    fn formulas_read_as_revits_do() {
        let values = [("Width", 1000.0), ("Leg Height", 700.0)];
        for (formula, want) in [
            ("Width / 2 + 10", 510.0),
            ("2 + 3 * 4", 14.0),
            ("(2 + 3) * 4", 20.0),
            ("-Width + 1", -999.0),
            ("2 ^ 3 ^ 2", 512.0),
            ("Leg Height - 2*3", 694.0),
            ("if(Width > 1000, 4, 2)", 2.0),
            ("if(Width >= 1000, 4, 2)", 4.0),
            ("if (Width = 1000, 1, 0)", 1.0),
            ("not(Width < 1000)", 1.0),
            ("and(1, 0) + or(1, 0)", 1.0),
            ("sqrt(16) + abs(-2) + min(3, 5) + max(3, 5)", 14.0),
            ("round(2.5) + roundup(2.1) + rounddown(2.9)", 8.0),
            ("sin(30) + cos(60)", 1.0),
            ("pi() * 0", 0.0),
            // The branch not taken isn't worked out.
            ("if(Width > 0, 1, 1 / 0)", 1.0),
        ] {
            let got = eval(formula, &mut with(&values)).unwrap();
            assert!((got - want).abs() < 1e-9, "{formula}: {got}, not {want}");
        }
        for (formula, says) in [
            ("Width +", "`Width +`"),
            ("Nope + 1", "Nope"),
            ("1 / 0", "isn't a number"),
            ("sqrt(-1)", "isn't a number"),
            ("if(1, 2)", "if"),
            ("frobnicate(1)", "frobnicate"),
            ("2 3", "`2 3`"),
            ("", "empty"),
        ] {
            let e = eval(formula, &mut with(&values)).unwrap_err();
            assert!(e.contains(says), "{formula}: {e}");
        }
        // Nested past reason: refused, not a stack overflow.
        let deep = format!("{}1{}", "(".repeat(10_000), ")".repeat(10_000));
        assert!(eval(&deep, &mut with(&values)).unwrap_err().contains("deep"));
    }

    fn table() -> Family {
        serde_json::from_value(json!({
            "parameters": [
                {"name": "Width", "value": 1000},
                {"name": "Depth", "value": 600},
                {"name": "Height", "instance": true, "value": 750},
                {"name": "Top", "formula": "Height - 30"},
                {"name": "Area", "kind": "number", "formula": "Width * Depth / 1e6"},
                {"name": "Legs", "kind": "yesno", "value": true},
                {"name": "Finish", "kind": "material", "value": "Timber"}
            ],
            "types": {"1000 x 600": {}, "Small": {"Width": 800}},
            "forms": [
                {"profile": [[0, 0], ["Width", 0], ["Width", "Depth"], [0, "Depth"]], "from": "Top", "to": "Height", "material": "Finish"},
                {"profile": [[0, 0], [50, 0], [50, 50], [0, 50]], "from": 0, "to": "Top", "visible": "Legs"}
            ]
        }))
        .unwrap()
    }

    #[test]
    fn types_and_components_set_parameters_and_formulas_follow() {
        let f = table();
        let v = f.values("Small", &BTreeMap::from([("Height".to_string(), json!(700))])).unwrap();
        let n = |k: &str| match v.get(k) {
            Some(Val::Number(x)) => *x,
            other => panic!("{k}: {other:?}"),
        };
        assert_eq!([n("Width"), n("Depth"), n("Height"), n("Top"), n("Area"), n("Legs")], [800.0, 600.0, 700.0, 670.0, 0.48, 1.0]);
        assert_eq!(v.get("Finish"), Some(&Val::Text("Timber".into())));
        let mut cyclic = f.clone();
        cyclic.parameters[0].formula = Some("Depth + Top".into());
        cyclic.parameters[1].formula = Some("Width - 1".into());
        let e = cyclic.values("Small", &BTreeMap::new()).unwrap_err();
        assert!(e.contains("Width") && e.contains("itself"), "{e}");
        assert!(f.values("Huge", &BTreeMap::new()).unwrap_err().contains("Huge"));
    }

    #[test]
    fn long_formulas_and_long_chains_of_them_never_exhaust_the_stack() {
        // A flat sum of many terms reads; parameters each taking the next, deeper than any family
        // needs, are refused, and neither runs a small stack dry.
        let worked = std::thread::Builder::new()
            .stack_size(256 * 1024)
            .spawn(|| {
                let flat = format!("1{}", " + 1".repeat(600));
                let sum = eval(&flat, &mut |_| Ok(0.0));
                let terms = format!("P{{}}{}", " + 1".repeat(500));
                let parameters: Vec<Value> = (0..64)
                    .map(|i| {
                        if i == 63 {
                            json!({"name": "P63", "value": 1})
                        } else {
                            json!({"name": format!("P{i}"), "formula": terms.replace("{}", &(i + 1).to_string())})
                        }
                    })
                    .collect();
                let f: Family = serde_json::from_value(json!({"parameters": parameters, "types": {"A": {}}})).unwrap();
                (sum, f.values("A", &BTreeMap::new()))
            })
            .unwrap()
            .join()
            .unwrap();
        assert_eq!(worked.0, Ok(601.0));
        assert!(worked.1.unwrap_err().contains("deep"));
    }

    #[test]
    fn a_family_too_big_to_build_is_refused() {
        let ring = |n: usize| -> Vec<Value> {
            (0..n).map(|k| (k as f64 * std::f64::consts::TAU / n as f64).sin_cos()).map(|(s, c)| json!([100.0 * c, 100.0 * s])).collect()
        };
        let f: Family =
            serde_json::from_value(json!({"types": {"A": {}}, "forms": [{"profile": ring(MAX_PROFILE + 1), "to": 10}]})).unwrap();
        assert!(f.check(&|_| true).unwrap_err().contains("corners"));
        // Many types, each building a little: past what one family may build, refused.
        let types: serde_json::Map<String, Value> = (0..MAX_TYPES).map(|k| (format!("T{k}"), json!({}))).collect();
        let form = json!({"profile": ring(MAX_PROFILE), "to": 10});
        let f: Family = serde_json::from_value(json!({"types": types, "forms": [form, form]})).unwrap();
        assert!(f.check(&|_| true).unwrap_err().contains("triangles"));
    }

    /// A closed surface's volume: the triangles' signed tetrahedra from the origin, positive while
    /// they face out.
    fn volume(tris: &[[P3; 3]]) -> f64 {
        tris.iter().map(|[a, b, c]| crate::view3d::dot(*a, crate::view3d::cross(*b, *c)) / 6.0).sum::<f64>()
    }

    #[test]
    fn forms_sweep_their_profiles_across_their_planes() {
        let f = table();
        let v = f.values("1000 x 600", &BTreeMap::new()).unwrap();
        let pieces = f.pieces(&v).unwrap();
        assert_eq!(pieces.len(), 2);
        let top = &pieces[0];
        assert_eq!((top.bottom, top.top, top.material.as_str(), top.round), (720.0, 750.0, "Timber", false));
        assert!((volume(&top.tris) - 1000.0 * 600.0 * 30.0).abs() < 1e-3, "{}", volume(&top.tris));
        assert_eq!(top.outline, [P2(0.0, 0.0), P2(1000.0, 0.0), P2(1000.0, 600.0), P2(0.0, 600.0)]);
        // Hidden: no leg.
        let mut no_legs = v.clone();
        no_legs.insert("Legs".into(), Val::Number(0.0));
        assert_eq!(f.pieces(&no_legs).unwrap().len(), 1);
        // Upright profiles: a back across x and z, swept along y; a rail across y and z, along x;
        // a round post, its sides one surface.
        let f: Family = serde_json::from_value(json!({
            "types": {"A": {}},
            "forms": [
                {"plane": "xz", "profile": [[0, 400], [500, 400], [500, 900], [0, 900]], "from": 0, "to": 40},
                {"plane": "yz", "profile": [[0, 0], [300, 0], [300, 100]], "from": 10, "to": 210},
                {"circle": {"centre": [100, 100], "radius": 20}, "from": 0, "to": 1000}
            ]
        }))
        .unwrap();
        let pieces = f.pieces(&f.values("A", &BTreeMap::new()).unwrap()).unwrap();
        let back = &pieces[0];
        assert!((volume(&back.tris) - 500.0 * 500.0 * 40.0).abs() < 1e-3);
        assert_eq!((back.bottom, back.top), (400.0, 900.0));
        assert_eq!(back.outline, [P2(0.0, 0.0), P2(500.0, 0.0), P2(500.0, 40.0), P2(0.0, 40.0)]);
        let rail = &pieces[1];
        assert!((volume(&rail.tris) - 300.0 * 100.0 / 2.0 * 200.0).abs() < 1e-3);
        assert_eq!(rail.outline, [P2(10.0, 0.0), P2(210.0, 0.0), P2(210.0, 300.0), P2(10.0, 300.0)]);
        let post = &pieces[2];
        let ring = crate::geom::area(&post.outline);
        assert!(
            post.round && (volume(&post.tris) - ring * 1000.0).abs() < 1e-3 && (ring / (std::f64::consts::PI * 400.0) - 1.0).abs() < 0.01
        );
        // Nothing to sweep, or a profile with no area: said.
        let mut flat = table();
        flat.forms[0].to = Expr::Formula("Top".into());
        assert!(flat.pieces(&flat.values("Small", &BTreeMap::new()).unwrap()).unwrap_err().contains("form 1"));
    }
}
