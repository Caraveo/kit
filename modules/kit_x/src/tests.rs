use crate::*;

fn out(src: &str) -> Vec<String> {
    let mut host = StdHost { quiet: true, ..Default::default() };
    if let Err(e) = run("test.x", src, &mut host) {
        panic!("{e}");
    }
    host.output
}

fn err(src: &str) -> String {
    let mut host = StdHost { quiet: true, ..Default::default() };
    match run("test.x", src, &mut host) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.to_string(),
    }
}

#[test]
fn readme_basics() {
    let o = out(r#"
STRING Welcome: "Hello" + ' World!'
INDEX Greetings: ["Hello", "Hey!", "Hi!"]
FLOAT Value: 12
INTEGER Int: 0
BOOL Check: TRUE
VOID CREATE X()[
    DISPLAY[Welcome]
]
CALL X()
GRAB Greeting IN Greetings[
    DISPLAY[Greeting]
]
DISPLAY[Value, Int, Check]
"#);
    assert_eq!(o, ["Hello World!", "Hello", "Hey!", "Hi!", "12.0 0 TRUE"]);
}

#[test]
fn classes_fields_methods_inheritance() {
    let o = out(r#"
CREATE Actor[
    STRING Name: "actor"
    INTEGER Health: 10
    VOID CREATE Hit(INTEGER amount)[
        Health: Health - amount
    ]
    STRING CREATE Describe()[
        GIVE Name + " (" + Health + ")"
    ]
]
CREATE Player RIDE Actor[
    STRING Name: "hero"
    VOID CREATE New(INTEGER hp)[
        Health: hp
    ]
    STRING CREATE Describe()[
        GIVE "Player " + Name + " hp=" + Health
    ]
]
NEW A Actor()
A.Hit(3)
DISPLAY[A.Describe()]
NEW P Player(50)
P.Hit(5)
DISPLAY[P.Describe()]
Player Q: NEW Player(7)
DISPLAY[Q.Health, Q.Name]
P.Name: "zed"
DISPLAY[P.Name]
"#);
    assert_eq!(o, ["actor (7)", "Player hero hp=45", "7 hero", "zed"]);
}

#[test]
fn control_flow() {
    let o = out(r#"
INTEGER I: 0
INTEGER Sum: 0
LOOP(TRUE)[
    I: I + 1
    IF(I % 2 == 0)[ SKIP ]
    IF(I > 9)[ EXIT ]
    Sum: Sum + I
]
DISPLAY[Sum]
IF(Sum > 100)[ DISPLAY["big"] ] ELSE IF(Sum > 10)[ DISPLAY["medium"] ] ELSE [ DISPLAY["small"] ]
GRAB N IN RANGE(3)[ DISPLAY[N] ]
GRAB C IN "ab"[ DISPLAY[C] ]
"#);
    assert_eq!(o, ["25", "medium", "0", "1", "2", "a", "b"]);
}

#[test]
fn multiple_returns_and_recursion() {
    let o = out(r#"
STRING, INTEGER FUN()[
    GIVE "Hello", 10
]
INDEX R: FUN()
DISPLAY[R[0], R[1], R[-1]]
INTEGER CREATE Fib(INTEGER n)[
    IF(n < 2)[ GIVE n ]
    GIVE Fib(n - 1) + Fib(n - 2)
]
DISPLAY[Fib(15)]
"#);
    assert_eq!(o, ["Hello 10 10", "610"]);
}

#[test]
fn collections_and_vectors() {
    let o = out(r#"
INDEX Cats: ["Pixie!", "Diva"]
Cats.Add("Chance")
DISPLAY[Cats.Length(), Cats[2], Cats.Contains("Diva")]
OBJECT Obj: { "Key": "Value", Count: 2 }
Obj.Count: Obj.Count + 1
Obj["New"]: TRUE
DISPLAY[Obj.Key, Obj.Count, Obj.Keys()]
VECTOR V: VECTOR(1, 2, 3)
V.Y: 5
VECTOR W: V * 2 + VECTOR(1, 1, 1)
DISPLAY[V, W.X, VECTOR(3, 4, 0).Length()]
"#);
    assert_eq!(o, ["3 Chance TRUE", "Value 3 [\"Key\", \"Count\", \"New\"]", "(1.0, 5.0, 3.0) 3.0 5.0"]);
}

#[test]
fn math_module_requires_take() {
    assert!(err("DISPLAY[Math.PI]").contains("TAKE Math"));
    let o = out("TAKE Math\nDISPLAY[Math.Floor(Math.PI), Math.Max(2, 7), Math.Abs(-3)]");
    assert_eq!(o, ["3 7 3"]);
}

#[test]
fn strict_types() {
    assert!(err("INTEGER A: 1.5").contains("INTEGER but got FLOAT"));
    assert!(err("FLOAT A: \"x\"").contains("FLOAT but got STRING"));
    assert!(err("B: 1").contains("not declared"));
    assert!(err("INTEGER A: 1\nA: \"s\"").contains("INTEGER but got STRING"));
    // Ints widen to FLOAT.
    assert_eq!(out("FLOAT F: 2\nF: F + 1\nDISPLAY[F]"), ["3.0"]);
    assert_eq!(out("DISPLAY[INTEGER(\"42\") + 1, STRING(3) + \"!\", FLOAT(1)]"), ["43 3! 1.0"]);
}

#[test]
fn runtime_errors_have_lines() {
    let e = err("INTEGER A: 1\n\nDISPLAY[A / 0]");
    assert!(e.starts_with("test.x:3"), "{e}");
    let e = err("INDEX L: []\nDISPLAY[L[3]]");
    assert!(e.contains("out of range"), "{e}");
}

#[test]
fn infinite_loop_is_stopped() {
    let mut host = StdHost { quiet: true, ..Default::default() };
    let program = parse("LOOP(TRUE)[ ]").unwrap();
    let mut i = Interpreter::new("t.x");
    i.step_budget = 10_000;
    let e = i.load(&program, &mut host).unwrap_err();
    assert!(e.message.contains("too long"));
}

struct EngineHost {
    pos: [f64; 3],
    log: Vec<String>,
}

impl Host for EngineHost {
    fn has_module(&self, name: &str) -> bool {
        name == "Transform"
    }
    fn call_module(&mut self, module: &str, member: &str, args: &[Value]) -> Result<Value, String> {
        assert_eq!(module, "Transform");
        match member {
            "Move" => {
                for (i, a) in args.iter().enumerate() {
                    self.pos[i] += a.as_f64().ok_or("number")?;
                }
                Ok(Value::Null)
            }
            "Position" => Ok(Value::Vector(self.pos)),
            "SetPosition" => {
                self.pos = args[0].as_vector().ok_or("vector")?;
                Ok(Value::Null)
            }
            _ => Err(format!("no member {member}")),
        }
    }
    fn display(&mut self, text: &str) {
        self.log.push(text.into());
    }
}

#[test]
fn host_modules_and_lifecycle() {
    let src = r#"
TAKE Transform
CREATE Mover[
    FLOAT Speed: 2.0
    INTEGER Ticks: 0
    VOID CREATE Init()[
        DISPLAY["ready"]
    ]
    VOID CREATE Update(FLOAT dt)[
        Ticks: Ticks + 1
        Transform.Move(Speed * dt, 0, 0)
        IF(Transform.Position.X > 1.5)[
            Transform.Position: VECTOR(0, 9, 0)
        ]
    ]
]
"#;
    let mut host = EngineHost { pos: [0.0; 3], log: Vec::new() };
    let program = parse(src).unwrap();
    let mut interp = Interpreter::new("mover.x");
    interp.load(&program, &mut host).unwrap();
    assert_eq!(interp.class_names(), ["Mover"]);
    let inst = interp.instantiate("Mover", Vec::new(), &mut host).unwrap();
    interp.call_method(&inst, "Init", Vec::new(), &mut host).unwrap();
    interp.call_method(&inst, "Update", vec![Value::Float(0.5)], &mut host).unwrap();
    assert_eq!(host.pos, [1.0, 0.0, 0.0]);
    interp.call_method(&inst, "Update", vec![Value::Float(0.5)], &mut host).unwrap();
    assert_eq!(host.pos, [0.0, 9.0, 0.0]);
    assert_eq!(host.log, ["ready"]);
    let Value::Instance(i) = &inst else { panic!() };
    assert_eq!(i.get("Ticks").unwrap().as_i64(), Some(2));
    assert!(parse("TAKE Nope").is_ok());
    let e = interp.load(&parse("TAKE Nope").unwrap(), &mut host).unwrap_err();
    assert!(e.to_string().contains("unknown module `Nope`"));
}

#[test]
fn fields_are_private_unless_public_or_edit() {
    let src = r#"
CREATE Base[
    EDIT FLOAT Speed: 2.0
    INTEGER Secret: 1
]
CREATE Hero RIDE Base[
    PUBLIC INTEGER Health: 100
    EDIT VECTOR Spawn: VECTOR(1, -2, 3)
    EDIT STRING Title: "hi"
    FLOAT Hidden: 5.0
    VOID CREATE Init()[ ]
]
"#;
    let p = crate::parse(src).unwrap();
    let fields = crate::declared_fields(&p, "hero");
    let names: Vec<(&str, crate::ast::Access)> = fields.iter().map(|f| (f.name.as_str(), f.access)).collect();
    use crate::ast::Access::*;
    assert_eq!(names, [("Speed", Edit), ("Health", Public), ("Spawn", Edit), ("Title", Edit)]);
    assert!(matches!(fields[2].default, Value::Vector([1.0, -2.0, 3.0])));
    // The running instance shows only the same fields.
    let mut host = crate::StdHost::default();
    let mut it = crate::run("t.x", src, &mut host).unwrap();
    let Value::Instance(i) = it.instantiate("Hero", vec![], &mut host).unwrap() else { panic!() };
    let live: Vec<String> = i.snapshot().into_iter().map(|f| f.0).collect();
    assert_eq!(live, ["Speed", "Health", "Spawn", "Title"]);
}

#[test]
fn public_and_edit_mistakes_are_explained() {
    let e = crate::parse("EDIT FLOAT X: 1.0").unwrap_err();
    assert!(e.message.contains("only works on class fields"), "{}", e.message);
    let e = crate::parse("CREATE A[\n    EDIT VOID CREATE Go()[ ]\n]").unwrap_err();
    assert!(e.message.contains("not a method"), "{}", e.message);
    let e = crate::parse("CREATE A[\n    EDIT INDEX Items: []\n]").unwrap_err();
    assert!(e.message.contains("use PUBLIC"), "{}", e.message);
    assert!(crate::parse("CREATE A[\n    PUBLIC INDEX Items: []\n]").is_ok());
}

#[test]
fn invoke_goes_to_the_host() {
    struct Spawner(Vec<(Vec<Value>, bool)>);
    impl Host for Spawner {
        fn has_module(&self, _: &str) -> bool {
            false
        }
        fn call_module(&mut self, _: &str, _: &str, _: &[Value]) -> Result<Value, String> {
            Err("no modules".into())
        }
        fn display(&mut self, _: &str) {}
        fn invoke(&mut self, args: &[Value], inside: bool) -> Result<Value, String> {
            self.0.push((args.to_vec(), inside));
            Ok(Value::Int(self.0.len() as i64))
        }
    }
    let mut host = Spawner(Vec::new());
    run(
        "test.x",
        r#"
ANY Rock: INVOKE("Sphere", VECTOR(1, 2, 3))
INVOKE("Cube")
INSTANTIATE("Torus", VECTOR(0, 0, 0))
# `Invoke` (not all caps) is still an ordinary name.
INTEGER Invoke: Rock + 1
CREATE Field[
    VOID CREATE Init()[
        INVOKE("Rock", VECTOR(0, 0, 0), VECTOR(0, 90, 0), 2)
    ]
]
Field F: NEW Field()
F.Init()
"#,
        &mut host,
    )
    .unwrap();
    assert_eq!(host.0.len(), 4);
    assert!(matches!(host.0[0].0[..], [Value::Str(ref s), Value::Vector([1.0, 2.0, 3.0])] if &**s == "Sphere"));
    // INVOKE spawns inside the calling object; INSTANTIATE at the top level.
    assert_eq!(host.0.iter().map(|c| c.1).collect::<Vec<_>>(), [true, true, false, true]);
    assert_eq!(host.0[3].0.len(), 4);

    // Without a game engine, INVOKE explains itself; bare INVOKE asks for parentheses.
    assert!(err(r#"INVOKE("Sphere")"#).contains("run this script in Kit"));
    assert!(
        err(r#"INSTANTIATE("Sphere")"#).starts_with("test.x:1: INSTANTIATE:")
            || err(r#"INSTANTIATE("Sphere")"#).contains("INSTANTIATE: spawning")
    );
    assert!(err("ANY X: INVOKE").contains("INVOKE needs what to spawn"));
    assert!(err("ANY X: INSTANTIATE").contains("INSTANTIATE needs what to spawn"));
}
