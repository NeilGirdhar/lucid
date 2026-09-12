use lucid_syntax::ast::{Module, Stmt};
use std::collections::HashSet;
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
        "build" => {
            let file_arg = args.iter().skip(2).find(|a| !a.starts_with('-'));
            let out_arg = args
                .iter()
                .position(|a| a == "-o")
                .and_then(|idx| args.get(idx + 1));
            build_file(file_arg, out_arg, 3);
        }
        "emit-c" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument for 'emit-c'");
                exit(1);
            }
            emit_c_file(&args[2]);
        }
        "emit-cir" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument for 'emit-cir'");
                exit(1);
            }
            let function = args
                .iter()
                .position(|arg| arg == "--function")
                .and_then(|index| args.get(index + 1));
            emit_cir_file(&args[2], function.map(String::as_str));
        }
        "run-cir" => {
            if args.len() < 3 {
                eprintln!("Error: missing file argument for 'run-cir'");
                exit(1);
            }
            let function = args
                .iter()
                .position(|arg| arg == "--function")
                .and_then(|index| args.get(index + 1));
            let values = match args
                .iter()
                .position(|arg| arg == "--args")
                .and_then(|index| args.get(index + 1))
            {
                Some(raw) => match raw
                    .split(',')
                    .map(str::trim)
                    .map(str::parse::<i64>)
                    .collect::<Result<Vec<_>, _>>()
                {
                    Ok(values) => values,
                    Err(_) => {
                        eprintln!("Error: --args must be a comma-separated list of integers");
                        exit(1);
                    }
                },
                None => Vec::new(),
            };
            let step_limit = match args
                .iter()
                .position(|arg| arg == "--step-limit")
                .and_then(|index| args.get(index + 1))
            {
                Some(raw) => match raw.parse::<usize>() {
                    Ok(limit) if limit > 0 => Some(limit),
                    _ => {
                        eprintln!("Error: --step-limit must be a positive integer");
                        exit(1);
                    }
                },
                None => None,
            };
            run_cir_file(&args[2], function.map(String::as_str), &values, step_limit);
        }
        "run" => {
            let native = args
                .iter()
                .any(|a| a == "--native" || a == "-n" || a == "--release");
            let file_arg = args.iter().skip(2).find(|a| !a.starts_with('-'));
            let entry_positions = args
                .iter()
                .enumerate()
                .filter_map(|(index, arg)| (arg == "--entry").then_some(index))
                .collect::<Vec<_>>();
            if entry_positions.len() > 1 {
                eprintln!("Error: --entry may be specified only once");
                exit(1);
            }
            let entry = match entry_positions.first().copied() {
                Some(index) => match args.get(index + 1).filter(|value| !value.starts_with('-')) {
                    Some(value) => Some(value.as_str()),
                    None => {
                        eprintln!("Error: --entry requires a function name");
                        exit(1);
                    }
                },
                None => None,
            };
            if let Some(f) = file_arg {
                if native {
                    run_native(f, entry);
                } else {
                    run_file(f, entry);
                }
            } else {
                eprintln!("Error: missing file argument for 'run'");
                eprintln!("Usage: lucid run <file.lucid> [--native]");
                exit(1);
            }
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
            let docs_path = args
                .iter()
                .skip(2)
                .find(|a| !a.starts_with('-'))
                .map(PathBuf::from)
                .unwrap_or_else(|| PathBuf::from("docs"));
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
    println!("    repl                  Start interactive REPL (default when no arguments)");
    println!("    build <file> [-o bin] Compile source file to an optimized native binary");
    println!(
        "    run <file> [--native] [--entry NAME] Run a Lucid source file or named interpreted entry"
    );
    println!("    check <file>          Parse and typecheck a Lucid source file");
    println!("    emit-c <file>         Emit generated C99 code for a Lucid source file");
    println!("    emit-cir <file> [--function NAME]  Emit validated CIR");
    println!(
        "    run-cir <file> [--function NAME] [--args A,B] [--step-limit N]  Execute validated CIR"
    );
    println!("    eval <code>           Evaluate a Lucid code snippet string");
    println!(
        "    test-spec [dir]       Extract and validate code snippets from Markdown specification docs"
    );
    println!("    help                  Display this help message");
    println!("    version               Show version information");
}

/// Load the entry module and its local source imports in dependency order for
/// the native backend. Built-in modules (currently `math` and `sys`) are
/// handled directly by code generation and do not have source files.
fn load_native_project(entry: &Path) -> Result<Module, String> {
    fn resolve_import(base_file: &Path, module: &str) -> Option<PathBuf> {
        if matches!(module, "math" | "sys" | "iteration") {
            return None;
        }
        let mut parent = base_file.parent()?.to_path_buf();
        let mut name = module;
        let mut dots = 0;
        while name.starts_with('.') {
            dots += 1;
            name = &name[1..];
        }
        for _ in 1..dots {
            parent = parent.parent()?.to_path_buf();
        }
        let relative = name.replace('.', "/");
        let candidate = parent.join(format!("{relative}.lucid"));
        if candidate.is_file() {
            return Some(candidate);
        }
        let package = parent.join(relative).join("__init__.lucid");
        package.is_file().then_some(package)
    }

    fn declaration_only(path: &Path) -> bool {
        let Ok(source) = fs::read_to_string(path) else {
            return false;
        };
        let Ok(module) = lucid_syntax::parse(&source) else {
            return false;
        };
        fn is_declaration(statement: &Stmt) -> bool {
            match statement {
                Stmt::Export(inner) => is_declaration(inner),
                Stmt::ClassDef { .. }
                | Stmt::InterfaceDef { .. }
                | Stmt::TraitDef { .. }
                | Stmt::ImplementDef { .. }
                | Stmt::TypeAlias { .. }
                | Stmt::Function(_)
                | Stmt::Import { .. }
                | Stmt::FromImport { .. }
                | Stmt::Pass(_)
                | Stmt::Break(_)
                | Stmt::Continue(_)
                | Stmt::VarDef { value: None, .. } => true,
                _ => false,
            }
        }
        module.statements.iter().all(is_declaration)
    }

    fn visit(
        path: &Path,
        visited: &mut HashSet<PathBuf>,
        active: &mut Vec<PathBuf>,
    ) -> Result<Vec<Stmt>, String> {
        let canonical = fs::canonicalize(path)
            .map_err(|e| format!("Error: failed to resolve module '{}': {e}", path.display()))?;
        if let Some(index) = active.iter().position(|item| item == &canonical) {
            let mut cycle = active[index..]
                .iter()
                .map(|item| item.display().to_string())
                .collect::<Vec<_>>();
            cycle.push(canonical.display().to_string());
            if active[index..].iter().all(|item| declaration_only(item)) {
                // Declaration names are collected before module execution;
                // the recursive edge contributes no executable statements.
                return Ok(Vec::new());
            }
            return Err(format!(
                "Import Error: cyclic local imports: {}",
                cycle.join(" -> ")
            ));
        }
        if !visited.insert(canonical.clone()) {
            return Ok(Vec::new());
        }
        active.push(canonical.clone());
        let source = fs::read_to_string(&canonical).map_err(|e| {
            format!(
                "Error: failed to read module '{}': {e}",
                canonical.display()
            )
        })?;
        let module = lucid_syntax::parse(&source)
            .map_err(|e| format!("Syntax Error in '{}': {e}", canonical.display()))?;
        let mut statements = Vec::new();
        for statement in &module.statements {
            if let Stmt::FromImport {
                module,
                names,
                span,
                ..
            } = statement
                && let Some((name, _)) = names.iter().find(|(name, _)| name.starts_with('_'))
            {
                return Err(format!(
                    "Import Error: cannot import private name '{}' from module '{}' at {}:{}",
                    name, module, span.line, span.column
                ));
            }
            let imported = match statement {
                Stmt::Import { module, .. } => Some(module),
                Stmt::FromImport { module, .. } => Some(module),
                _ => None,
            };
            if let Some(module_name) = imported {
                if let Some(import_path) = resolve_import(&canonical, module_name) {
                    statements.extend(visit(&import_path, visited, active)?);
                } else if !matches!(module_name.as_str(), "math" | "sys" | "iteration") {
                    return Err(format!(
                        "Import Error: cannot resolve local module '{}' imported by '{}'",
                        module_name,
                        canonical.display()
                    ));
                }
            }
        }
        statements.extend(
            module
                .statements
                .into_iter()
                .filter_map(|statement| match statement {
                    Stmt::FromImport { .. } => None,
                    Stmt::Export(inner) => Some(*inner),
                    other => Some(other),
                }),
        );
        active.pop();
        Ok(statements)
    }

    let statements = visit(entry, &mut HashSet::new(), &mut Vec::new())?;
    Ok(Module {
        statements,
        span: Default::default(),
    })
}

fn run_file(path_str: &str, entry: Option<&str>) {
    let path = Path::new(path_str);
    let project = load_project_manifest(path);
    let resolved_entry = resolve_entry_target_from_project(project.as_ref(), entry);
    let library_context = project
        .as_ref()
        .and_then(|config| config.library_context.as_deref());
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read file '{path_str}': {e}");
            exit(1);
        }
    };

    let mut database = lucid_db::CompilerDatabase::default();
    let file = database.add_file(path_str.to_string(), source.clone());
    let diagnostics = lucid_db::file_diagnostics(&database, file);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == lucid_db::Severity::Error)
    {
        for diagnostic in diagnostics.iter() {
            eprintln!(
                "{}:{}:{}: {}: {}",
                path_str,
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.code,
                diagnostic.message
            );
        }
        exit(1);
    }
    let module = match lucid_db::parse_ast(&database, file).as_ref() {
        Ok(module) => module.as_ref().clone(),
        Err(error) => {
            eprintln!("Syntax Error: {error}");
            exit(1);
        }
    };
    if let Err(error) = lucid_db::type_check_file(&database, file) {
        eprintln!("Type Error: {error}");
        exit(1);
    }

    let mut interp = lucid_runtime::Interpreter::new();
    if let Ok(canon) = fs::canonicalize(path) {
        interp.set_current_file(Some(canon));
    } else {
        interp.set_current_file(Some(path.to_path_buf()));
    }
    let result = match interp.eval_module(&module) {
        Ok(module_value) => {
            // Manifest targets are module paths, not merely aliases for names
            // already imported by the entry source. Load each missing target
            // root lazily after module initialization, so direct function
            // targets remain local while `.setup.initialize` can name a module
            // that the source never imports explicitly.
            for target in [library_context, resolved_entry.as_deref()]
                .into_iter()
                .flatten()
            {
                let root = target
                    .trim_start_matches('.')
                    .split('.')
                    .next()
                    .unwrap_or_default();
                if root.is_empty() || interp.env.borrow().get(root).is_some() {
                    continue;
                }
                let import = match lucid_syntax::parse(&format!("import {root}\n")) {
                    Ok(module) => module,
                    Err(error) => {
                        eprintln!("Syntax Error: {error}");
                        exit(1);
                    }
                };
                if let Err(error) = interp.eval_module(&import) {
                    eprintln!("Runtime Error: {} at {:?}", error.message, error.span);
                    exit(1);
                }
            }
            match resolved_entry.as_deref() {
                Some(entry) => {
                    if let Some(context) = library_context {
                        interp.call_named_with_context(context, entry, &[])
                    } else {
                        interp.call_named(entry, &[])
                    }
                }
                None => Ok(module_value),
            }
        }
        Err(error) => Err(error),
    };
    match result {
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

fn build_file(path_str: Option<&String>, output_path_str: Option<&String>, opt_level: usize) {
    let path_str = match path_str {
        Some(s) => s,
        None => {
            eprintln!("Error: missing file argument for 'build'");
            eprintln!("Usage: lucid build <file.lucid> [-o <binary>]");
            exit(1);
        }
    };
    let path = Path::new(path_str);
    validate_file_with_database(path);
    let module = match load_native_project(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    };

    let mut checker = lucid_checker::TypeChecker::new();
    if let Err(type_err) = checker.check_module(&module) {
        eprintln!("Type Error: {} at {:?}", type_err.message, type_err.span);
        exit(1);
    }

    let default_out = path
        .file_stem()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    let out_bin_str = output_path_str.cloned().unwrap_or(default_out);
    let out_path = Path::new(&out_bin_str);

    if let Err(err) = lucid_codegen::compile_to_native(&module, out_path, opt_level) {
        eprintln!("Compilation Error: {}", err.message);
        exit(1);
    }

    println!("✓ Successfully compiled {path_str} to native binary '{out_bin_str}'");
}

fn emit_c_file(path_str: &str) {
    let path = Path::new(path_str);
    let module = match load_native_project(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    };

    let mut checker = lucid_checker::TypeChecker::new();
    if let Err(type_err) = checker.check_module(&module) {
        eprintln!("Type Error: {} at {:?}", type_err.message, type_err.span);
        exit(1);
    }

    let mut generator = lucid_codegen::CCodeGenerator::new();
    match generator.generate(&module) {
        Ok(code) => println!("{code}"),
        Err(err) => {
            eprintln!("Codegen Error: {}", err.message);
            exit(1);
        }
    }
}

fn emit_cir_file(path_str: &str, function_name: Option<&str>) {
    let path = Path::new(path_str);
    validate_project_manifest(path);
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("Error reading {path_str}: {error}");
            exit(1);
        }
    };
    let database = lucid_db::CompilerDatabase::default();
    let file = lucid_db::SourceFile::new(&database, source, path_str.to_string());
    let lowered = function_name.map_or_else(
        || lucid_db::lower_module(&database, file),
        |name| lucid_db::lower_function_body(&database, file, name.to_string()),
    );
    match lowered {
        Ok(function) => print!("{}", function.to_text()),
        Err(error) => {
            eprintln!("CIR lowering error: {error}");
            exit(1);
        }
    }
}

fn run_cir_file(
    path_str: &str,
    function_name: Option<&str>,
    arguments: &[i64],
    step_limit: Option<usize>,
) {
    let path = Path::new(path_str);
    validate_project_manifest(path);
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("Error reading {path_str}: {error}");
            exit(1);
        }
    };
    let database = lucid_db::CompilerDatabase::default();
    let file = lucid_db::SourceFile::new(&database, source, path_str.to_string());
    if let Err(error) = lucid_db::typed_module(&database, file) {
        eprintln!("Type error: {error}");
        exit(1);
    }
    if let Some(name) = function_name {
        let typed = lucid_db::typed_module(&database, file);
        let parameter_count = typed.as_ref().ok().and_then(|module| {
            module
                .functions
                .iter()
                .find(|function| function.symbol.name(&database).as_str() == name)
                .map(|function| function.parameter_names.len())
        });
        if let Some(expected) = parameter_count {
            if expected != arguments.len() {
                eprintln!(
                    "run-cir: function '{name}' expects {expected} integer arguments, got {}",
                    arguments.len()
                );
                exit(1);
            }
        } else {
            eprintln!("run-cir: function '{name}' not found");
            exit(1);
        }
    } else if !arguments.is_empty() {
        eprintln!("run-cir: --args requires --function NAME");
        exit(1);
    }
    let lowered = function_name.map_or_else(
        || lucid_db::lower_module(&database, file),
        |name| lucid_db::lower_function_body(&database, file, name.to_string()),
    );
    let function = match lowered {
        Ok(function) => function,
        Err(error) => {
            eprintln!("CIR lowering error: {error}");
            exit(1);
        }
    };
    if let Some(limit) = step_limit {
        let result = lucid_codegen::native_abi::execute_cir_with_args_and_step_limit(
            function.as_ref(),
            arguments,
            limit,
        );
        if result.is_ok() {
            println!("{}", result.value);
        } else {
            eprintln!("CIR execution error: {}", result.error);
            exit(1);
        }
        return;
    }
    let has_recoverable_operations = function.has_recoverable_operations();
    let result_compile =
        lucid_codegen::cranelift_backend::compile_integer_result_function(function.as_ref());
    if has_recoverable_operations && result_compile.is_err() {
        eprintln!(
            "CIR lowering error: recoverable operations require the result ABI: {:?}",
            result_compile.as_ref().err()
        );
        exit(1);
    }
    let result_compiled = result_compile.ok();
    let compiled = if result_compiled.is_none() {
        Some(
            match lucid_codegen::cranelift_backend::compile_integer_function(function.as_ref()) {
                Ok(compiled) => compiled,
                Err(error) => {
                    eprintln!("CIR lowering error: {error:?}");
                    exit(1);
                }
            },
        )
    } else {
        None
    };
    if let Some(compiled) = result_compiled {
        let result = if function_name.is_some() {
            unsafe { compiled.call_result_with_args(arguments) }
        } else {
            unsafe { compiled.call_result() }
        };
        if result.is_ok() {
            println!("{}", result.value);
        } else {
            eprintln!("CIR execution error: {}", result.error);
            exit(1);
        }
        return;
    }
    let Some(compiled) = compiled else {
        eprintln!("CIR lowering error: no compatible native entry point");
        exit(1);
    };
    if !compiled.returns_value() {
        if function_name.is_some() {
            unsafe { compiled.call_void_with_args(arguments) };
        } else {
            unsafe { compiled.call_void() };
        }
        return;
    }
    let value = if function_name.is_some() {
        unsafe { compiled.call_with_args(arguments) }
    } else {
        unsafe { compiled.call() }
    };
    println!("{value}");
}

fn run_native(path_str: &str, entry: Option<&str>) {
    let path = Path::new(path_str);
    if load_project_manifest(path)
        .as_ref()
        .and_then(|config| config.library_context.as_ref())
        .is_some()
    {
        eprintln!("Compilation Error: manifest library-context requires interpreted execution");
        exit(1);
    }
    let resolved_entry = resolve_entry_target(path, entry);
    validate_file_with_database(path);
    let module = match load_native_project(path) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            exit(1);
        }
    };

    let mut checker = lucid_checker::TypeChecker::new();
    if let Err(type_err) = checker.check_module(&module) {
        eprintln!("Type Error: {} at {:?}", type_err.message, type_err.span);
        exit(1);
    }

    let temp_dir = env::temp_dir();
    let temp_bin = temp_dir.join(format!("lucid_bin_{}", std::process::id()));

    if let Err(err) =
        lucid_codegen::compile_to_native_entry(&module, &temp_bin, 3, resolved_entry.as_deref())
    {
        eprintln!("Compilation Error: {}", err.message);
        exit(1);
    }

    let status = std::process::Command::new(&temp_bin).status();
    let _ = fs::remove_file(&temp_bin);

    match status {
        Ok(s) => {
            if !s.success() {
                exit(s.code().unwrap_or(1));
            }
        }
        Err(e) => {
            eprintln!("Error executing native binary: {e}");
            exit(1);
        }
    }
}

fn check_file(path_str: &str) {
    let path = Path::new(path_str);
    validate_project_manifest(path);
    let source = match fs::read_to_string(path) {
        Ok(s) => s,
        Err(e) => {
            eprintln!("Error: failed to read file '{path_str}': {e}");
            exit(1);
        }
    };

    let mut database = lucid_db::CompilerDatabase::default();
    let file = database.add_file(path_str.to_string(), source);
    let diagnostics = lucid_db::file_diagnostics(&database, file);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == lucid_db::Severity::Error)
    {
        for diagnostic in diagnostics.iter() {
            eprintln!(
                "{}:{}:{}: {}: {}",
                path_str,
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.code,
                diagnostic.message
            );
        }
        exit(1);
    }
    println!("✓ Type check passed: no errors found in {path_str}");
}

fn validate_file_with_database(path: &Path) {
    validate_project_manifest(path);
    let source = match fs::read_to_string(path) {
        Ok(source) => source,
        Err(error) => {
            eprintln!("Error: failed to read file '{}': {error}", path.display());
            exit(1);
        }
    };
    let mut database = lucid_db::CompilerDatabase::default();
    let file = database.add_file(path.to_string_lossy().into_owned(), source);
    let diagnostics = lucid_db::file_diagnostics(&database, file);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == lucid_db::Severity::Error)
    {
        for diagnostic in diagnostics.iter() {
            eprintln!(
                "{}:{}:{}: {}: {}",
                path.display(),
                diagnostic.span.line,
                diagnostic.span.column,
                diagnostic.code,
                diagnostic.message
            );
        }
        exit(1);
    }
}

fn load_project_manifest(path: &Path) -> Option<lucid_config::ProjectConfig> {
    let project_file = lucid_config::find_project_manifest(path)?;
    match lucid_config::load(&project_file) {
        Ok(config) => Some(config),
        Err(error) => {
            eprintln!(
                "Project configuration error in '{}': {error:?}",
                project_file.display()
            );
            exit(1);
        }
    }
}

fn resolve_entry_target(path: &Path, entry: Option<&str>) -> Option<String> {
    let project = load_project_manifest(path);
    resolve_entry_target_from_project(project.as_ref(), entry)
}

fn resolve_entry_target_from_project(
    project: Option<&lucid_config::ProjectConfig>,
    entry: Option<&str>,
) -> Option<String> {
    match (entry, project.as_ref()) {
        (Some(name), Some(config)) if !config.entry_points.is_empty() => {
            match config.entry_point(name) {
                Ok(target) => Some(target.to_owned()),
                Err(error) => {
                    eprintln!("Project entry-point error: {error}");
                    exit(1);
                }
            }
        }
        (Some(name), _) => Some(name.to_owned()),
        (None, _) => None,
    }
}

fn validate_project_manifest(path: &Path) {
    let _ = load_project_manifest(path);
}

fn eval_string(source: &str) {
    let mut database = lucid_db::CompilerDatabase::default();
    let file = database.add_file("<eval>".to_string(), source.to_string());
    let diagnostics = lucid_db::file_diagnostics(&database, file);
    if diagnostics
        .iter()
        .any(|diagnostic| diagnostic.severity == lucid_db::Severity::Error)
    {
        for diagnostic in diagnostics.iter() {
            eprintln!(
                "<eval>:{}:{}: {}: {}",
                diagnostic.span.line, diagnostic.span.column, diagnostic.code, diagnostic.message
            );
        }
        exit(1);
    }
    let module = match lucid_db::parse_ast(&database, file).as_ref() {
        Ok(module) => module.as_ref().clone(),
        Err(error) => {
            eprintln!("Syntax Error: {error}");
            exit(1);
        }
    };
    if let Err(error) = lucid_db::type_check_file(&database, file) {
        eprintln!("Type Error: {error}");
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

                let is_expr = module.statements.len() == 1
                    && matches!(module.statements[0], lucid_syntax::ast::Stmt::Expr(_));

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
                    if c == '\n' {
                        break;
                    }
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
    println!(
        "Testing Lucid specification code snippets from: {}",
        docs_dir.display()
    );
    let mut doc_files = Vec::new();
    for readme in ["README.md", "README.rst"] {
        if Path::new(readme).exists() {
            doc_files.push(PathBuf::from(readme));
        }
    }
    if docs_dir.exists()
        && let Ok(entries) = fs::read_dir(docs_dir)
    {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.extension().is_some_and(|ext| ext == "rst" || ext == "md") {
                doc_files.push(p);
            }
        }
    }
    doc_files.sort();

    let mut total_blocks = 0;
    let mut parsed_blocks = 0;
    let mut positive_parsed_blocks = 0;
    let mut typechecked_blocks = 0;
    let mut expected_failure_blocks = 0;
    let mut expected_failures_matched = 0;

    for doc_file in &doc_files {
        let content = match fs::read_to_string(doc_file) {
            Ok(c) => c,
            Err(_) => continue,
        };

        let blocks = if doc_file.extension().is_some_and(|ext| ext == "md") {
            extract_markdown_code_blocks(&content)
        } else {
            extract_rst_code_blocks(&content)
        };
        for (idx, block) in blocks.into_iter().enumerate() {
            total_blocks += 1;
            let expect_failure = spec_block_expects_failure(doc_file, &block);
            if expect_failure {
                expected_failure_blocks += 1;
            }
            match lucid_syntax::parse(&block) {
                Ok(module) => {
                    parsed_blocks += 1;
                    if !expect_failure {
                        positive_parsed_blocks += 1;
                    }
                    let mut checker = lucid_checker::TypeChecker::new();
                    match checker.check_module(&module) {
                        Ok(()) if expect_failure => {
                            if verbose {
                                println!(
                                    "! Expected failure passed in {} block #{}",
                                    doc_file.display(),
                                    idx + 1,
                                );
                            }
                        }
                        Ok(()) => {
                            typechecked_blocks += 1;
                        }
                        Err(_) if expect_failure => {
                            expected_failures_matched += 1;
                        }
                        Err(err) if verbose => {
                            println!(
                                "! Typecheck failed in {} block #{}: {}",
                                doc_file.display(),
                                idx + 1,
                                err.message
                            );
                        }
                        Err(_) => {}
                    }
                }
                Err(_) if expect_failure => {
                    expected_failures_matched += 1;
                }
                Err(err) => {
                    if verbose {
                        println!(
                            "✗ Parse failed in {} block #{}: {}",
                            doc_file.display(),
                            idx + 1,
                            err
                        );
                        let first_line = block.lines().next().unwrap_or("").trim();
                        println!("    Snippet: {first_line}");
                    }
                }
            }
        }
    }

    println!("Specification validation complete:");
    println!("  Documentation files scanned: {}", doc_files.len());
    println!("  Code blocks found: {total_blocks}");
    let parse_percent = if total_blocks == 0 {
        100.0
    } else {
        (parsed_blocks as f64 / total_blocks as f64) * 100.0
    };
    println!(
        "  Valid Lucid modules parsed: {parsed_blocks} / {total_blocks} ({parse_percent:.1}%)"
    );
    let check_percent = if positive_parsed_blocks == 0 {
        100.0
    } else {
        (typechecked_blocks as f64 / positive_parsed_blocks as f64) * 100.0
    };
    println!(
        "  Positive snippets type checked: {typechecked_blocks} / {positive_parsed_blocks} ({check_percent:.1}%)"
    );
    if expected_failure_blocks > 0 {
        println!(
            "  Expected failures accepted: {expected_failures_matched} / {expected_failure_blocks}"
        );
    }
}

fn spec_block_expects_failure(path: &Path, block: &str) -> bool {
    let lower_path = path.to_string_lossy().to_ascii_lowercase();
    if lower_path.contains("rejected-features") {
        return !(block.contains("class FileHandle:")
            || block.contains("contextmanager def transaction"));
    }
    let lower_block = block.to_ascii_lowercase();
    if lower_block.contains("\n    global ")
        || lower_block.contains("\n        nonlocal ")
        || lower_block.contains("metaclass=")
    {
        return true;
    }
    block.lines().any(|line| {
        let line = line.to_ascii_lowercase();
        let comment = line.split_once('#').map(|(_, comment)| comment.trim());
        comment.is_some_and(|comment| {
            comment.contains("error")
                || comment.contains("not part of lucid")
                || comment.contains("discarded in lucid")
                || comment.contains("not supported")
        })
    })
}

fn extract_markdown_code_blocks(markdown: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current: Option<Vec<&str>> = None;
    for line in markdown.lines() {
        if let Some(rest) = line.strip_prefix("```") {
            if current.is_some() {
                if let Some(lines) = current.take()
                    && !lines.is_empty()
                {
                    blocks.push(lines.join("\n"));
                }
            } else if matches!(rest.trim(), "lucid" | "python" | "py") {
                current = Some(Vec::new());
            }
        } else if let Some(lines) = current.as_mut() {
            lines.push(line);
        }
    }
    blocks
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

#[cfg(test)]
mod tests {
    use super::extract_markdown_code_blocks;

    #[test]
    fn markdown_extraction_handles_empty_and_unclosed_blocks() {
        assert!(extract_markdown_code_blocks("```lucid\n```").is_empty());
        assert!(extract_markdown_code_blocks("```lucid\nanswer = 42\n").is_empty());
    }
}
