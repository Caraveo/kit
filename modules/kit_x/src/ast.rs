//! Syntax tree for x.

use std::rc::Rc;

/// A declared type.
#[derive(Clone, Debug, PartialEq)]
pub enum Type {
    String,
    Integer,
    Float,
    Bool,
    Index,
    Object,
    Vector,
    Void,
    Any,
    /// A user class, e.g. `Enemy Target: NULL`.
    Class(String),
}

impl Type {
    pub fn name(&self) -> String {
        match self {
            Type::String => "STRING".into(),
            Type::Integer => "INTEGER".into(),
            Type::Float => "FLOAT".into(),
            Type::Bool => "BOOL".into(),
            Type::Index => "INDEX".into(),
            Type::Object => "OBJECT".into(),
            Type::Vector => "VECTOR".into(),
            Type::Void => "VOID".into(),
            Type::Any => "ANY".into(),
            Type::Class(c) => c.clone(),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BinOp {
    Add,
    Sub,
    Mul,
    Div,
    Rem,
    Eq,
    Ne,
    Lt,
    Gt,
    Le,
    Ge,
    And,
    Or,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UnOp {
    Neg,
    Not,
}

#[derive(Clone, Debug)]
pub struct Expr {
    pub kind: ExprKind,
    pub line: u32,
    pub col: u32,
}

#[derive(Clone, Debug)]
pub enum ExprKind {
    Int(i64),
    Float(f64),
    Str(Rc<str>),
    Bool(bool),
    Null,
    This,
    Ident(Rc<str>),
    List(Vec<Expr>),
    Object(Vec<(Rc<str>, Expr)>),
    Unary(UnOp, Box<Expr>),
    Binary(BinOp, Box<Expr>, Box<Expr>),
    Call {
        callee: Box<Expr>,
        args: Vec<Expr>,
    },
    Field {
        obj: Box<Expr>,
        name: Rc<str>,
    },
    Index {
        obj: Box<Expr>,
        index: Box<Expr>,
    },
    /// `NEW Class(args)` used as an expression.
    New {
        class: Rc<str>,
        args: Vec<Expr>,
    },
    /// `INTEGER(x)`, `STRING(x)`, `VECTOR(x, y, z)` ...
    Convert(Type, Vec<Expr>),
    /// `INVOKE("Sphere", position, ...)` spawns an object inside the calling
    /// object (`inside: true`); `INSTANTIATE(...)` spawns it at the top level.
    Invoke {
        args: Vec<Expr>,
        inside: bool,
    },
    /// `EXIST(expr)` / `EXIST expr`: true when the value is not NULL.
    Exist(Box<Expr>),
}

#[derive(Clone, Debug)]
pub struct Param {
    pub ty: Type,
    pub name: Rc<str>,
}

#[derive(Debug)]
pub struct FuncDecl {
    pub name: Rc<str>,
    pub params: Vec<Param>,
    /// Empty for VOID. More than one: `GIVE a, b` returns an INDEX.
    pub returns: Vec<Type>,
    pub body: Vec<Stmt>,
    pub line: u32,
}

/// Who can see a class field outside the script.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Access {
    /// The default: only the script itself sees it.
    #[default]
    Private,
    /// `PUBLIC FLOAT Health: 100`: shown in the editor, read-only.
    Public,
    /// `EDIT FLOAT Speed: 5.0`: public, and editable in the editor.
    Edit,
}

impl Access {
    pub fn is_public(self) -> bool {
        self != Access::Private
    }
}

#[derive(Debug)]
pub struct FieldDecl {
    pub access: Access,
    pub ty: Type,
    pub name: Rc<str>,
    pub init: Option<Expr>,
    pub line: u32,
}

#[derive(Debug)]
pub struct ClassDecl {
    pub name: Rc<str>,
    /// `CREATE Child RIDE Parent[ ... ]`
    pub parent: Option<Rc<str>>,
    pub fields: Vec<FieldDecl>,
    pub methods: Vec<Rc<FuncDecl>>,
    pub line: u32,
}

#[derive(Clone, Debug)]
pub enum TakeTarget {
    /// An engine module, e.g. `TAKE Transform`.
    Module(Rc<str>),
    /// Another script file, e.g. `TAKE "lib/enemy.x"`.
    File(Rc<str>),
}

#[derive(Clone, Debug)]
pub struct IfBranch {
    pub cond: Expr,
    pub body: Vec<Stmt>,
}

#[derive(Clone, Debug)]
pub enum Stmt {
    Take {
        targets: Vec<TakeTarget>,
        line: u32,
    },
    Class(Rc<ClassDecl>),
    Func(Rc<FuncDecl>),
    Var {
        ty: Type,
        name: Rc<str>,
        init: Option<Expr>,
        line: u32,
    },
    Assign {
        target: Expr,
        value: Expr,
        line: u32,
    },
    If {
        branches: Vec<IfBranch>,
        otherwise: Option<Vec<Stmt>>,
        line: u32,
    },
    Loop {
        cond: Expr,
        body: Vec<Stmt>,
        line: u32,
    },
    Grab {
        var: Rc<str>,
        iter: Expr,
        body: Vec<Stmt>,
        line: u32,
    },
    Give {
        values: Vec<Expr>,
        line: u32,
    },
    Exit {
        line: u32,
    },
    Skip {
        line: u32,
    },
    Display {
        values: Vec<Expr>,
        line: u32,
    },
    /// `WAIT seconds`: pause a coroutine (a method started with `Time.Start`).
    Wait {
        seconds: Expr,
        line: u32,
    },
    /// `NEW Name Class(args)` — declares `Name` as a new instance.
    NewInstance {
        name: Rc<str>,
        class: Rc<str>,
        args: Vec<Expr>,
        line: u32,
    },
    Expr(Expr),
}

/// A parsed x source file.
#[derive(Debug, Default)]
pub struct Program {
    pub stmts: Vec<Stmt>,
}
