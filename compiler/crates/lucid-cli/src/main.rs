use std::env;
use std::fs;
use std::path::{Path, PathBuf};
use std::process::exit;

fn main() {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        start_repl();
        return;
    }

    match args[1].as_str() {
        "repl" => {
            start_repl();
        }
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
            let verbose = args.iter().any(|a| a == "--verbose" || a == "-v");
            let docs_path = args.iter().skip(2).find(|a| !a.starts_with('-')).map(PathBuf::from).unwrap_or_else(|| PathBuf::from("docs"));
            test_spec_docs(&docs_path, verbose);
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
    println!("    lucid [COMMAND] [OPTIONS]");
    println!();
    println!("COMMANDS:");
    println!("    repl             Start interactive REPL (default when no arguments)");
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
    if let Ok(canon) = fs::canonicalize(path) {
        interp.set_current_file(Some(canon));
    } else {
        interp.set_current_file(Some(path.to_path_buf()));
    }
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

fn start_repl() {
    use std::io::{self, BufRead, Write};

    println!("Lucid 0.1.0 interactive REPL");
    println!("Type :help for assistance, :exit or :quit to leave.");
    println!();

    let mut checker = lucid_checker::TypeChecker::new();
    let mut interp = lucid_runtime::Interpreter::new();
    let stdin = io::stdin();
    let mut handle = stdin.lock();

    let mut buffer = String::new();
    let mut is_continuation = false;

    loop {
        if is_continuation {
            print!("... ");
        } else {
            print!(">>> ");
        }
        let _ = io::stdout().flush();

        let mut line = String::new();
        match handle.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {}
            Err(e) => {
                eprintln!("Error reading input: {e}");
                break;
            }
        }

        let trimmed = line.trim();

        if !is_continuation {
            match trimmed {
                ":exit" | ":quit" | "exit()" | "quit()" => break,
                ":help" => {
                    print_repl_help();
                    continue;
                }
                ":clear" => {
                    print!("\x1b[2J\x1b[H");
                    let _ = io::stdout().flush();
                    continue;
                }
                ":reset" => {
                    checker = lucid_checker::TypeChecker::new();
                    interp = lucid_runtime::Interpreter::new();
                    println!("Environment reset.");
                    continue;
                }
                ":vars" => {
                    println!("Variables:");
                    let env = interp.env.borrow();
                    let mut keys: Vec<_> = env.bindings.keys().collect();
                    keys.sort();
                    for k in keys {
                        if let Some(v) = env.bindings.get(k) {
                            println!("  {k} = {v:?}");
                        }
                    }
                    continue;
                }
                "" => continue,
                _ => {}
            }
        }

        buffer.push_str(&line);

        let needs_more = check_multiline_continuation(&buffer, &line, is_continuation);
        if needs_more {
            is_continuation = true;
            continue;
        }

        let code = buffer.trim().to_string();
        buffer.clear();
        is_continuation = false;

        if code.is_empty() {
            continue;
        }

        match lucid_syntax::parse(&code) {
            Ok(module) => {
                for stmt in &module.statements {
                    if let Err(te) = checker.check_statement(stmt) {
                        eprintln!("Type Error: {}", te.message);
                    }
                }

                let is_expr = module.statements.len() == 1 && matches!(module.statements[0], lucid_syntax::ast::Stmt::Expr(_));

                match interp.eval_module(&module) {
                    Ok(val) => {
                        for out_line in interp.output.drain(..) {
                            println!("{out_line}");
                        }
                        if is_expr && val != lucid_runtime::Value::None {
                            println!("{val:?}");
                        }
                    }
                    Err(re) => {
                        for out_line in interp.output.drain(..) {
                            println!("{out_line}");
                        }
                        eprintln!("Runtime Error: {}", re.message);
                    }
                }
            }
            Err(e) => {
                eprintln!("Syntax Error: {e}");
            }
        }
    }

    println!("\nGoodbye!");
}

fn check_multiline_continuation(buffer: &str, line: &str, is_continuation: bool) -> bool {
    if is_continuation && line.trim().is_empty() {
        return false;
    }

    let trimmed = line.trim();
    if trimmed.ends_with(':') || trimmed.ends_with('\\') {
        return true;
    }

    let mut paren = 0;
    let mut bracket = 0;
    let mut brace = 0;
    let mut in_single_quote = false;
    let mut in_double_quote = false;
    let mut chars = buffer.chars().peekable();

    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                if in_single_quote || in_double_quote {
                    let _ = chars.next();
                }
            }
            '\'' if !in_double_quote => in_single_quote = !in_single_quote,
            '"' if !in_single_quote => in_double_quote = !in_double_quote,
            '#' if !in_single_quote && !in_double_quote => {
                for c in chars.by_ref() {
                    if c == '\n' { break; }
                }
            }
            '(' if !in_single_quote && !in_double_quote => paren += 1,
            ')' if !in_single_quote && !in_double_quote && paren > 0 => paren -= 1,
            '[' if !in_single_quote && !in_double_quote => bracket += 1,
            ']' if !in_single_quote && !in_double_quote && bracket > 0 => bracket -= 1,
            '{' if !in_single_quote && !in_double_quote => brace += 1,
            '}' if !in_single_quote && !in_double_quote && brace > 0 => brace -= 1,
            _ => {}
        }
    }

    if paren > 0 || bracket > 0 || brace > 0 || in_single_quote || in_double_quote {
        return true;
    }

    if is_continuation && (line.starts_with(' ') || line.starts_with('\t')) {
        return true;
    }

    false
}

fn print_repl_help() {
    println!("Lucid REPL Commands:");
    println!("  :help     Show this help message");
    println!("  :vars     List active variables and their current values");
    println!("  :reset    Reset the type environment and interpreter");
    println!("  :clear    Clear the terminal screen");
    println!("  :exit     Exit the REPL (or :quit)");
    println!();
    println!("Tips:");
    println!("  - Indented blocks (def, class, if, for) continue with '... ' until a blank line.");
    println!("  - Unclosed brackets ( ), [ ], {{ }} continue on the next line automatically.");
    println!("  - Expressions are evaluated and their result printed immediately.");
}

fn test_spec_docs(docs_dir: &Path, verbose: bool) {
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
    let mut typechecked_blocks = 0;

    for rst_file in &rst_files {
        let content = match fs::read_to_string(rst_file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let blocks = extract_rst_code_blocks(&content);
        for (idx, block) in blocks.into_iter().enumerate() {
            total_blocks += 1;
            match lucid_syntax::parse(&block) {
                Ok(module) => {
                    parsed_blocks += 1;
                    let mut checker = lucid_checker::TypeChecker::new();
                    if checker.check_module(&module).is_ok() {
                        typechecked_blocks += 1;
                    } else if verbose {
                        let err = checker.check_module(&module).unwrap_err();
                        println!("! Typecheck failed in {} block #{}: {}", rst_file.display(), idx + 1, err.message);
                    }
                }
                Err(err) => {
                    if verbose {
                        println!("✗ Parse failed in {} block #{}: {}", rst_file.display(), idx + 1, err);
                        let first_line = block.lines().next().unwrap_or("").trim();
                        println!("    Snippet: {first_line}");
                    }
                }
            }
        }
    }

    println!("Specification validation complete:");
    println!("  RST files scanned: {}", rst_files.len());
    println!("  Code blocks found: {total_blocks}");
    println!("  Valid Lucid modules parsed: {parsed_blocks} / {total_blocks} ({:.1}%)", (parsed_blocks as f64 / total_blocks as f64) * 100.0);
    println!("  Type checked without errors: {typechecked_blocks} / {parsed_blocks}");
}

fn extract_rst_code_blocks(rst: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let lines: Vec<&str> = rst.lines().collect();
    let mut i = 0;

    while i < lines.len() {
        let line = lines[i];
        let trimmed = line.trim();

        let is_code = trimmed.starts_with(".. code-block:: python")
            || trimmed.starts_with(".. code-block:: lucid")
            || trimmed == "::"
            || (trimmed.ends_with("::") && !trimmed.starts_with(".. "));

        if is_code {
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
