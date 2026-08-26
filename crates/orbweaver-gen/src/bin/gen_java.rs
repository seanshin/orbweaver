//! `gen-java` — write a Java client source tree for each IDL file.
//!
//! ```text
//! gen-java --out <dir> [--package <name>] [-I <dir>]... <file.idl>...
//! ```
//!
//! One root package per file, named after the file's stem unless `--package`
//! says otherwise, with `_Rt.java` and `_Types.java` inside it. The output
//! directory is what a consumer puts on `javac`'s source path; nothing else is
//! needed, because a generated tree imports only its own runtime and
//! `java.base`. In particular it does **not** need an `org.omg.CORBA`: JDK 11
//! removed one (JEP 320) and the only one on a machine like this is JacORB's
//! jar, which is an LGPL **fixture, never a dependency**.
//!
//! Each file is read as a **translation unit**: what it includes is part of what
//! it says, and `-I` adds a directory to resolve `#include` against as
//! `sidl-validate` does. A class generated from a file read as a string has no
//! method for an operation an included header declares, so the client cannot
//! make a call the peer would have answered.
//!
//! Skips are printed with their reasons and are **not** failures: a deferred
//! wire type (§4.4) and a type with no AnyJSON form are decisions, and the exit
//! code is reserved for a file that would not generate at all.
//!
//! *파일마다 하나의 루트 패키지. 생성된 트리는 자기 런타임과 `java.base`만 쓴다 —
//! JacORB는 픽스처이지 의존성이 아니다.*

use std::path::Path;

use orbweaver_gen::java::emit_java;
use orbweaver_registry::{Contract, Registry, Strictness, take_include_dirs};

fn main() -> std::process::ExitCode {
    let mut out_dir: Option<String> = None;
    let mut package_arg: Option<String> = None;
    let mut files: Vec<String> = Vec::new();
    let mut argv: Vec<String> = std::env::args().skip(1).collect();
    let search = match take_include_dirs(&mut argv) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("{e}");
            return std::process::ExitCode::from(2);
        }
    };
    let mut args = argv.into_iter();
    while let Some(a) = args.next() {
        match a.as_str() {
            "--out" => out_dir = args.next(),
            "--package" => package_arg = args.next(),
            other => files.push(other.to_owned()),
        }
    }
    let (Some(out_dir), false) = (out_dir, files.is_empty()) else {
        eprintln!(
            "usage: gen-java --out <dir> [--package <name>] [-I <dir>]... <file.idl>..."
        );
        return std::process::ExitCode::from(2);
    };
    if package_arg.is_some() && files.len() > 1 {
        eprintln!(
            "gen-java: --package names one root package and {} files were given; \
             two trees under one package would overwrite each other's `_Types.java`",
            files.len()
        );
        return std::process::ExitCode::from(2);
    }

    let mut failed = 0usize;
    let mut emitted = 0usize;
    for path in &files {
        let stem = Path::new(path)
            .file_stem()
            .map(|s| s.to_string_lossy().replace(['-', '.'], "_"))
            .unwrap_or_default();
        // A Java package segment may not start with a digit, and the corpus is
        // numbered.
        let package = package_arg.clone().unwrap_or_else(|| {
            if stem.starts_with(|c: char| c.is_ascii_digit()) { format!("g{stem}") } else { stem }
        });

        if let Err(e) = std::fs::File::open(path) {
            eprintln!("{path}: {e}");
            return std::process::ExitCode::from(2);
        }
        let contract = match Contract::load(Path::new(path), &search, Strictness::Checked) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("{path}: rejected by the front end:");
                for line in e.message.lines().take(3) {
                    eprintln!("  {line}");
                }
                failed += 1;
                continue;
            }
        };
        let mut registry = Registry::new();
        if let Err(e) = registry.load(&contract.spec) {
            eprintln!("{path}: {e}");
            failed += 1;
            continue;
        }
        let generated = emit_java(&registry, &package);
        emitted += generated.emitted;
        for (name, source) in &generated.files {
            let target = Path::new(&out_dir).join(name);
            if let Some(parent) = target.parent()
                && let Err(e) = std::fs::create_dir_all(parent)
            {
                eprintln!("{}: {e}", parent.display());
                return std::process::ExitCode::from(2);
            }
            if let Err(e) = std::fs::write(&target, source) {
                eprintln!("{}: {e}", target.display());
                return std::process::ExitCode::from(2);
            }
        }
        println!("{package}: {} item(s), {} file(s)", generated.emitted, generated.files.len());
        for (id, why) in &generated.skipped {
            println!("  skipped {id}: {why}");
        }
    }

    println!("generated {emitted} item(s) from {} file(s) into {out_dir}", files.len());
    if failed > 0 {
        println!("{failed} file(s) failed");
        return std::process::ExitCode::FAILURE;
    }
    std::process::ExitCode::SUCCESS
}
