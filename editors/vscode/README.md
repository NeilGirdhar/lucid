# Lucid Language Support for VS Code

This extension provides syntax highlighting and language configuration for the Lucid programming language.

## Features

- **Syntax Highlighting**: Supports keywords, types, mutability sigils (`!`, `&`), definition-site variance (`+`, `-`, `=`), the error propagation operator (`?`), multiple dispatch (`def dispatch`), pattern matching, literals, and comments.
- **Language Configuration**: Auto-closing pairs for parentheses, brackets, braces, and quotes, comment toggling (`#`), and Python/Lucid-style indentation rules.

## Installation

### From Source / Symlink

You can install this extension locally by creating a symlink in your VS Code extensions directory:

```bash
# On Linux / macOS:
ln -s "$(pwd)/editors/vscode" ~/.vscode/extensions/lucid

# On Windows (cmd as Administrator):
mklink /D "%USERPROFILE%\.vscode\extensions\lucid" "%CD%\editors\vscode"
```

Restart or reload VS Code (`Developer: Reload Window`), and `.lucid` files will have full syntax highlighting and indentation support.
