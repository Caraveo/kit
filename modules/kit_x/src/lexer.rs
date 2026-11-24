//! Tokenizer for x.

use crate::XError;

#[derive(Clone, Debug, PartialEq)]
pub enum Tok {
    Ident(String),
    Kw(Kw),
    Int(i64),
    Float(f64),
    Str(String),
    LBracket,
    RBracket,
    LParen,
    RParen,
    LBrace,
    RBrace,
    Comma,
    Colon,
    Dot,
    Plus,
    Minus,
    Star,
    Slash,
    Percent,
    EqEq,
    NotEq,
    Lt,
    Gt,
    Le,
    Ge,
    Bang,
    AndAnd,
    OrOr,
    Eof,
}

macro_rules! keywords {
    ($($name:ident),* $(,)?) => {
        /// Reserved ALL-CAPS words.
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        pub enum Kw { $($name),* }
        impl Kw {
            pub fn from_str(s: &str) -> Option<Kw> {
                match s { $(stringify!($name) => Some(Kw::$name),)* _ => None }
            }
            pub fn as_str(self) -> &'static str {
                match self { $(Kw::$name => stringify!($name),)* }
            }
        }
    };
}

keywords!(
    // declarations
    CREATE, NEW, RIDE, TAKE, BRING, CALL, // control flow
    GIVE, IF, ELSE, LOOP, GRAB, IN, EXIT, SKIP, DISPLAY, // literals / operators
    TRUE, FALSE, NULL, AND, OR, NOT, THIS, EXIST, // types
    STRING, INTEGER, FLOAT, BOOL, INDEX, OBJECT, VOID, VECTOR, ANY,
);

impl Kw {
    pub fn is_type(self) -> bool {
        matches!(
            self,
            Kw::STRING | Kw::INTEGER | Kw::FLOAT | Kw::BOOL | Kw::INDEX | Kw::OBJECT | Kw::VOID | Kw::VECTOR | Kw::ANY
        )
    }
}

#[derive(Clone, Debug)]
pub struct Token {
    pub tok: Tok,
    pub line: u32,
    pub col: u32,
}

pub fn tokenize(src: &str) -> Result<Vec<Token>, XError> {
    let chars: Vec<char> = src.chars().collect();
    let mut out = Vec::new();
    let (mut i, mut line, mut col) = (0usize, 1u32, 1u32);

    macro_rules! bump {
        () => {{
            let c = chars[i];
            i += 1;
            if c == '\n' {
                line += 1;
                col = 1;
            } else {
                col += 1;
            }
            c
        }};
    }

    while i < chars.len() {
        let c = chars[i];
        let (tl, tc) = (line, col);
        let push = |out: &mut Vec<Token>, tok| out.push(Token { tok, line: tl, col: tc });

        if c.is_whitespace() {
            bump!();
            continue;
        }
        if c == '#' {
            while i < chars.len() && chars[i] != '\n' {
                bump!();
            }
            continue;
        }
        if c.is_ascii_digit() {
            let start = i;
            while i < chars.len() && (chars[i].is_ascii_digit() || chars[i] == '_') {
                bump!();
            }
            let mut is_float = false;
            if i + 1 < chars.len() && chars[i] == '.' && chars[i + 1].is_ascii_digit() {
                is_float = true;
                bump!();
                while i < chars.len() && chars[i].is_ascii_digit() {
                    bump!();
                }
            }
            if i < chars.len() && (chars[i] == 'e' || chars[i] == 'E') {
                let save = (i, line, col);
                bump!();
                if i < chars.len() && (chars[i] == '+' || chars[i] == '-') {
                    bump!();
                }
                if i < chars.len() && chars[i].is_ascii_digit() {
                    is_float = true;
                    while i < chars.len() && chars[i].is_ascii_digit() {
                        bump!();
                    }
                } else {
                    (i, line, col) = save;
                }
            }
            let text: String = chars[start..i].iter().filter(|c| **c != '_').collect();
            let tok = if is_float {
                Tok::Float(text.parse().map_err(|_| XError::at(tl, tc, "bad number"))?)
            } else {
                Tok::Int(text.parse().map_err(|_| XError::at(tl, tc, "integer too large"))?)
            };
            push(&mut out, tok);
            continue;
        }
        if c.is_alphabetic() || c == '_' {
            let start = i;
            while i < chars.len() && (chars[i].is_alphanumeric() || chars[i] == '_') {
                bump!();
            }
            let word: String = chars[start..i].iter().collect();
            let tok = match Kw::from_str(&word) {
                Some(k) => Tok::Kw(k),
                None => Tok::Ident(word),
            };
            push(&mut out, tok);
            continue;
        }
        if c == '"' || c == '\'' {
            let quote = bump!();
            let mut s = String::new();
            loop {
                if i >= chars.len() {
                    return Err(XError::at(tl, tc, "unterminated string"));
                }
                let ch = bump!();
                if ch == quote {
                    break;
                }
                if ch == '\\' {
                    if i >= chars.len() {
                        return Err(XError::at(tl, tc, "unterminated string"));
                    }
                    let e = bump!();
                    s.push(match e {
                        'n' => '\n',
                        't' => '\t',
                        'r' => '\r',
                        '0' => '\0',
                        other => other,
                    });
                } else {
                    s.push(ch);
                }
            }
            push(&mut out, Tok::Str(s));
            continue;
        }
        let next = chars.get(i + 1).copied();
        let (tok, len) = match (c, next) {
            ('=', Some('=')) => (Tok::EqEq, 2),
            ('!', Some('=')) => (Tok::NotEq, 2),
            ('<', Some('=')) => (Tok::Le, 2),
            ('>', Some('=')) => (Tok::Ge, 2),
            ('&', Some('&')) => (Tok::AndAnd, 2),
            ('|', Some('|')) => (Tok::OrOr, 2),
            ('[', _) => (Tok::LBracket, 1),
            (']', _) => (Tok::RBracket, 1),
            ('(', _) => (Tok::LParen, 1),
            (')', _) => (Tok::RParen, 1),
            ('{', _) => (Tok::LBrace, 1),
            ('}', _) => (Tok::RBrace, 1),
            (',', _) => (Tok::Comma, 1),
            (':', _) => (Tok::Colon, 1),
            ('.', _) => (Tok::Dot, 1),
            ('+', _) => (Tok::Plus, 1),
            ('-', _) => (Tok::Minus, 1),
            ('*', _) => (Tok::Star, 1),
            ('/', _) => (Tok::Slash, 1),
            ('%', _) => (Tok::Percent, 1),
            ('<', _) => (Tok::Lt, 1),
            ('>', _) => (Tok::Gt, 1),
            ('!', _) => (Tok::Bang, 1),
            ('=', _) => {
                return Err(XError::at(tl, tc, "x assigns with ':' (e.g. `Score: Score + 1`); use '==' to compare"));
            }
            _ => return Err(XError::at(tl, tc, format!("unexpected character '{c}'"))),
        };
        for _ in 0..len {
            bump!();
        }
        push(&mut out, tok);
    }
    out.push(Token { tok: Tok::Eof, line, col });
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn basics() {
        let t = tokenize("FLOAT Speed: 2.5 # comment\nIF(a >= 10)[ DISPLAY[\"hi\\n\"] ]").unwrap();
        let toks: Vec<Tok> = t.into_iter().map(|t| t.tok).collect();
        assert_eq!(toks[0], Tok::Kw(Kw::FLOAT));
        assert_eq!(toks[1], Tok::Ident("Speed".into()));
        assert_eq!(toks[3], Tok::Float(2.5));
        assert_eq!(toks[4], Tok::Kw(Kw::IF));
        assert!(toks.contains(&Tok::Ge));
        assert!(toks.contains(&Tok::Str("hi\n".into())));
    }

    #[test]
    fn single_equals_is_helpful_error() {
        let e = tokenize("A = 1").unwrap_err();
        assert!(e.message.contains("assigns with ':'"));
    }
}
