# wirespec-lsp

Language Server Protocol server for [wirespec](https://github.com/wirespec-lang/wirespec).

Provides diagnostics, completion, hover, Go to Definition, Document Symbols, and semantic tokens for `.wspec` files.

## Install

```bash
cargo install wirespec-lsp
```

## Usage

The LSP server communicates via stdin/stdout. Configure your editor to launch `wirespec-lsp` as the language server for `.wspec` files.

For VS Code, use the [wirespec-language-tools](https://github.com/wirespec-lang/wirespec-language-tools) extension.
