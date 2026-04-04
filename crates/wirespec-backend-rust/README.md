# wirespec-backend-rust

Rust code generation backend for [wirespec](https://github.com/wirespec-lang/wirespec).

Generates `.rs` files with parse, serialize, and serialized_len functions from wirespec protocol definitions. Supports ASN.1 integration via rasn.

This is an internal crate used by the wirespec compiler. For the user-facing tool, see [`wirespec`](https://crates.io/crates/wirespec).
