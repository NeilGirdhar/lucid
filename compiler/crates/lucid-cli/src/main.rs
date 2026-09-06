use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        print_help();
        return;
    }

    match args[1].as_str() {
        "run" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument for 'run'");
                eprintln!("Usage: lucid run <file.lucid>");
                exit(1);
            }
            run_file(&args[2]);
        }
        "check" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument for 'check'");
                eprintln!("Usage: lucid check <file.lucid>");
                exit(1);
            }
            check_file(&args[2]);
        }
        "eval" => {
            if args.len() < 3 {
                eprintln!("Error: missing code argument for 'eval'");
                eprintln!("Usage: lucid eval \"<code>\"");
                exit(1);
            }
            eval_string(&args[2]);
        }
        "test-spec" => {
            let docs_path = if args.len() >= 3 {
                PathBuf::from(&args[2])
            } else {
                PathBuf::from("docs")
            };
            test_spec_docs(&docs_path);
        }
        "--help" | "-h" | "help" => {
            print_help();
        }
        "--version" | "-v" | "version" => {
            println!("lucid 0.1.0 (compiler & runtime)");
        }
        unknown => {
            eprintln!("Error: unknown command '{unknown}'");
            print_help();
            exit(1);
        }
    }
}

fn print_help() {
    println!("Lucid Language Compiler & Runtime");
    println!();
    println!("USAGE:");
    println!("    lucid <COMMAND> [OPTIONS]");
    println!();
    println!("COMMANDS:");
    println!("    run <file>       Parse, typecheck, and evaluate a Lucid source file");
    println!("    check <file>     Parse and typecheck a Lucid source file");
    println!("    eval <code>      Evaluate a Lucid code snippet string");
    println!("    test-spec [dir]  Extract and validate code snippets from RST specification docs");
    println!("    help             Display this help message");
    println!("    version          Show version information");
}

fn run_file(path_str: &str) {
    let path = Path::new(path_str);
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read file '{path_str}': {e}");
            exit(1);
        }
    };

    let module = match lucid_syntax::parse(&source) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Syntax Error: {e}");
            exit(1);
        }
    };

    let mut checker = lucid_checker::TypeChecker::new();
    if let Err(type_err) = checker.check_module(&module) {
        eprintln!("Type Error: {} at {:?}", type_err.message, type_err.span);
        exit(1);
    }

    let mut interp = lucid_runtime::Interpreter::new();
    match interp.eval_module(&module) {
        Ok(res) => {
            for line in &interp.output {
                println!("{line}");
            }
            if res != lucid_runtime::Value::None {
                println!("{res:?}");
            }
        }
        Err(err) => {
            eprintln!("Runtime Error: {} at {:?}", err.message, err.span);
            exit(1);
        }
    }
}

fn check_file(path_str: &str) {
    let path = Path::new(path_str);
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read file '{path_str}': {e}");
            exit(1);
        }
    };

    let module = match lucid_syntax::parse(&source) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Syntax Error: {e}");
            exit(1);
        }
    };

    let mut checker = lucid_checker::TypeChecker::new();
    match checker.check_module(&module) {
        Ok(()) => {
            println!("✓ Type check passed: no errors found in {path_str}");
        }
        Err(type_err) => {
            eprintln!("Type Error: {} at {:?}", type_err.message, type_err.span);
            exit(1);
        }
    }
}

fn eval_string(source: &str) {
    let module = match lucid_syntax::parse(source) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("Syntax Error: {e}");
            exit(1);
        }
    };

    let mut checker = lucid_checker::TypeChecker::new();
    if let Err(type_err) = checker.check_module(&module) {
        eprintln!("Type Error: {} at {:?}", type_err.message, type_err.span);
        exit(1);
    }

    let mut interp = lucid_runtime::Interpreter::new();
    match interp.eval_module(&module) {
        Ok(res) => {
            for line in &interp.output {
                println!("{line}");
            }
            if res != lucid_runtime::Value::None {
                println!("{res:?}");
            }
        }
        Err(err) => {
            eprintln!("Runtime Error: {} at {:?}", err.message, err.span);
            exit(1);
        }
    }
}

fn test_spec_docs(docs_dir: &Path) {
    println!("Testing Lucid specification code snippets from: {}", docs_dir.display());
    let mut rst_files = Vec::new();
    if Path::new("README.rst").exists() {
        rst_files.push(PathBuf::from("README.rst"));
    }
    if docs_dir.exists() {
        if let Ok(entries) = fs::read_dir(docs_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().map_or(false, |ext| ext == "rst") {
                    rst_files.push(p);
                }
            }
        }
    }
    rst_files.sort();

    let mut total_blocks = 0;
    let mut parsed_blocks = 0;

    for rst_file in &rst_files {
        let content = match fs::read_to_string(rst_file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let blocks = extract_rst_code_blocks(&content);
        for block in blocks {
            total_blocks += 1;
            // Attempt parse
            if lucid_syntax::parse(&block).is_ok() {
                parsed_blocks += 1;
            }
        }
    }

    println!("Specification validation complete:");
    println!("  RST files scanned: {}", rst_files.len());
    println!("  Code blocks found: {total_blocks}");
    println!("  Valid Lucid modules parsed: {parsed_blocks}");
}

fn extract_rst_code_blocks(rst: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = rst.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        if trimmed.starts_with(".. code-block:: python")
            || trimmed.starts_with(".. code-block:: lucid")
            || trimmed == "::"
            || trimmed.ends_with("::")
        {
            let mut block_lines = Vec::new();
            i += 1;
            // Skip empty lines
            while i < lines.len() && lines[i].trim().is_empty() {
                i += 1;
            }
            if i >= lines.len() {
                break;
            }
            // Determine indentation of the block
            let base_indent = lines[i].chars().take_while(|c| *c == ' ').count();
            if base_indent > 0 {
                while i < lines.len() {
                    let cur = lines[i];
                    if cur.trim().is_empty() {
                        block_lines.push("");
                        i += 1;
                        continue;
                    }
                    let indent = cur.chars().take_while(|c| *c == ' ').count();
                    if indent < base_indent {
                        break;
                    }
                    let unindented = if cur.len() >= base_indent {
                        &cur[base_indent..]
                    } else {
                        cur.trim_start()
                    };
                    block_lines.push(unindented);
                    i += 1;
                }
            }
            let block = block_lines.join("\n");
            if !block.trim().is_empty() {
                blocks.push(block);
            }
        } else {
            i += 1;
        }
    }

    blocks
}
