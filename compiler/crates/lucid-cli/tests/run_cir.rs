use std::fs;
use std::process::Command;

#[test]
fn run_cir_compiles_and_executes_initializer() {
    let path = std::env::temp_dir().join(format!("lucid_run_cir_{}.lucid", std::process::id()));
    fs::write(&path, "answer = 6 * 7\n").expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[test]
fn run_entry_invokes_named_interpreter_function() {
    let path = std::env::temp_dir().join(format!("lucid_run_entry_{}.lucid", std::process::id()));
    fs::write(&path, "def main():\n    return 42\n").expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            path.to_str().expect("temporary path should be UTF-8"),
            "--entry",
            "main",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stdout).contains("42"),
        "stdout was: {:?}, stderr was: {:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_entry_requires_a_name() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_entry_missing_{}.lucid",
        std::process::id()
    ));
    fs::write(&path, "answer = 42\n").expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            path.to_str().expect("temporary path should be UTF-8"),
            "--entry",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("requires a function name"));
}

#[test]
fn native_run_invokes_zero_argument_entry() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_native_entry_{}.lucid",
        std::process::id()
    ));
    fs::write(&path, "def main():\n    return 42\n").expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            path.to_str().expect("temporary path should be UTF-8"),
            "--native",
            "--entry",
            "main",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "native run failed: stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("42"));
}

#[test]
fn run_entry_rejects_duplicate_flags() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_entry_duplicate_{}.lucid",
        std::process::id()
    ));
    fs::write(&path, "def main():\n    return 42\n").expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            path.to_str().expect("temporary path should be UTF-8"),
            "--entry",
            "main",
            "--entry",
            "main",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("only once"));
}

#[test]
fn run_entry_resolves_manifest_entry_point_target() {
    let root =
        std::env::temp_dir().join(format!("lucid_run_manifest_entry_{}", std::process::id()));
    fs::create_dir_all(&root).expect("temporary project directory should be writable");
    let manifest = root.join("project.yaml");
    let source = root.join("main.lucid");
    let child = root.join("child.lucid");
    fs::write(
        &manifest,
        "name: demo\nversion: \"0.1\"\nentry-points:\n  serve: .child.answer\n",
    )
    .expect("manifest should be writable");
    fs::write(&child, "def answer():\n    return 42\n").expect("child module should be writable");
    fs::write(&source, "import child\n").expect("entry module should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            source.to_str().expect("temporary path should be UTF-8"),
            "--entry",
            "serve",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_dir_all(&root);
    assert!(
        output.status.success(),
        "run failed: stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("42"));
}

#[test]
fn run_entry_rejects_unknown_manifest_entry_point() {
    let root = std::env::temp_dir().join(format!(
        "lucid_run_manifest_entry_unknown_{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary project directory should be writable");
    fs::write(
        root.join("project.yaml"),
        "name: demo\nversion: \"0.1\"\nentry-points:\n  serve: .main\n",
    )
    .expect("manifest should be writable");
    let source = root.join("main.lucid");
    fs::write(&source, "def main():\n    return 42\n").expect("source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            source.to_str().expect("temporary path should be UTF-8"),
            "--entry",
            "missing",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_dir_all(&root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("unknown entry point"));
}

#[test]
fn native_run_resolves_manifest_entry_point_target() {
    let root = std::env::temp_dir().join(format!(
        "lucid_native_manifest_entry_{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary project directory should be writable");
    fs::write(
        root.join("project.yaml"),
        "name: demo\nversion: \"0.1\"\nentry-points:\n  serve: .child.answer\n",
    )
    .expect("manifest should be writable");
    fs::write(root.join("child.lucid"), "def answer():\n    return 42\n")
        .expect("child module should be writable");
    let source = root.join("main.lucid");
    fs::write(&source, "import child\n").expect("entry module should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            source.to_str().expect("temporary path should be UTF-8"),
            "--native",
            "--entry",
            "serve",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_dir_all(&root);
    assert!(
        output.status.success(),
        "native run failed: stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(String::from_utf8_lossy(&output.stdout).contains("42"));
}

#[test]
fn run_entry_enters_manifest_library_context() {
    let root =
        std::env::temp_dir().join(format!("lucid_run_library_context_{}", std::process::id()));
    fs::create_dir_all(&root).expect("temporary project directory should be writable");
    fs::write(
        root.join("project.yaml"),
        "name: demo\nversion: \"0.1\"\nlibrary-context: .managed\nentry-points:\n  serve: .main\n",
    )
    .expect("manifest should be writable");
    let source = root.join("main.lucid");
    fs::write(
        &source,
        "contextmanager def managed():\n    print(\"setup\")\n    yield none\n    print(\"teardown\")\ndef main():\n    return 42\n",
    )
    .expect("source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            source.to_str().expect("temporary path should be UTF-8"),
            "--entry",
            "serve",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_dir_all(&root);
    assert!(
        output.status.success(),
        "run failed: stdout={:?}, stderr={:?}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("setup"));
    assert!(stdout.contains("teardown"));
    assert!(stdout.contains("42"));
}

#[test]
fn native_run_rejects_manifest_library_context_until_native_cleanup_abi_exists() {
    let root = std::env::temp_dir().join(format!(
        "lucid_native_library_context_{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary project directory should be writable");
    fs::write(
        root.join("project.yaml"),
        "name: demo\nversion: \"0.1\"\nlibrary-context: .managed\n",
    )
    .expect("manifest should be writable");
    let source = root.join("main.lucid");
    fs::write(&source, "contextmanager def managed():\n    yield none\n")
        .expect("source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            source.to_str().expect("temporary path should be UTF-8"),
            "--native",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_dir_all(&root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("library-context"));
}

#[test]
fn run_entry_lazily_loads_manifest_context_module() {
    let root = std::env::temp_dir().join(format!(
        "lucid_run_lazy_library_context_{}",
        std::process::id()
    ));
    fs::create_dir_all(&root).expect("temporary project directory should be writable");
    fs::write(
        root.join("project.yaml"),
        "name: demo\nversion: \"0.1\"\nlibrary-context: .setup.managed\nentry-points:\n  serve: .main\n",
    )
    .expect("manifest should be writable");
    fs::write(
        root.join("setup.lucid"),
        "contextmanager def managed():\n    print(\"setup\")\n    yield none\n    print(\"teardown\")\n",
    )
    .expect("context module should be writable");
    let source = root.join("main.lucid");
    fs::write(&source, "def main():\n    return 42\n").expect("source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run",
            source.to_str().expect("temporary path should be UTF-8"),
            "--entry",
            "serve",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_dir_all(&root);
    assert!(output.status.success());
    let stdout = String::from_utf8_lossy(&output.stdout);
    assert!(stdout.contains("setup"));
    assert!(stdout.contains("teardown"));
    assert!(stdout.contains("42"));
}

#[test]
fn run_cir_step_limit_executes_through_bounded_abi_path() {
    let path =
        std::env::temp_dir().join(format!("lucid_run_cir_limit_{}.lucid", std::process::id()));
    fs::write(
        &path,
        "def add(left: int, right: int):\n    return left + right\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "add",
            "--args",
            "20,22",
            "--step-limit",
            "32",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(output.status.success());
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[test]
fn run_cir_rejects_non_positive_step_limit() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_bad_limit_{}.lucid",
        std::process::id()
    ));
    fs::write(&path, "answer = 42\n").expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--step-limit",
            "0",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("positive integer"));
}

#[test]
fn run_cir_validates_ancestor_project_manifest() {
    let root = std::env::temp_dir().join(format!("lucid_run_cir_manifest_{}", std::process::id()));
    let nested = root.join("src/pkg");
    fs::create_dir_all(&nested).expect("temporary project directory should be creatable");
    fs::write(root.join("project.yaml"), "name: bad name\n").expect("manifest should be writable");
    let path = nested.join("main.lucid");
    fs::write(&path, "answer = 42\n").expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_dir_all(&root);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("Project configuration error"));
}

#[test]
fn emit_c_rejects_source_that_fails_type_checking() {
    let path =
        std::env::temp_dir().join(format!("lucid_emit_c_invalid_{}.lucid", std::process::id()));
    fs::write(&path, "for ch in \"abc\":\n    pass\n")
        .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "emit-c",
            path.to_str().expect("temporary path should be UTF-8"),
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("E0200"), "unexpected stderr: {stderr}");
    assert!(
        stderr.contains("for ch in \"abc\":"),
        "emit-c should render the source line before codegen: {stderr}"
    );
}

#[test]
fn check_renders_source_line_for_structured_diagnostics() {
    let path = std::env::temp_dir().join(format!(
        "lucid_check_diagnostic_{}.lucid",
        std::process::id()
    ));
    fs::write(&path, "value: int = \"wrong\"\n").expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "check",
            path.to_str().expect("temporary path should be UTF-8"),
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(!output.status.success());
    assert!(stderr.contains("E0200"), "unexpected stderr: {stderr}");
    assert!(
        stderr.contains("value: int = \"wrong\""),
        "diagnostic should render the source line: {stderr}"
    );
    assert!(
        stderr.contains("^^^^^"),
        "diagnostic should underline the offending span: {stderr}"
    );
}

#[test]
fn run_cir_uses_statement_and_cfg_lowering() {
    let path = std::env::temp_dir().join(format!("lucid_run_cir_cfg_{}.lucid", std::process::id()));
    fs::write(
        &path,
        "if 1 < 2:\n    y = 4\n    answer = y + 2\nelse:\n    answer = 0\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "6");
}

#[test]
fn run_cir_can_select_a_typed_function_body() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_{}.lucid",
        std::process::id()
    ));
    fs::write(&path, "def answer():\n    return 6 * 7\n")
        .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "answer",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir function failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[test]
fn run_cir_passes_integer_arguments_to_typed_function_body() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_args_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def add(left: int, right: int):\n    return left + right\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "add",
            "--args",
            "20,22",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir function args failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
}

#[test]
fn run_cir_executes_dynamic_parameter_conditional() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_if_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(value: int):\n    if value > 0:\n        return value + 1\n    else:\n        return -value\n",
    )
    .expect("temporary source should be writable");
    let positive = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "41",
        ])
        .output()
        .expect("lucid binary should execute");
    let negative = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-41",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        positive.status.success(),
        "positive branch failed: {}",
        String::from_utf8_lossy(&positive.stderr)
    );
    assert!(
        negative.status.success(),
        "negative branch failed: {}",
        String::from_utf8_lossy(&negative.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&positive.stdout).trim(), "42");
    assert_eq!(String::from_utf8_lossy(&negative.stdout).trim(), "41");
}

#[test]
fn run_cir_executes_parameter_counted_while_loop() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_while_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def countdown(n: int):\n    while n > 0:\n        n -= 1\n    return n\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "countdown",
            "--args",
            "4",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}

#[test]
fn run_cir_executes_counted_while_with_pass_pass_tail() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_while_pass_pass_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def countdown(n: int):\n    while n > 0:\n        n -= 1\n        pass\n        pass\n    return n\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "countdown",
            "--args",
            "4",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir pass/pass loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}

#[test]
fn run_cir_executes_signed_update_counted_while_loop() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_signed_update_while_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def countdown(n: int):\n    while n > 0:\n        n += -1\n    return n\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "countdown",
            "--args",
            "4",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir signed-update counted loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "0");
}

#[test]
fn run_cir_executes_local_bound_counted_while_loop() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_local_bound_while_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def clamp_down(n: int, limit: int):\n    value = n\n    stop = limit\n    while value > stop:\n        value -= 1\n    return value\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "clamp_down",
            "--args",
            "5,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir local-bound counted loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "2");
}

#[test]
fn run_cir_executes_void_local_bound_counted_while_loop() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_void_local_bound_while_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def clamp_down(n: int, limit: int):\n    value = n\n    stop = limit\n    while value > stop:\n        value -= 1\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "clamp_down",
            "--args",
            "5,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir void local-bound counted loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn run_cir_executes_parameter_counted_while_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_while_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def count(n: int):\n    total = 0\n    while n > 0:\n        total += 1\n        n -= 1\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "count",
            "--args",
            "4",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir accumulator loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "4");
}

#[test]
fn run_cir_executes_parameter_counted_while_induction_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_while_induction_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_to(n: int):\n    total = 0\n    while n > 0:\n        total += n\n        n -= 1\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_to",
            "--args",
            "4",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir induction accumulator loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "10");
}

#[test]
fn run_cir_executes_signed_update_while_induction_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_signed_update_while_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_to(n: int):\n    total = -1\n    while n > 0:\n        total += n\n        n += -1\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_to",
            "--args",
            "4",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir signed-update induction accumulator loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "9");
}

#[test]
fn run_cir_executes_commuted_update_while_induction_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_commuted_update_while_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_to(n: int):\n    total = -1\n    while n > 0:\n        total += n\n        n = -1 + n\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_to",
            "--args",
            "4",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir commuted-update induction accumulator loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "9");
}

#[test]
fn run_cir_executes_parameter_bound_while_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_parameter_bound_while_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_down_to(n: int, limit: int):\n    total = 0\n    while n > limit:\n        total += n\n        n -= 1\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_down_to",
            "--args",
            "5,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir parameter-bound accumulator loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "12");
}

#[test]
fn run_cir_executes_local_induction_while_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_local_induction_while_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_to(n: int):\n    total = 0\n    value = n\n    while value > 0:\n        total += value\n        value -= 1\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_to",
            "--args",
            "4",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir local-induction accumulator loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "10");
}

#[test]
fn run_cir_executes_local_bound_while_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_local_bound_while_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_down_to(n: int, limit: int):\n    total = 0\n    value = n\n    stop = limit\n    while value > stop:\n        total += value\n        value -= 1\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_down_to",
            "--args",
            "5,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir local-bound accumulator loop failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "12");
}

#[test]
fn run_cir_executes_range_accumulator_with_local_bound_alias() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_range_alias_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_to(n: int):\n    total = 0\n    limit = n\n    for i in range(limit):\n        total += i\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_to",
            "--args",
            "5",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir range alias accumulator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "10");
}

#[test]
fn run_cir_executes_commuted_range_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_commuted_range_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_to(n: int):\n    total = 0\n    for i in range(n):\n        total = i + total\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_to",
            "--args",
            "5",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir commuted range accumulator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "10");
}

#[test]
fn run_cir_executes_literal_step_range_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_literal_step_range_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def count_by_two(n: int):\n    total = 0\n    for i in range(n):\n        total += 2\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "count_by_two",
            "--args",
            "5",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir literal-step range accumulator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "10");
}

#[test]
fn run_cir_executes_signed_literal_step_range_accumulator() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_signed_literal_step_range_accumulator_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def count_down_by_two(n: int):\n    total = 0\n    for i in range(n):\n        total += -2\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "count_down_by_two",
            "--args",
            "5",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir signed literal-step range accumulator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "-10");
}

#[test]
fn run_cir_executes_range_accumulator_with_local_start_alias() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_range_start_alias_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_from(n: int, seed: int):\n    total = 0\n    begin = seed\n    for i in range(begin, n):\n        total += i\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_from",
            "--args",
            "5,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir range start alias accumulator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "9");
}

#[test]
fn run_cir_executes_range_accumulator_with_two_local_bound_aliases() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_range_two_aliases_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_between(seed: int, limit: int):\n    total = 0\n    begin = seed\n    stop = limit\n    for i in range(begin, stop):\n        total += i\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_between",
            "--args",
            "2,6",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir range two-alias accumulator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "14");
}

#[test]
fn run_cir_executes_range_accumulator_with_reordered_local_bound_aliases() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_range_reordered_aliases_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_between(seed: int, limit: int):\n    begin = seed\n    stop = limit\n    total = 0\n    for i in range(begin, stop):\n        total += i\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_between",
            "--args",
            "2,6",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir range reordered-alias accumulator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "14");
}

#[test]
fn run_cir_executes_descending_range_accumulator_with_local_bound_alias() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_descending_range_alias_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def sum_down(n: int, limit: int):\n    total = 0\n    stop = limit\n    for i in range(n, stop, -1):\n        total += i\n    return total\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "sum_down",
            "--args",
            "5,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir descending range alias accumulator failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "12");
}

#[test]
fn run_cir_executes_conditional_return_expression() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_conditional_expr_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(value: int):\n    return value + 1 if value > 0 else -value\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-41",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "conditional return failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "41");
}

#[test]
fn run_cir_propagates_division_error_from_conditional_branch() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_branch_div_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(flag: int, left: int, right: int):\n    if flag > 0:\n        return left / right\n    else:\n        return 7\n",
    )
    .expect("temporary source should be writable");
    let success = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "1,42,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let error = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "1,42,0",
        ])
        .output()
        .expect("lucid binary should execute");
    let overflow = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "1,-9223372036854775808,-1",
        ])
        .output()
        .expect("lucid binary should execute");
    let alternate = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "0,42,0",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        success.status.success(),
        "conditional division failed: {}",
        String::from_utf8_lossy(&success.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&success.stdout).trim(), "21");
    assert!(!error.status.success());
    assert!(String::from_utf8_lossy(&error.stderr).contains("division by zero"));
    assert!(!overflow.status.success());
    assert!(String::from_utf8_lossy(&overflow.stderr).contains("arithmetic overflow"));
    assert!(
        alternate.status.success(),
        "alternate branch failed: {}",
        String::from_utf8_lossy(&alternate.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&alternate.stdout).trim(), "7");
}

#[test]
fn run_cir_preserves_floor_division_semantics_in_conditional_branch() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_branch_floor_div_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(flag: int, left: int, right: int):\n    if flag > 0:\n        return left // right\n    else:\n        return 7\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "1,-5,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let error = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "1,5,0",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "floor division failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "-3");
    assert!(!error.status.success());
    assert!(String::from_utf8_lossy(&error.stderr).contains("division by zero"));
}

#[test]
fn run_cir_preserves_modulo_semantics_in_conditional_branch() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_branch_mod_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(flag: int, left: int, right: int):\n    if flag > 0:\n        return left % right\n    else:\n        return 7\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "1,-5,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let error = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "1,5,0",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "modulo failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1");
    assert!(!error.status.success());
    assert!(String::from_utf8_lossy(&error.stderr).contains("division by zero"));
}

#[test]
fn run_cir_propagates_division_error_from_conditional_expression() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_conditional_div_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(flag: int, left: int, right: int):\n    return left / right if flag > 0 else 7\n",
    )
    .expect("temporary source should be writable");
    let error = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "1,42,0",
        ])
        .output()
        .expect("lucid binary should execute");
    let alternate = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "0,42,0",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(!error.status.success());
    assert!(String::from_utf8_lossy(&error.stderr).contains("division by zero"));
    assert!(
        alternate.status.success(),
        "alternate branch failed: {}",
        String::from_utf8_lossy(&alternate.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&alternate.stdout).trim(), "7");
}

#[test]
fn run_cir_reports_arithmetic_overflow_from_result_abi() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_overflow_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def add(left: int, right: int):\n    return left + right\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "add",
            "--args",
            "9223372036854775807,1",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("arithmetic overflow"));
}

#[test]
fn run_cir_executes_branch_local_assignment() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_local_if_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(value: int):\n    if value > 0:\n        result = value + 1\n    else:\n        result = -value\n    return result\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-41",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "branch-local function failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "41");
}

#[test]
fn run_cir_executes_post_diamond_continuation() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_post_diamond_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(value: int):\n    if value > 0:\n        result = value\n    else:\n        result = -value\n    return result + 1\n",
    )
    .expect("temporary source should be writable");
    let positive = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "41",
        ])
        .output()
        .expect("lucid binary should execute");
    let negative = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-41",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        positive.status.success(),
        "positive post-diamond function failed: {}",
        String::from_utf8_lossy(&positive.stderr)
    );
    assert!(
        negative.status.success(),
        "negative post-diamond function failed: {}",
        String::from_utf8_lossy(&negative.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&positive.stdout).trim(), "42");
    assert_eq!(String::from_utf8_lossy(&negative.stdout).trim(), "42");
}

#[test]
fn run_cir_executes_dynamic_elif_post_diamond_continuation() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_elif_post_diamond_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(value: int, other: int):\n    if value > 0:\n        result = value\n    elif other > 0:\n        result = other\n    else:\n        result = 0\n    return result + 1\n",
    )
    .expect("temporary source should be writable");
    let first = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "41,5",
        ])
        .output()
        .expect("lucid binary should execute");
    let second = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-1,5",
        ])
        .output()
        .expect("lucid binary should execute");
    let fallback = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-1,-5",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        first.status.success(),
        "first branch failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "elif branch failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        fallback.status.success(),
        "else branch failed: {}",
        String::from_utf8_lossy(&fallback.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&first.stdout).trim(), "42");
    assert_eq!(String::from_utf8_lossy(&second.stdout).trim(), "6");
    assert_eq!(String::from_utf8_lossy(&fallback.stdout).trim(), "1");
}

#[test]
fn run_cir_executes_dynamic_elif_initialized_fallback_continuation() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_elif_fallthrough_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(value: int, other: int):\n    result = 0\n    if value > 0:\n        result = value\n    elif other > 0:\n        result = other\n    return result + 1\n",
    )
    .expect("temporary source should be writable");
    let first = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "41,5",
        ])
        .output()
        .expect("lucid binary should execute");
    let second = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-1,5",
        ])
        .output()
        .expect("lucid binary should execute");
    let fallback = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-1,-5",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        first.status.success(),
        "first branch failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "elif branch failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        fallback.status.success(),
        "fallthrough branch failed: {}",
        String::from_utf8_lossy(&fallback.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&first.stdout).trim(), "42");
    assert_eq!(String::from_utf8_lossy(&second.stdout).trim(), "6");
    assert_eq!(String::from_utf8_lossy(&fallback.stdout).trim(), "1");
}

#[test]
fn run_cir_skips_static_false_elif_before_dynamic_continuation() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_static_false_elif_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(value: int, other: int):\n    if value > 0:\n        result = value\n    elif false:\n        result = 99\n    elif other > 0:\n        result = other\n    else:\n        result = 0\n    return result + 1\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-1,5",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "dynamic elif after static false branch failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "6");
}

#[test]
fn run_cir_executes_multiple_dynamic_elif_post_diamond_continuation() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_multi_elif_post_diamond_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(a: int, b: int, c: int):\n    if a > 0:\n        result = a\n    elif b > 0:\n        result = b\n    elif c > 0:\n        result = c\n    else:\n        result = 0\n    return result + 1\n",
    )
    .expect("temporary source should be writable");
    let first = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "41,5,9",
        ])
        .output()
        .expect("lucid binary should execute");
    let second = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-1,5,9",
        ])
        .output()
        .expect("lucid binary should execute");
    let third = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-1,-2,9",
        ])
        .output()
        .expect("lucid binary should execute");
    let fallback = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-1,-2,-3",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        first.status.success(),
        "first branch failed: {}",
        String::from_utf8_lossy(&first.stderr)
    );
    assert!(
        second.status.success(),
        "second branch failed: {}",
        String::from_utf8_lossy(&second.stderr)
    );
    assert!(
        third.status.success(),
        "third branch failed: {}",
        String::from_utf8_lossy(&third.stderr)
    );
    assert!(
        fallback.status.success(),
        "else branch failed: {}",
        String::from_utf8_lossy(&fallback.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&first.stdout).trim(), "42");
    assert_eq!(String::from_utf8_lossy(&second.stdout).trim(), "6");
    assert_eq!(String::from_utf8_lossy(&third.stdout).trim(), "10");
    assert_eq!(String::from_utf8_lossy(&fallback.stdout).trim(), "1");
}

#[test]
fn run_cir_executes_unused_dynamic_elif_branch_locals() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_unused_elif_locals_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def choose(a: int, b: int, c: int):\n    if a > 0:\n        first = a\n    elif b > 0:\n        second = b\n    elif c > 0:\n        third = c\n    else:\n        fallback = 0\n    return 42\n",
    )
    .expect("temporary source should be writable");
    let selected = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "1,2,3",
        ])
        .output()
        .expect("lucid binary should execute");
    let fallback = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "choose",
            "--args",
            "-1,-2,-3",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        selected.status.success(),
        "selected unused branch failed: {}",
        String::from_utf8_lossy(&selected.stderr)
    );
    assert!(
        fallback.status.success(),
        "fallback unused branch failed: {}",
        String::from_utf8_lossy(&fallback.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&selected.stdout).trim(), "42");
    assert_eq!(String::from_utf8_lossy(&fallback.stdout).trim(), "42");
}

#[test]
fn run_cir_executes_one_sided_parameter_conditional() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_optional_if_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def maybe(value: int):\n    if value > 0:\n        return value + 1\n",
    )
    .expect("temporary source should be writable");
    let positive = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "maybe",
            "--args",
            "41",
        ])
        .output()
        .expect("lucid binary should execute");
    let negative = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "maybe",
            "--args",
            "-41",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        positive.status.success(),
        "positive branch failed: {}",
        String::from_utf8_lossy(&positive.stderr)
    );
    assert!(
        negative.status.success(),
        "void fall-through failed: {}",
        String::from_utf8_lossy(&negative.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&positive.stdout).trim(), "42");
    assert_eq!(String::from_utf8_lossy(&negative.stdout).trim(), "0");
}

#[test]
fn run_cir_executes_value_or_pass_conditional() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_function_pass_if_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def maybe(value: int):\n    if value > 0:\n        return value + 1\n    else:\n        pass\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "maybe",
            "--args",
            "41",
        ])
        .output()
        .expect("lucid binary should execute");
    let pass = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "maybe",
            "--args",
            "-41",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "value/pass function failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(
        pass.status.success(),
        "pass branch failed: {}",
        String::from_utf8_lossy(&pass.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42");
    assert_eq!(String::from_utf8_lossy(&pass.stdout).trim(), "0");
}

#[test]
fn run_cir_invokes_void_typed_function_without_integer_abi() {
    let path =
        std::env::temp_dir().join(format!("lucid_run_cir_void_{}.lucid", std::process::id()));
    fs::write(&path, "def answer(value: int):\n    return\n")
        .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "answer",
            "--args",
            "42",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir void function failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert!(output.stdout.is_empty());
}

#[test]
fn run_cir_executes_parameterized_checked_division() {
    let path = std::env::temp_dir().join(format!("lucid_run_cir_div_{}.lucid", std::process::id()));
    fs::write(
        &path,
        "def divide(left: int, right: int):\n    return left / right\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "divide",
            "--args",
            "42,2",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "run-cir division failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "21");
}

#[test]
fn run_cir_reports_parameterized_checked_division_error() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_div_zero_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def divide(left: int, right: int):\n    return left / right\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "divide",
            "--args",
            "42,0",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(!output.status.success());
    assert!(String::from_utf8_lossy(&output.stderr).contains("division by zero"));
}

#[test]
fn run_cir_reports_constant_negative_power_error() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_negative_power_{}.lucid",
        std::process::id()
    ));
    fs::write(&path, "value = 2 ** -1\n").expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(!output.status.success());
    assert!(
        String::from_utf8_lossy(&output.stderr).contains("negative exponent"),
        "unexpected stderr: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}

#[test]
fn run_cir_executes_parameterized_power() {
    let path = std::env::temp_dir().join(format!(
        "lucid_run_cir_dynamic_power_{}.lucid",
        std::process::id()
    ));
    fs::write(
        &path,
        "def power(base: int, exponent: int):\n    return base ** exponent\n",
    )
    .expect("temporary source should be writable");
    let output = Command::new(env!("CARGO_BIN_EXE_lucid"))
        .args([
            "run-cir",
            path.to_str().expect("temporary path should be UTF-8"),
            "--function",
            "power",
            "--args",
            "2,10",
        ])
        .output()
        .expect("lucid binary should execute");
    let _ = fs::remove_file(&path);
    assert!(
        output.status.success(),
        "dynamic power failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "1024");
}
