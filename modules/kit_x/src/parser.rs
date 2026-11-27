//! Recursive-descent parser for x.
//!
//! x has no statement terminators. To keep that unambiguous, a call `(` or an
//! index `[` must start on the same line as the expression before it.

use crate::XError;
use crate::ast::*;
use crate::lexer::{Kw, Tok, Token, tokenize};
use std::rc::Rc;

pub fn parse(src: &str) -> Result<Program, XError> {
    let tokens = tokenize(src)?;
    let mut p = Parser { toks: tokens, pos: 0 };
    let mut stmts = Vec::new();
    while !p.at(&Tok::Eof) {
        stmts.push(p.stmt(true)?);
    }
    Ok(Program { stmts })
}

struct Parser {
    toks: Vec<Token>,
    pos: usize,
}

type PResult<T> = Result<T, XError>;

impl Parser {
    fn peek(&self) -> &Tok {
        &self.toks[self.pos].tok
    }
    fn peek_at(&self, n: usize) -> &Tok {
        let i = (self.pos + n).min(self.toks.len() - 1);
        &self.toks[i].tok
    }
    fn tok(&self) -> &Token {
        &self.toks[self.pos]
    }
    fn line(&self) -> u32 {
        self.tok().line
    }
    fn prev_line(&self) -> u32 {
        if self.pos == 0 { 0 } else { self.toks[self.pos - 1].line }
    }
    fn at(&self, t: &Tok) -> bool {
        self.peek() == t
    }
    fn at_kw(&self, k: Kw) -> bool {
        matches!(self.peek(), Tok::Kw(x) if *x == k)
    }
    fn advance(&mut self) -> Token {
        let t = self.toks[self.pos].clone();
        if self.pos < self.toks.len() - 1 {
            self.pos += 1;
        }
        t
    }
    fn eat(&mut self, t: &Tok) -> bool {
        if self.at(t) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn eat_kw(&mut self, k: Kw) -> bool {
        if self.at_kw(k) {
            self.advance();
            true
        } else {
            false
        }
    }
    fn err<T>(&self, msg: impl Into<String>) -> PResult<T> {
        Err(XError::at(self.tok().line, self.tok().col, msg))
    }
    fn describe(t: &Tok) -> String {
        match t {
            Tok::Ident(s) => format!("name `{s}`"),
            Tok::Kw(k) => format!("`{}`", k.as_str()),
            Tok::Int(i) => format!("number {i}"),
            Tok::Float(f) => format!("number {f}"),
            Tok::Str(_) => "string".into(),
            Tok::Eof => "end of file".into(),
            other => format!("`{}`", tok_text(other)),
        }
    }
    fn expect(&mut self, t: Tok, what: &str) -> PResult<Token> {
        if self.at(&t) {
            Ok(self.advance())
        } else {
            self.err(format!("expected {what}, found {}", Self::describe(self.peek())))
        }
    }
    fn ident(&mut self, what: &str) -> PResult<Rc<str>> {
        match self.peek().clone() {
            Tok::Ident(s) => {
                self.advance();
                Ok(s.into())
            }
            other => self.err(format!("expected {what}, found {}", Self::describe(&other))),
        }
    }

    // ------------------------------------------------------------------ types

    fn type_kw(k: Kw) -> Option<Type> {
        Some(match k {
            Kw::STRING => Type::String,
            Kw::INTEGER => Type::Integer,
            Kw::FLOAT => Type::Float,
            Kw::BOOL => Type::Bool,
            Kw::INDEX => Type::Index,
            Kw::OBJECT => Type::Object,
            Kw::VECTOR => Type::Vector,
            Kw::VOID => Type::Void,
            Kw::ANY => Type::Any,
            _ => return None,
        })
    }

    /// Parses a type keyword or a class name used as a type.
    fn ty(&mut self) -> PResult<Type> {
        match self.peek().clone() {
            Tok::Kw(k) if k.is_type() => {
                self.advance();
                Ok(Self::type_kw(k).unwrap())
            }
            Tok::Ident(name) => {
                self.advance();
                Ok(Type::Class(name))
            }
            other => self.err(format!("expected a type, found {}", Self::describe(&other))),
        }
    }

    /// Does a declaration (variable or function) start here?
    /// `TYPE Name ...`, `TYPE, TYPE ...`, `TYPE CREATE ...`, `Class Name:` / `Class Name(`.
    fn at_declaration(&self) -> bool {
        match self.peek() {
            Tok::Kw(k) if k.is_type() => {
                matches!(self.peek_at(1), Tok::Ident(_) | Tok::Comma | Tok::Kw(Kw::CREATE))
            }
            Tok::Ident(_) => {
                let same_line = self.toks[self.pos + 1].line == self.line();
                match self.peek_at(1) {
                    Tok::Kw(Kw::CREATE) => true,
                    Tok::Ident(_) if same_line => {
                        matches!(self.peek_at(2), Tok::Colon | Tok::LParen)
                            || self.toks[self.pos + 2].line != self.line()
                    }
                    _ => false,
                }
            }
            _ => false,
        }
    }

    // ------------------------------------------------------------- statements

    fn block(&mut self) -> PResult<Vec<Stmt>> {
        self.expect(Tok::LBracket, "`[` to open a block")?;
        let mut body = Vec::new();
        while !self.at(&Tok::RBracket) {
            if self.at(&Tok::Eof) {
                return self.err("unclosed block: expected `]`");
            }
            body.push(self.stmt(false)?);
        }
        self.advance();
        Ok(body)
    }

    fn stmt(&mut self, top_level: bool) -> PResult<Stmt> {
        let line = self.line();
        match self.peek().clone() {
            Tok::Kw(Kw::TAKE) | Tok::Kw(Kw::BRING) => {
                self.advance();
                let mut targets = Vec::new();
                loop {
                    match self.peek().clone() {
                        Tok::Ident(m) => {
                            self.advance();
                            targets.push(TakeTarget::Module(m.into()));
                        }
                        Tok::Str(f) => {
                            self.advance();
                            targets.push(TakeTarget::File(f.into()));
                        }
                        other => {
                            return self.err(format!(
                                "TAKE expects a module name or a \"file.x\" path, found {}",
                                Self::describe(&other)
                            ));
                        }
                    }
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                if !top_level {
                    return Err(XError::at(line, 1, "TAKE is only allowed at the top of a file"));
                }
                Ok(Stmt::Take { targets, line })
            }
            Tok::Kw(Kw::WAIT) => {
                self.advance();
                let seconds = self.expr()?;
                Ok(Stmt::Wait { seconds, line })
            }
            Tok::Kw(Kw::CREATE) => {
                self.advance();
                let name = self.ident("a class or function name after CREATE")?;
                if self.at(&Tok::LParen) {
                    // `CREATE Name(...)[...]` — a VOID function.
                    let f = self.func_rest(name, Vec::new(), line)?;
                    return Ok(Stmt::Func(f));
                }
                Ok(Stmt::Class(self.class_rest(name, line)?))
            }
            Tok::Kw(k @ (Kw::PUBLIC | Kw::EDIT)) => Err(XError::at(
                line,
                1,
                format!("{} only works on class fields, inside `CREATE Name[ ... ]`", k.as_str()),
            )),
            Tok::Kw(Kw::IF) => self.if_stmt(),
            Tok::Kw(Kw::LOOP) => {
                self.advance();
                self.expect(Tok::LParen, "`(` after LOOP")?;
                let cond = self.expr()?;
                self.expect(Tok::RParen, "`)`")?;
                let body = self.block()?;
                Ok(Stmt::Loop { cond, body, line })
            }
            Tok::Kw(Kw::GRAB) => {
                self.advance();
                let var = self.ident("a loop variable name after GRAB")?;
                if !self.eat_kw(Kw::IN) {
                    return self.err("expected IN, as in `GRAB Item IN Items[ ... ]`");
                }
                let iter = self.expr_no_index()?;
                let body = self.block()?;
                Ok(Stmt::Grab { var, iter, body, line })
            }
            Tok::Kw(Kw::GIVE) => {
                self.advance();
                let mut values = Vec::new();
                if self.line() == line && !self.at(&Tok::RBracket) && !self.at(&Tok::Eof) {
                    values.push(self.expr()?);
                    while self.eat(&Tok::Comma) {
                        values.push(self.expr()?);
                    }
                }
                Ok(Stmt::Give { values, line })
            }
            Tok::Kw(Kw::EXIT) => {
                self.advance();
                Ok(Stmt::Exit { line })
            }
            Tok::Kw(Kw::SKIP) => {
                self.advance();
                Ok(Stmt::Skip { line })
            }
            Tok::Kw(Kw::DISPLAY) => {
                self.advance();
                let close = if self.eat(&Tok::LBracket) {
                    Tok::RBracket
                } else if self.eat(&Tok::LParen) {
                    Tok::RParen
                } else {
                    return self.err("expected `[` after DISPLAY, as in DISPLAY[\"Hello\"]");
                };
                let mut values = Vec::new();
                if !self.at(&close) {
                    values.push(self.expr()?);
                    while self.eat(&Tok::Comma) {
                        values.push(self.expr()?);
                    }
                }
                self.expect(close, "closing bracket for DISPLAY")?;
                Ok(Stmt::Display { values, line })
            }
            Tok::Kw(Kw::NEW) if matches!(self.peek_at(2), Tok::Ident(_)) => {
                self.advance();
                let name = self.ident("an instance name")?;
                let class = self.ident("a class name")?;
                let args = if self.at(&Tok::LParen) { self.args()? } else { Vec::new() };
                Ok(Stmt::NewInstance { name, class, args, line })
            }
            Tok::Kw(Kw::CALL) => {
                self.advance();
                let e = self.expr()?;
                if !matches!(e.kind, ExprKind::Call { .. }) {
                    return Err(XError::at(e.line, e.col, "CALL expects a call, e.g. CALL Jump()"));
                }
                Ok(Stmt::Expr(e))
            }
            _ if self.at_declaration() => self.declaration(),
            _ => {
                let target = self.expr()?;
                if self.eat(&Tok::Colon) {
                    match target.kind {
                        ExprKind::Ident(_) | ExprKind::Field { .. } | ExprKind::Index { .. } => {}
                        _ => {
                            return Err(XError::at(
                                target.line,
                                target.col,
                                "can only assign to a name, a field or an index",
                            ));
                        }
                    }
                    let value = self.expr()?;
                    return Ok(Stmt::Assign { target, value, line });
                }
                Ok(Stmt::Expr(target))
            }
        }
    }

    fn declaration(&mut self) -> PResult<Stmt> {
        let line = self.line();
        let mut types = vec![self.ty()?];
        while self.eat(&Tok::Comma) {
            types.push(self.ty()?);
        }
        let is_func = self.eat_kw(Kw::CREATE);
        let name = self.ident("a name")?;
        if is_func || self.at(&Tok::LParen) {
            let returns = if types == [Type::Void] { Vec::new() } else { types };
            return Ok(Stmt::Func(self.func_rest(name, returns, line)?));
        }
        if types.len() != 1 {
            return self.err("a variable has exactly one type");
        }
        let ty = types.pop().unwrap();
        if ty == Type::Void {
            return Err(XError::at(line, 1, "a variable cannot be VOID"));
        }
        let init = if self.eat(&Tok::Colon) { Some(self.expr()?) } else { None };
        Ok(Stmt::Var { ty, name, init, line })
    }

    fn params(&mut self) -> PResult<Vec<Param>> {
        self.expect(Tok::LParen, "`(`")?;
        let mut params = Vec::new();
        if !self.at(&Tok::RParen) {
            loop {
                // Parameters may be typed (`FLOAT dt`) or untyped (`dt`, means ANY).
                let (ty, name) = match (self.peek().clone(), self.peek_at(1).clone()) {
                    (Tok::Ident(n), Tok::Comma | Tok::RParen) => {
                        self.advance();
                        (Type::Any, Rc::from(n.as_str()))
                    }
                    _ => {
                        let ty = self.ty()?;
                        (ty, self.ident("a parameter name")?)
                    }
                };
                params.push(Param { ty, name });
                if !self.eat(&Tok::Comma) {
                    break;
                }
            }
        }
        self.expect(Tok::RParen, "`)` to close the parameter list")?;
        Ok(params)
    }

    fn func_rest(&mut self, name: Rc<str>, returns: Vec<Type>, line: u32) -> PResult<Rc<FuncDecl>> {
        let params = self.params()?;
        let body = self.block()?;
        Ok(Rc::new(FuncDecl { name, params, returns, body, line }))
    }

    fn class_rest(&mut self, name: Rc<str>, line: u32) -> PResult<Rc<ClassDecl>> {
        let parent = if self.eat_kw(Kw::RIDE) { Some(self.ident("a parent class after RIDE")?) } else { None };
        self.expect(Tok::LBracket, "`[` to open the class body")?;
        let mut fields = Vec::new();
        let mut methods = Vec::new();
        while !self.eat(&Tok::RBracket) {
            if self.at(&Tok::Eof) {
                return self.err(format!("unclosed class `{name}`: expected `]`"));
            }
            let mline = self.line();
            let access = if self.eat_kw(Kw::PUBLIC) {
                Access::Public
            } else if self.eat_kw(Kw::EDIT) {
                Access::Edit
            } else {
                Access::Private
            };
            let word = if access == Access::Edit { "EDIT" } else { "PUBLIC" };
            if self.eat_kw(Kw::CREATE) {
                if access.is_public() {
                    return self
                        .err(format!("{word} goes in front of a field (`{word} FLOAT Speed: 5.0`), not a method"));
                }
                let n = self.ident("a method name")?;
                methods.push(self.func_rest(n, Vec::new(), mline)?);
                continue;
            }
            if access.is_public() && !self.at_declaration() {
                return self.err(format!("expected a field after {word}, like `{word} FLOAT Speed: 5.0`"));
            }
            if !self.at_declaration() {
                return self.err(format!(
                    "class bodies hold fields (`FLOAT Speed: 1.0`) and methods (`VOID CREATE Update()[ ]`), found {}",
                    Self::describe(self.peek())
                ));
            }
            match self.declaration()? {
                Stmt::Func(_) if access.is_public() => {
                    return Err(XError::at(mline, 1, format!("{word} goes in front of a field, not a method")));
                }
                Stmt::Func(f) => methods.push(f),
                Stmt::Var { ty, name, init, line } => {
                    if access == Access::Edit && !editable_type(&ty) {
                        return Err(XError::at(
                            line,
                            1,
                            format!(
                                "EDIT fields must be STRING, INTEGER, FLOAT, BOOL or VECTOR; `{name}` is {} (use PUBLIC to show it read-only)",
                                ty.name()
                            ),
                        ));
                    }
                    fields.push(FieldDecl { access, ty, name, init, line })
                }
                _ => unreachable!(),
            }
        }
        Ok(Rc::new(ClassDecl { name, parent, fields, methods, line }))
    }

    fn if_stmt(&mut self) -> PResult<Stmt> {
        let line = self.line();
        self.advance(); // IF
        let mut branches = Vec::new();
        let mut otherwise = None;
        loop {
            self.expect(Tok::LParen, "`(` after IF")?;
            let cond = self.expr()?;
            self.expect(Tok::RParen, "`)`")?;
            let body = self.block()?;
            branches.push(IfBranch { cond, body });
            if self.eat_kw(Kw::ELSE) {
                if self.eat_kw(Kw::IF) {
                    continue;
                }
                otherwise = Some(self.block()?);
            }
            break;
        }
        Ok(Stmt::If { branches, otherwise, line })
    }

    // ------------------------------------------------------------ expressions

    fn args(&mut self) -> PResult<Vec<Expr>> {
        self.expect(Tok::LParen, "`(`")?;
        let mut args = Vec::new();
        if !self.at(&Tok::RParen) {
            args.push(self.expr()?);
            while self.eat(&Tok::Comma) {
                args.push(self.expr()?);
            }
        }
        self.expect(Tok::RParen, "`)` to close the argument list")?;
        Ok(args)
    }

    pub fn expr(&mut self) -> PResult<Expr> {
        self.binary(0, true)
    }

    /// Expression where a trailing `[` is a block, not indexing (`GRAB x IN List[`).
    fn expr_no_index(&mut self) -> PResult<Expr> {
        self.binary(0, false)
    }

    fn binop(&self) -> Option<(BinOp, u8)> {
        Some(match self.peek() {
            Tok::OrOr | Tok::Kw(Kw::OR) => (BinOp::Or, 1),
            Tok::AndAnd | Tok::Kw(Kw::AND) => (BinOp::And, 2),
            Tok::EqEq => (BinOp::Eq, 3),
            Tok::NotEq => (BinOp::Ne, 3),
            Tok::Lt => (BinOp::Lt, 4),
            Tok::Gt => (BinOp::Gt, 4),
            Tok::Le => (BinOp::Le, 4),
            Tok::Ge => (BinOp::Ge, 4),
            Tok::Plus => (BinOp::Add, 5),
            Tok::Minus => (BinOp::Sub, 5),
            Tok::Star => (BinOp::Mul, 6),
            Tok::Slash => (BinOp::Div, 6),
            Tok::Percent => (BinOp::Rem, 6),
            _ => return None,
        })
    }

    fn binary(&mut self, min_prec: u8, allow_index: bool) -> PResult<Expr> {
        let mut lhs = self.unary(allow_index)?;
        while let Some((op, prec)) = self.binop() {
            if prec <= min_prec {
                break;
            }
            // A leading `-` on a new line starts a new statement, not a subtraction.
            if op == BinOp::Sub && self.line() != self.prev_line() {
                break;
            }
            let t = self.advance();
            let rhs = self.binary(prec, allow_index)?;
            lhs = Expr { kind: ExprKind::Binary(op, Box::new(lhs), Box::new(rhs)), line: t.line, col: t.col };
        }
        Ok(lhs)
    }

    fn unary(&mut self, allow_index: bool) -> PResult<Expr> {
        let t = self.tok().clone();
        let op = match &t.tok {
            Tok::Minus => Some(UnOp::Neg),
            Tok::Bang | Tok::Kw(Kw::NOT) => Some(UnOp::Not),
            _ => None,
        };
        if let Some(op) = op {
            self.advance();
            let inner = self.unary(allow_index)?;
            return Ok(Expr { kind: ExprKind::Unary(op, Box::new(inner)), line: t.line, col: t.col });
        }
        if self.eat_kw(Kw::EXIST) {
            let inner = self.unary(allow_index)?;
            return Ok(Expr { kind: ExprKind::Exist(Box::new(inner)), line: t.line, col: t.col });
        }
        self.postfix(allow_index)
    }

    fn postfix(&mut self, allow_index: bool) -> PResult<Expr> {
        let mut e = self.primary()?;
        loop {
            let same_line = self.line() == self.prev_line();
            let t = self.tok().clone();
            if same_line && self.at(&Tok::LParen) {
                let args = self.args()?;
                e = Expr { kind: ExprKind::Call { callee: Box::new(e), args }, line: t.line, col: t.col };
            } else if same_line && allow_index && self.at(&Tok::LBracket) {
                self.advance();
                let index = self.expr()?;
                self.expect(Tok::RBracket, "`]` to close the index")?;
                e = Expr {
                    kind: ExprKind::Index { obj: Box::new(e), index: Box::new(index) },
                    line: t.line,
                    col: t.col,
                };
            } else if self.at(&Tok::Dot) {
                self.advance();
                let name = match self.peek().clone() {
                    Tok::Ident(n) => n,
                    // Allow keyword-looking members such as `.STRING` from multi-returns.
                    Tok::Kw(k) => k.as_str().to_string(),
                    other => {
                        return self.err(format!("expected a member name after `.`, found {}", Self::describe(&other)));
                    }
                };
                self.advance();
                e = Expr { kind: ExprKind::Field { obj: Box::new(e), name: name.into() }, line: t.line, col: t.col };
            } else {
                break;
            }
        }
        Ok(e)
    }

    fn primary(&mut self) -> PResult<Expr> {
        let t = self.advance();
        let (line, col) = (t.line, t.col);
        let kind = match t.tok {
            Tok::Int(i) => ExprKind::Int(i),
            Tok::Float(f) => ExprKind::Float(f),
            Tok::Str(s) => ExprKind::Str(s.into()),
            Tok::Kw(Kw::TRUE) => ExprKind::Bool(true),
            Tok::Kw(Kw::FALSE) => ExprKind::Bool(false),
            Tok::Kw(Kw::NULL) => ExprKind::Null,
            Tok::Kw(Kw::THIS) => ExprKind::This,
            Tok::Ident(n) => ExprKind::Ident(n.into()),
            Tok::Kw(Kw::NEW) => {
                let class = self.ident("a class name after NEW")?;
                let args = if self.at(&Tok::LParen) { self.args()? } else { Vec::new() };
                ExprKind::New { class, args }
            }
            Tok::Kw(k @ (Kw::INVOKE | Kw::INSTANTIATE)) => {
                if !self.at(&Tok::LParen) {
                    return self.err(format!(
                        "{} needs what to spawn in parentheses, e.g. {}(\"Sphere\")",
                        k.as_str(),
                        k.as_str()
                    ));
                }
                ExprKind::Invoke { args: self.args()?, inside: k == Kw::INVOKE }
            }
            Tok::Kw(k) if k.is_type() && self.at(&Tok::LParen) => {
                let args = self.args()?;
                ExprKind::Convert(Self::type_kw(k).unwrap(), args)
            }
            Tok::LParen => {
                let e = self.expr()?;
                self.expect(Tok::RParen, "`)`")?;
                return Ok(e);
            }
            Tok::LBracket => {
                let mut items = Vec::new();
                if !self.at(&Tok::RBracket) {
                    items.push(self.expr()?);
                    while self.eat(&Tok::Comma) {
                        if self.at(&Tok::RBracket) {
                            break;
                        }
                        items.push(self.expr()?);
                    }
                }
                self.expect(Tok::RBracket, "`]` to close the list")?;
                ExprKind::List(items)
            }
            Tok::LBrace => {
                let mut entries = Vec::new();
                while !self.at(&Tok::RBrace) {
                    let key: Rc<str> = match self.advance().tok {
                        Tok::Str(s) | Tok::Ident(s) => s.into(),
                        other => {
                            return self.err(format!("expected an object key, found {}", Self::describe(&other)));
                        }
                    };
                    self.expect(Tok::Colon, "`:` after the object key")?;
                    entries.push((key, self.expr()?));
                    if !self.eat(&Tok::Comma) {
                        break;
                    }
                }
                self.expect(Tok::RBrace, "`}` to close the object")?;
                ExprKind::Object(entries)
            }
            other => {
                return Err(XError::at(line, col, format!("expected an expression, found {}", Self::describe(&other))));
            }
        };
        Ok(Expr { kind, line, col })
    }
}

fn tok_text(t: &Tok) -> &'static str {
    match t {
        Tok::LBracket => "[",
        Tok::RBracket => "]",
        Tok::LParen => "(",
        Tok::RParen => ")",
        Tok::LBrace => "{",
        Tok::RBrace => "}",
        Tok::Comma => ",",
        Tok::Colon => ":",
        Tok::Dot => ".",
        Tok::Plus => "+",
        Tok::Minus => "-",
        Tok::Star => "*",
        Tok::Slash => "/",
        Tok::Percent => "%",
        Tok::EqEq => "==",
        Tok::NotEq => "!=",
        Tok::Lt => "<",
        Tok::Gt => ">",
        Tok::Le => "<=",
        Tok::Ge => ">=",
        Tok::Bang => "!",
        Tok::AndAnd => "&&",
        Tok::OrOr => "||",
        _ => "?",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_class_and_methods() {
        let p = parse(
            r#"
TAKE Transform, Input
CREATE Player RIDE Actor[
    FLOAT Speed: 2.0
    INDEX Items: ["a", "b"]
    VOID CREATE Update(FLOAT dt)[
        IF(Input.Down("Space"))[
            Transform.Move(0.0, Speed * dt, 0.0)
        ] ELSE IF(Speed > 3)[
            Speed: Speed - 1
        ] ELSE [
            DISPLAY["idle"]
        ]
    ]
    STRING, INTEGER Info()[
        GIVE "p", 1
    ]
]
"#,
        )
        .unwrap();
        assert_eq!(p.stmts.len(), 2);
        let Stmt::Class(c) = &p.stmts[1] else { panic!() };
        assert_eq!(&*c.name, "Player");
        assert_eq!(c.parent.as_deref(), Some("Actor"));
        assert_eq!(c.fields.len(), 2);
        assert_eq!(c.methods.len(), 2);
        assert_eq!(c.methods[1].returns, vec![Type::String, Type::Integer]);
    }

    #[test]
    fn newline_separates_statements() {
        // `Fn` on one line and `(1)` on the next must not become a call.
        let p = parse("INTEGER A: 1\nA: A\n(A)\nGRAB C IN Cats[ DISPLAY[C] ]").unwrap();
        assert_eq!(p.stmts.len(), 4);
    }

    #[test]
    fn readme_examples_parse() {
        parse(
            r#"
STRING Welcome: "Hello" + ' World!'
INDEX Greetings: ["Hello", "Hey!", "Hi!"]
OBJECT Obj: { "Key": "Value" }
VOID CREATE X()[
    DISPLAY[Welcome]
]
CALL X()
NEW X2 Box()
GRAB Greeting IN Greetings[
    DISPLAY[Greeting]
]
LOOP(TRUE)[
    EXIT
]
"#,
        )
        .unwrap();
    }

    #[test]
    fn error_has_position() {
        let e = parse("FLOAT A: \nIF(1)[").unwrap_err();
        assert_eq!(e.line, 2);
    }
}

/// Types the editor can edit (EDIT fields).
pub fn editable_type(ty: &Type) -> bool {
    matches!(ty, Type::String | Type::Integer | Type::Float | Type::Bool | Type::Vector)
}
