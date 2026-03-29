// crates/wirespec-backend-rust/src/names.rs
//
// Rust naming conventions: PascalCase types, snake_case fields, UPPER_SNAKE constants.

/// Convert PascalCase/camelCase to snake_case.
pub fn to_snake_case(name: &str) -> String {
    let mut result = String::new();
    for (i, ch) in name.chars().enumerate() {
        if ch.is_uppercase() && i > 0 {
            let prev = name.chars().nth(i - 1).unwrap_or('_');
            if prev.is_lowercase() || prev.is_ascii_digit() {
                result.push('_');
            }
        }
        result.push(
            ch.to_lowercase()
                .next()
                .expect("to_lowercase always yields at least one char"),
        );
    }
    result
}

/// Convert a snake_case or PascalCase name to PascalCase.
pub fn to_pascal_case(name: &str) -> String {
    // If name contains underscores, treat as snake_case
    if name.contains('_') {
        name.split('_')
            .filter(|s| !s.is_empty())
            .map(|s| {
                let mut chars = s.chars();
                match chars.next() {
                    Some(c) => {
                        let mut r = c.to_uppercase().to_string();
                        r.extend(chars);
                        r
                    }
                    None => String::new(),
                }
            })
            .collect()
    } else {
        // Already PascalCase or camelCase; ensure first char is uppercase
        let mut chars = name.chars();
        match chars.next() {
            Some(c) => {
                let mut r = c.to_uppercase().to_string();
                r.extend(chars);
                r
            }
            None => String::new(),
        }
    }
}

/// Generate a Rust type name from a wirespec name.
/// Converts to PascalCase, optionally with a prefix.
pub fn rust_type_name(prefix: &str, name: &str) -> String {
    if prefix.is_empty() {
        to_pascal_case(name)
    } else {
        format!("{}{}", to_pascal_case(prefix), to_pascal_case(name))
    }
}

/// Generate a Rust constant name: UPPER_SNAKE_CASE.
pub fn rust_const_name(prefix: &str, name: &str) -> String {
    let snake = to_snake_case(name);
    if prefix.is_empty() {
        snake.to_uppercase()
    } else {
        format!(
            "{}_{}",
            to_snake_case(prefix).to_uppercase(),
            snake.to_uppercase()
        )
    }
}
