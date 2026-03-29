// crates/wirespec-backend-c/src/names.rs
//
// C naming conventions: type names, function names, field accessors, enum members.

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

/// Generate C type name: `{prefix}_{snake_name}_t`
pub fn c_type_name(prefix: &str, name: &str) -> String {
    format!("{prefix}_{}_t", to_snake_case(name))
}

/// Generate C function name: `{prefix}_{snake_name}_{suffix}`
pub fn c_func_name(prefix: &str, name: &str, suffix: &str) -> String {
    format!("{prefix}_{}_{suffix}", to_snake_case(name))
}

/// Generate C enum member name: `{PREFIX}_{SNAKE_NAME}_{member}`
pub fn c_enum_member(prefix: &str, enum_name: &str, member: &str) -> String {
    format!(
        "{}_{}_{member}",
        prefix.to_uppercase(),
        to_snake_case(enum_name).to_uppercase()
    )
}

/// Generate frame tag enum type name: `{prefix}_{frame_snake}_tag_t`
pub fn c_frame_tag_type(prefix: &str, frame_name: &str) -> String {
    format!("{prefix}_{}_tag_t", to_snake_case(frame_name))
}

/// Generate frame tag enum value: `{PREFIX}_{FRAME}_{VARIANT}`
pub fn c_frame_tag_value(prefix: &str, frame_name: &str, variant: &str) -> String {
    format!(
        "{}_{}_{}",
        prefix.to_uppercase(),
        to_snake_case(frame_name).to_uppercase(),
        to_snake_case(variant).to_uppercase()
    )
}

/// Generate include guard name: `{PREFIX}_{SNAKE_NAME}_H`
pub fn c_include_guard(prefix: &str) -> String {
    format!("{}_H", prefix.to_uppercase())
}
