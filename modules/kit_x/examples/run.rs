//! Run an x file from the terminal: `cargo run -p kit_x --example run -- file.x`

fn main() {
    let Some(path) = std::env::args().nth(1) else {
        eprintln!("usage: run <file.x>");
        std::process::exit(2);
    };
    let src = match std::fs::read_to_string(&path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{path}: {e}");
            std::process::exit(1);
        }
    };
    let mut host = kit_x::StdHost::default();
    let result = kit_x::run(&path, &src, &mut host).and_then(|mut i| {
        if i.has_function("Main") { i.call_function("Main", Vec::new(), &mut host).map(|_| ()) } else { Ok(()) }
    });
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}
