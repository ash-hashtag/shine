//! Expressions for function cells: `in1 + in2`, `p1 > 0.5`, `bit(in1, 3)`.
//!
//! Values are `f64`. Bitwise and logical operators work on the integer part
//! (`a & b`, `a | b`, `a ^ b`, `!a`, `a << n`, `a >> n`); comparisons and
//! logic return `1` or `0`. Precedence, loosest first:
//! `? :`, `||`, `&&`, `|`, `^`, `&`, `== !=`, `< <= > >=`, `<< >>`, `+ -`,
//! `* / %`, unary `- !`, `**` (right associative).
//!
//! Functions: `abs sqrt floor ceil round exp ln sin cos asin acos atan min
//! max clamp bit pow argmax sum`.

use std::collections::HashMap;
use std::fmt;

#[derive(Clone, Debug, PartialEq)]
pub enum Expr {
    Number(f64),
    Var(String),
    Unary(char, Box<Expr>),
    Binary(&'static str, Box<Expr>, Box<Expr>),
    Ternary(Box<Expr>, Box<Expr>, Box<Expr>),
    Call(String, Vec<Expr>),
}

#[derive(Clone, Debug, PartialEq)]
pub struct ParseError {
    pub message: String,
    pub position: usize,
}

impl fmt::Display for ParseError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{} (at character {})", self.message, self.position + 1)
    }
}

#[derive(Clone, Debug, PartialEq)]
enum Token {
    Number(f64),
    Ident(String),
    Op(&'static str),
    LParen,
    RParen,
    Comma,
    Question,
    Colon,
}

const OPERATORS: [&str; 22] = [
    "**", "<<", ">>", "<=", ">=", "==", "!=", "&&", "||", "+", "-", "*", "/", "%", "<", ">", "&", "|", "^", "!", "~", "=",
];

fn tokenize(src: &str) -> Result<Vec<(Token, usize)>, ParseError> {
    let bytes = src.as_bytes();
    let mut tokens = Vec::new();
    let mut i = 0;
    while i < bytes.len() {
        let c = bytes[i] as char;
        if c.is_whitespace() {
            i += 1;
            continue;
        }
        let start = i;
        if c.is_ascii_digit() || (c == '.' && bytes.get(i + 1).is_some_and(|b| b.is_ascii_digit())) {
            if c == '0' && matches!(bytes.get(i + 1), Some(b'b' | b'x')) {
                let radix = if bytes[i + 1] == b'b' { 2 } else { 16 };
                i += 2;
                while i < bytes.len() && (bytes[i] as char).is_ascii_alphanumeric() {
                    i += 1;
                }
                let value = u64::from_str_radix(&src[start + 2..i], radix).map_err(|_| ParseError {
                    message: format!("bad number “{}”", &src[start..i]),
                    position: start,
                })?;
                tokens.push((Token::Number(value as f64), start));
                continue;
            }
            while i < bytes.len() && ((bytes[i] as char).is_ascii_digit() || bytes[i] == b'.') {
                i += 1;
            }
            if i < bytes.len() && matches!(bytes[i], b'e' | b'E') {
                i += 1;
                if i < bytes.len() && matches!(bytes[i], b'+' | b'-') {
                    i += 1;
                }
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }
            }
            let value = src[start..i].parse::<f64>().map_err(|_| ParseError {
                message: format!("bad number “{}”", &src[start..i]),
                position: start,
            })?;
            tokens.push((Token::Number(value), start));
            continue;
        }
        if c.is_ascii_alphabetic() || c == '_' {
            while i < bytes.len() && ((bytes[i] as char).is_ascii_alphanumeric() || bytes[i] == b'_') {
                i += 1;
            }
            tokens.push((Token::Ident(src[start..i].to_owned()), start));
            continue;
        }
        let single = match c {
            '(' => Some(Token::LParen),
            ')' => Some(Token::RParen),
            ',' => Some(Token::Comma),
            '?' => Some(Token::Question),
            ':' => Some(Token::Colon),
            _ => None,
        };
        if let Some(t) = single {
            tokens.push((t, start));
            i += 1;
            continue;
        }
        match OPERATORS.iter().find(|op| src[i..].starts_with(**op)) {
            Some(&"=") => {
                return Err(ParseError {
                    message: "use “==” to compare".into(),
                    position: start,
                });
            }
            Some(op) => {
                tokens.push((Token::Op(op), start));
                i += op.len();
            }
            None => {
                return Err(ParseError {
                    message: format!("unexpected “{c}”"),
                    position: start,
                });
            }
        }
    }
    Ok(tokens)
}

struct Parser {
    tokens: Vec<(Token, usize)>,
    pos: usize,
    end: usize,
}

const BINARY_LEVELS: [&[&str]; 9] = [
    &["||"],
    &["&&"],
    &["|"],
    &["^"],
    &["&"],
    &["==", "!="],
    &["<", "<=", ">", ">="],
    &["<<", ">>", "+", "-"],
    &["*", "/", "%"],
];

impl Parser {
    fn peek(&self) -> Option<&Token> {
        self.tokens.get(self.pos).map(|t| &t.0)
    }

    fn position(&self) -> usize {
        self.tokens.get(self.pos).map_or(self.end, |t| t.1)
    }

    fn error<T>(&self, message: impl Into<String>) -> Result<T, ParseError> {
        Err(ParseError {
            message: message.into(),
            position: self.position(),
        })
    }

    fn expect(&mut self, token: Token, what: &str) -> Result<(), ParseError> {
        if self.peek() == Some(&token) {
            self.pos += 1;
            Ok(())
        } else {
            self.error(format!("expected {what}"))
        }
    }

    fn ternary(&mut self) -> Result<Expr, ParseError> {
        let condition = self.binary(0)?;
        if self.peek() == Some(&Token::Question) {
            self.pos += 1;
            let yes = self.ternary()?;
            self.expect(Token::Colon, "“:”")?;
            let no = self.ternary()?;
            return Ok(Expr::Ternary(Box::new(condition), Box::new(yes), Box::new(no)));
        }
        Ok(condition)
    }

    fn binary(&mut self, level: usize) -> Result<Expr, ParseError> {
        if level == BINARY_LEVELS.len() {
            return self.unary();
        }
        // Shifts bind tighter than + and - in C; keep that order here.
        let ops = BINARY_LEVELS[level];
        let mut left = self.binary(level + 1)?;
        while let Some(Token::Op(op)) = self.peek() {
            let op = *op;
            if !ops.contains(&op) {
                break;
            }
            self.pos += 1;
            let right = self.binary(level + 1)?;
            left = Expr::Binary(op, Box::new(left), Box::new(right));
        }
        Ok(left)
    }

    fn unary(&mut self) -> Result<Expr, ParseError> {
        match self.peek() {
            Some(Token::Op("-")) => {
                self.pos += 1;
                Ok(Expr::Unary('-', Box::new(self.unary()?)))
            }
            Some(Token::Op("!")) => {
                self.pos += 1;
                Ok(Expr::Unary('!', Box::new(self.unary()?)))
            }
            Some(Token::Op("~")) => {
                self.pos += 1;
                Ok(Expr::Unary('~', Box::new(self.unary()?)))
            }
            Some(Token::Op("+")) => {
                self.pos += 1;
                self.unary()
            }
            _ => self.power(),
        }
    }

    fn power(&mut self) -> Result<Expr, ParseError> {
        let base = self.atom()?;
        if self.peek() == Some(&Token::Op("**")) {
            self.pos += 1;
            let exponent = self.unary()?;
            return Ok(Expr::Binary("**", Box::new(base), Box::new(exponent)));
        }
        Ok(base)
    }

    fn atom(&mut self) -> Result<Expr, ParseError> {
        match self.peek().cloned() {
            Some(Token::Number(v)) => {
                self.pos += 1;
                Ok(Expr::Number(v))
            }
            Some(Token::Ident(name)) => {
                self.pos += 1;
                if self.peek() == Some(&Token::LParen) {
                    self.pos += 1;
                    let mut args = Vec::new();
                    if self.peek() != Some(&Token::RParen) {
                        loop {
                            args.push(self.ternary()?);
                            if self.peek() == Some(&Token::Comma) {
                                self.pos += 1;
                            } else {
                                break;
                            }
                        }
                    }
                    self.expect(Token::RParen, "“)”")?;
                    return Ok(Expr::Call(name, args));
                }
                match name.as_str() {
                    "true" => Ok(Expr::Number(1.0)),
                    "false" => Ok(Expr::Number(0.0)),
                    "pi" => Ok(Expr::Number(std::f64::consts::PI)),
                    _ => Ok(Expr::Var(name)),
                }
            }
            Some(Token::LParen) => {
                self.pos += 1;
                let inner = self.ternary()?;
                self.expect(Token::RParen, "“)”")?;
                Ok(inner)
            }
            Some(_) => self.error("expected a number, name or “(”"),
            None => self.error("unexpected end of expression"),
        }
    }
}

pub fn parse(src: &str) -> Result<Expr, ParseError> {
    let tokens = tokenize(src)?;
    let mut parser = Parser {
        tokens,
        pos: 0,
        end: src.len(),
    };
    if parser.peek().is_none() {
        return parser.error("empty expression");
    }
    let expr = parser.ternary()?;
    if parser.pos < parser.tokens.len() {
        return parser.error("unexpected text after the expression");
    }
    Ok(expr)
}

fn int(v: f64) -> i64 {
    if v.is_finite() { v.trunc() as i64 } else { 0 }
}

fn truth(v: bool) -> f64 {
    if v { 1.0 } else { 0.0 }
}

#[derive(Clone, Debug, PartialEq)]
pub struct EvalError(pub String);

impl fmt::Display for EvalError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl Expr {
    pub fn eval(&self, vars: &HashMap<String, f64>) -> Result<f64, EvalError> {
        Ok(match self {
            Expr::Number(v) => *v,
            Expr::Var(name) => *vars.get(name).ok_or_else(|| EvalError(format!("unknown name “{name}”")))?,
            Expr::Unary(op, e) => {
                let v = e.eval(vars)?;
                match op {
                    '-' => -v,
                    '!' => truth(v == 0.0),
                    _ => !int(v) as f64,
                }
            }
            Expr::Ternary(c, a, b) => {
                if c.eval(vars)? != 0.0 {
                    a.eval(vars)?
                } else {
                    b.eval(vars)?
                }
            }
            Expr::Binary(op, a, b) => {
                let (x, y) = (a.eval(vars)?, b.eval(vars)?);
                match *op {
                    "+" => x + y,
                    "-" => x - y,
                    "*" => x * y,
                    "/" => x / y,
                    "%" => x % y,
                    "**" => x.powf(y),
                    "<" => truth(x < y),
                    "<=" => truth(x <= y),
                    ">" => truth(x > y),
                    ">=" => truth(x >= y),
                    "==" => truth((x - y).abs() < 1e-9),
                    "!=" => truth((x - y).abs() >= 1e-9),
                    "&&" => truth(x != 0.0 && y != 0.0),
                    "||" => truth(x != 0.0 || y != 0.0),
                    "&" => (int(x) & int(y)) as f64,
                    "|" => (int(x) | int(y)) as f64,
                    "^" => (int(x) ^ int(y)) as f64,
                    // Shifting by 64 or more clears every bit.
                    "<<" => u32::try_from(int(y)).ok().and_then(|n| int(x).checked_shl(n)).unwrap_or(0) as f64,
                    ">>" => u32::try_from(int(y)).ok().and_then(|n| int(x).checked_shr(n)).unwrap_or(0) as f64,
                    _ => unreachable!("parser only produces known operators"),
                }
            }
            Expr::Call(name, args) => {
                let values = args.iter().map(|a| a.eval(vars)).collect::<Result<Vec<_>, _>>()?;
                let arity = |n: usize| {
                    if values.len() == n {
                        Ok(())
                    } else {
                        Err(EvalError(format!("{name}() takes {n} argument{}", if n == 1 { "" } else { "s" })))
                    }
                };
                let at_least_one = || {
                    if values.is_empty() {
                        Err(EvalError(format!("{name}() needs at least one argument")))
                    } else {
                        Ok(())
                    }
                };
                match name.as_str() {
                    "abs" => {
                        arity(1)?;
                        values[0].abs()
                    }
                    "sqrt" => {
                        arity(1)?;
                        values[0].sqrt()
                    }
                    "floor" => {
                        arity(1)?;
                        values[0].floor()
                    }
                    "ceil" => {
                        arity(1)?;
                        values[0].ceil()
                    }
                    "round" => {
                        arity(1)?;
                        values[0].round()
                    }
                    "exp" => {
                        arity(1)?;
                        values[0].exp()
                    }
                    "ln" => {
                        arity(1)?;
                        values[0].ln()
                    }
                    "sin" => {
                        arity(1)?;
                        values[0].sin()
                    }
                    "cos" => {
                        arity(1)?;
                        values[0].cos()
                    }
                    "asin" => {
                        arity(1)?;
                        values[0].asin()
                    }
                    "acos" => {
                        arity(1)?;
                        values[0].acos()
                    }
                    "atan" => {
                        arity(1)?;
                        values[0].atan()
                    }
                    "pow" => {
                        arity(2)?;
                        values[0].powf(values[1])
                    }
                    "bit" => {
                        arity(2)?;
                        ((int(values[0]) >> int(values[1]).clamp(0, 63)) & 1) as f64
                    }
                    "clamp" => {
                        arity(3)?;
                        values[0].clamp(values[1].min(values[2]), values[2].max(values[1]))
                    }
                    "min" => {
                        at_least_one()?;
                        values.iter().copied().fold(f64::INFINITY, f64::min)
                    }
                    "max" => {
                        at_least_one()?;
                        values.iter().copied().fold(f64::NEG_INFINITY, f64::max)
                    }
                    "sum" => values.iter().fold(0.0, |a, b| a + b),
                    // 0-based index of the largest argument (first on ties).
                    "argmax" => {
                        at_least_one()?;
                        values
                            .iter()
                            .enumerate()
                            .fold((0, f64::NEG_INFINITY), |best, (i, &v)| if v > best.1 { (i, v) } else { best })
                            .0 as f64
                    }
                    _ => return Err(EvalError(format!("unknown function “{name}”"))),
                }
            }
        })
    }

    /// Variable names used by the expression.
    pub fn variables(&self) -> Vec<String> {
        let mut out = Vec::new();
        self.collect(&mut out);
        out.sort();
        out.dedup();
        out
    }

    fn collect(&self, out: &mut Vec<String>) {
        match self {
            Expr::Number(_) => {}
            Expr::Var(name) => out.push(name.clone()),
            Expr::Unary(_, e) => e.collect(out),
            Expr::Binary(_, a, b) => {
                a.collect(out);
                b.collect(out);
            }
            Expr::Ternary(a, b, c) => {
                a.collect(out);
                b.collect(out);
                c.collect(out);
            }
            Expr::Call(_, args) => args.iter().for_each(|a| a.collect(out)),
        }
    }
}

/// Parse and evaluate in one step.
pub fn evaluate(src: &str, vars: &HashMap<String, f64>) -> Result<f64, String> {
    parse(src).map_err(|e| e.to_string())?.eval(vars).map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vars(pairs: &[(&str, f64)]) -> HashMap<String, f64> {
        pairs.iter().map(|(k, v)| ((*k).to_owned(), *v)).collect()
    }

    fn eval(src: &str) -> f64 {
        evaluate(src, &vars(&[("in1", 6.0), ("in2", 3.0), ("p1", 0.75)])).unwrap()
    }

    #[test]
    fn arithmetic_and_precedence() {
        assert_eq!(eval("1 + 2 * 3"), 7.0);
        assert_eq!(eval("(1 + 2) * 3"), 9.0);
        assert_eq!(eval("2 ** 3 ** 2"), 512.0, "right associative");
        assert_eq!(eval("-2 ** 2"), -4.0);
        assert_eq!(eval("10 % 4 + 1.5e1 / 3"), 7.0);
        assert_eq!(eval("in1 - in2 - 1"), 2.0, "left associative");
        assert_eq!(eval("+in2"), 3.0);
        assert_eq!(eval("0b1010 + 0x10"), 26.0);
        assert_eq!(eval(".5 * 4"), 2.0);
    }

    #[test]
    fn logic_bits_and_comparisons() {
        assert_eq!(eval("in1 & in2"), 2.0);
        assert_eq!(eval("in1 | in2"), 7.0);
        assert_eq!(eval("in1 ^ in2"), 5.0);
        assert_eq!(eval("1 << 4 >> 2"), 4.0);
        assert_eq!(eval("~0"), -1.0);
        assert_eq!(eval("p1 > 0.5"), 1.0);
        assert_eq!(eval("p1 >= 0.75 && in2 == 3"), 1.0);
        assert_eq!(eval("!(in1 != 6) || false"), 1.0);
        assert_eq!(eval("in1 <= 5"), 0.0);
        assert_eq!(eval("in2 < 3"), 0.0);
        assert_eq!(eval("p1 > 0.5 ? in1 : in2"), 6.0);
        assert_eq!(eval("0 ? 1 : 0 ? 2 : 3"), 3.0, "nested ternary");
        assert_eq!(eval("1 << 100"), 0.0, "shifts saturate instead of panicking");
    }

    #[test]
    fn functions() {
        assert_eq!(eval("bit(in1, 1) + bit(in1, 0)"), 1.0);
        assert_eq!(eval("min(4, in2, 9) + max(1, 2)"), 5.0);
        assert_eq!(eval("argmax(0.1, 0.9, 0.9, 0.2)"), 1.0);
        assert_eq!(eval("sum(1, 2, 3) + sum()"), 6.0);
        assert_eq!(eval("clamp(7, 0, 5) + clamp(-1, 5, 0)"), 5.0);
        assert_eq!(eval("round(sqrt(p1) * 100)"), 87.0);
        assert_eq!(eval("abs(-2) + floor(1.7) + ceil(1.2) + pow(2, 3)"), 13.0);
        assert!((eval("exp(ln(5)) + sin(0) + cos(0)") - 6.0).abs() < 1e-12);
        assert!((eval("asin(1)") - std::f64::consts::FRAC_PI_2).abs() < 1e-15 && eval("acos(1)") == 0.0 && (eval("atan(1) * 4") - std::f64::consts::PI).abs() < 1e-15);
        assert!((eval("pi") - std::f64::consts::PI).abs() < 1e-15);
    }

    #[test]
    fn errors_point_at_the_problem() {
        let e = parse("in1 + ").unwrap_err();
        assert_eq!((e.message.as_str(), e.position), ("unexpected end of expression", 6));
        assert!(parse("in1 = 2").unwrap_err().message.contains("=="));
        assert!(parse("(1 + 2").unwrap_err().message.contains(")"));
        assert!(parse("1 2").unwrap_err().message.contains("after"));
        assert!(parse("1 $ 2").unwrap_err().message.contains("$"));
        assert!(parse("").unwrap_err().message.contains("empty"));
        assert!(parse("0b2").unwrap_err().message.contains("bad number"));
        assert!(parse("a ? b").unwrap_err().message.contains(":"));
        assert!(parse(", 1").unwrap_err().message.contains("expected a number"));
        assert_eq!(parse("1 +").unwrap_err().to_string(), "unexpected end of expression (at character 4)");

        let none = HashMap::new();
        assert!(evaluate("x + 1", &none).unwrap_err().contains("unknown name “x”"));
        assert!(evaluate("nope(1)", &none).unwrap_err().contains("unknown function"));
        assert!(evaluate("sqrt(1, 2)", &none).unwrap_err().contains("takes 1 argument"));
        assert!(evaluate("pow(1)", &none).unwrap_err().contains("takes 2 arguments"));
        assert!(evaluate("max()", &none).unwrap_err().contains("at least one"));
    }

    #[test]
    fn variables_are_listed() {
        let e = parse("bit(in1, 2) ? p2 * in1 : -t").unwrap();
        assert_eq!(e.variables(), vec!["in1", "p2", "t"]);
    }
}
