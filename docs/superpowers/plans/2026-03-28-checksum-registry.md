# Checksum Registry Refactoring Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Replace hardcoded checksum match statements across 6 files with a centralized `ChecksumCatalog` so adding a new algorithm requires changes in only 1-2 files.

**Architecture:** Create a `ChecksumAlgorithmSpec` struct containing all algorithm metadata (field type, width, verify mode, input model, profile). Store all specs in a static `ChecksumCatalog`. Sema, codec, and backends query the catalog instead of having their own match statements. Backend bindings remain per-backend (runtime symbols are target-specific), but algorithm metadata is centralized.

**Tech Stack:** Rust, `wirespec-sema` crate (catalog lives here, shared downstream)

---

## Current State (6 files with hardcoded matches)

| File | What's hardcoded |
|------|-----------------|
| `sema/profile.rs` | allowed algorithms per profile, required field type, field width |
| `codec/checksum.rs` | verify_mode, input_model, field_width |
| `backend-c/checksum_binding.rs` | runtime symbol names |
| `backend-c/source.rs` | verify/compute C code patterns |
| `backend-rust/checksum_binding.rs` | runtime symbol names |
| `backend-rust/emit.rs` | verify/compute Rust code patterns |

## Target State

| File | Role |
|------|------|
| `sema/checksum_catalog.rs` | **Single source of truth** — all algorithm metadata |
| `sema/profile.rs` | Queries catalog for profile filtering |
| `codec/checksum.rs` | Queries catalog for verify_mode/input_model/width |
| `backend-c/checksum_binding.rs` | Runtime symbols only (unchanged) |
| `backend-c/source.rs` | Uses catalog metadata for code pattern selection |
| `backend-rust/checksum_binding.rs` | Runtime symbols only (unchanged) |
| `backend-rust/emit.rs` | Uses catalog metadata for code pattern selection |

**Adding a new algorithm = add 1 entry to catalog + add runtime symbols to each backend binding.**

---

## Chunk 1: Catalog Definition + Sema Integration

### Task 1: Create ChecksumCatalog

**Files:**
- Create: `crates/wirespec-sema/src/checksum_catalog.rs`
- Modify: `crates/wirespec-sema/src/lib.rs`

- [ ] **Step 1: Define the catalog**

```rust
// crates/wirespec-sema/src/checksum_catalog.rs

/// All metadata for a checksum algorithm — single source of truth.
#[derive(Debug, Clone)]
pub struct ChecksumAlgorithmSpec {
    pub id: &'static str,
    pub required_field_type: &'static str,  // "u16" or "u32"
    pub field_width_bytes: u8,              // 2 or 4
    pub verify_mode: ChecksumVerifyMode,
    pub input_model: ChecksumInputModel,
    pub min_profile: &'static str,          // "phase2_strict_v1_0" or "phase2_extended_current"
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumVerifyMode {
    ZeroSum,
    RecomputeCompare,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChecksumInputModel {
    ZeroSumWholeScope,
    RecomputeWithSkippedField,
}

/// Static catalog of all known checksum algorithms.
static CATALOG: &[ChecksumAlgorithmSpec] = &[
    ChecksumAlgorithmSpec {
        id: "internet",
        required_field_type: "u16",
        field_width_bytes: 2,
        verify_mode: ChecksumVerifyMode::ZeroSum,
        input_model: ChecksumInputModel::ZeroSumWholeScope,
        min_profile: "phase2_strict_v1_0",
    },
    ChecksumAlgorithmSpec {
        id: "crc32",
        required_field_type: "u32",
        field_width_bytes: 4,
        verify_mode: ChecksumVerifyMode::RecomputeCompare,
        input_model: ChecksumInputModel::RecomputeWithSkippedField,
        min_profile: "phase2_strict_v1_0",
    },
    ChecksumAlgorithmSpec {
        id: "crc32c",
        required_field_type: "u32",
        field_width_bytes: 4,
        verify_mode: ChecksumVerifyMode::RecomputeCompare,
        input_model: ChecksumInputModel::RecomputeWithSkippedField,
        min_profile: "phase2_strict_v1_0",
    },
    ChecksumAlgorithmSpec {
        id: "fletcher16",
        required_field_type: "u16",
        field_width_bytes: 2,
        verify_mode: ChecksumVerifyMode::RecomputeCompare,
        input_model: ChecksumInputModel::RecomputeWithSkippedField,
        min_profile: "phase2_extended_current",
    },
];

/// Look up an algorithm by name.
pub fn lookup(algorithm: &str) -> Option<&'static ChecksumAlgorithmSpec> {
    CATALOG.iter().find(|s| s.id == algorithm)
}

/// Get all algorithm IDs allowed under a given profile.
pub fn algorithms_for_profile(profile: &str) -> Vec<&'static str> {
    CATALOG
        .iter()
        .filter(|s| profile_includes(profile, s.min_profile))
        .map(|s| s.id)
        .collect()
}

fn profile_includes(active: &str, required: &str) -> bool {
    match (active, required) {
        (_, "phase2_strict_v1_0") => true, // strict is included in all profiles
        ("phase2_extended_current", "phase2_extended_current") => true,
        _ => false,
    }
}
```

- [ ] **Step 2: Add `pub mod checksum_catalog;` to lib.rs**

- [ ] **Step 3: Commit**

---

### Task 2: Refactor sema/profile.rs to use catalog

**Files:**
- Modify: `crates/wirespec-sema/src/profile.rs`

- [ ] **Step 1: Replace hardcoded match statements with catalog lookups**

```rust
impl ComplianceProfile {
    pub fn allowed_checksum_algorithms(self) -> Vec<&'static str> {
        crate::checksum_catalog::algorithms_for_profile(self.as_str())
    }
    // ... rest unchanged
}

pub fn checksum_required_type(algorithm: &str) -> Option<&'static str> {
    crate::checksum_catalog::lookup(algorithm).map(|s| s.required_field_type)
}

pub fn checksum_field_width(algorithm: &str) -> Option<u8> {
    crate::checksum_catalog::lookup(algorithm).map(|s| s.field_width_bytes)
}
```

Note: `allowed_checksum_algorithms` return type changes from `&'static [&'static str]` to `Vec<&'static str>`. Update callers.

- [ ] **Step 2: Run tests, fix any callers, commit**

---

### Task 3: Refactor codec/checksum.rs to use catalog

**Files:**
- Modify: `crates/wirespec-codec/src/checksum.rs`
- Modify: `crates/wirespec-codec/Cargo.toml` (if needed — already depends on wirespec-sema)

- [ ] **Step 1: Replace hardcoded modes with catalog lookup**

```rust
use wirespec_sema::checksum_catalog;

fn checksum_modes(algorithm: &str) -> (ChecksumVerifyMode, ChecksumInputModel) {
    if let Some(spec) = checksum_catalog::lookup(algorithm) {
        (
            match spec.verify_mode {
                checksum_catalog::ChecksumVerifyMode::ZeroSum => ChecksumVerifyMode::ZeroSum,
                checksum_catalog::ChecksumVerifyMode::RecomputeCompare => ChecksumVerifyMode::RecomputeCompare,
            },
            match spec.input_model {
                checksum_catalog::ChecksumInputModel::ZeroSumWholeScope => ChecksumInputModel::ZeroSumWholeScope,
                checksum_catalog::ChecksumInputModel::RecomputeWithSkippedField => ChecksumInputModel::RecomputeWithSkippedField,
            },
        )
    } else {
        (ChecksumVerifyMode::RecomputeCompare, ChecksumInputModel::RecomputeWithSkippedField)
    }
}

fn checksum_field_width(algorithm: &str) -> u8 {
    checksum_catalog::lookup(algorithm)
        .map(|s| s.field_width_bytes)
        .unwrap_or(0)
}
```

Note: The codec IR has its own `ChecksumVerifyMode`/`ChecksumInputModel` enums (in `ir.rs`). The conversion above maps between catalog types and codec types. Alternatively, make codec re-use the catalog types directly, or keep the conversion for API stability.

- [ ] **Step 2: Run tests, commit**

---

## Chunk 2: Backend Codegen Uses Catalog Metadata

### Task 4: C backend source.rs uses catalog for code pattern selection

**Files:**
- Modify: `crates/wirespec-backend-c/src/source.rs`

- [ ] **Step 1: Replace algorithm-specific match with catalog-driven logic**

The C codegen currently has separate branches for `"internet"`, `"crc32"|"crc32c"`, `"fletcher16"`. Refactor to use `verify_mode` from the catalog:

```rust
use wirespec_sema::checksum_catalog;

fn emit_checksum_verify(..., algorithm: &str, ...) {
    let spec = checksum_catalog::lookup(algorithm);
    match spec.map(|s| s.verify_mode) {
        Some(checksum_catalog::ChecksumVerifyMode::ZeroSum) => {
            // internet pattern: whole-scope sum == 0
        }
        Some(checksum_catalog::ChecksumVerifyMode::RecomputeCompare) => {
            // crc32/crc32c/fletcher16 pattern: recompute with skip, compare
            // Use spec.field_width_bytes for byte count
        }
        None => { /* unknown algorithm — comment */ }
    }
}
```

The key insight: the C code patterns are determined by `verify_mode` and `field_width_bytes`, NOT by algorithm name. So adding a new recompute-compare algorithm with 4-byte width (e.g., `crc64` with u64 field) would work automatically if the catalog has the right metadata.

Backend binding (`checksum_binding.rs`) still maps algorithm → runtime function names. That's correct — function names ARE target-specific.

- [ ] **Step 2: Run tests, commit**

---

### Task 5: Rust backend emit.rs uses catalog for code pattern selection

**Files:**
- Modify: `crates/wirespec-backend-rust/src/emit.rs`

- [ ] **Step 1: Same refactoring as Task 4 but for Rust codegen**

Replace algorithm-name matches with `verify_mode`-based branching using catalog lookup.

- [ ] **Step 2: Run tests, commit**

---

### Task 6: Add test for easy extensibility

**Files:**
- Test: `crates/wirespec-sema/tests/checksum_catalog_tests.rs`

- [ ] **Step 1: Write catalog tests**

```rust
use wirespec_sema::checksum_catalog;

#[test]
fn catalog_known_algorithms() {
    assert!(checksum_catalog::lookup("internet").is_some());
    assert!(checksum_catalog::lookup("crc32").is_some());
    assert!(checksum_catalog::lookup("crc32c").is_some());
    assert!(checksum_catalog::lookup("fletcher16").is_some());
}

#[test]
fn catalog_unknown_algorithm() {
    assert!(checksum_catalog::lookup("sha256").is_none());
}

#[test]
fn catalog_strict_profile() {
    let algos = checksum_catalog::algorithms_for_profile("phase2_strict_v1_0");
    assert!(algos.contains(&"internet"));
    assert!(algos.contains(&"crc32"));
    assert!(algos.contains(&"crc32c"));
    assert!(!algos.contains(&"fletcher16")); // extension only
}

#[test]
fn catalog_extended_profile() {
    let algos = checksum_catalog::algorithms_for_profile("phase2_extended_current");
    assert!(algos.contains(&"internet"));
    assert!(algos.contains(&"fletcher16")); // included in extended
}

#[test]
fn catalog_field_metadata() {
    let internet = checksum_catalog::lookup("internet").unwrap();
    assert_eq!(internet.required_field_type, "u16");
    assert_eq!(internet.field_width_bytes, 2);

    let crc32 = checksum_catalog::lookup("crc32").unwrap();
    assert_eq!(crc32.required_field_type, "u32");
    assert_eq!(crc32.field_width_bytes, 4);
}
```

- [ ] **Step 2: Run all tests, commit**

---

## Summary

| Task | File(s) | Change |
|------|---------|--------|
| 1 | NEW: `sema/checksum_catalog.rs` | Central catalog with all algorithm metadata |
| 2 | `sema/profile.rs` | Query catalog instead of hardcoded lists |
| 3 | `codec/checksum.rs` | Query catalog for verify_mode/input_model/width |
| 4 | `backend-c/source.rs` | Use verify_mode for code pattern selection |
| 5 | `backend-rust/emit.rs` | Same for Rust |
| 6 | NEW: test file | Catalog correctness tests |

**After refactoring, adding a new algorithm (e.g., `crc16_ccitt`) requires:**
1. Add 1 entry to `CATALOG` in `checksum_catalog.rs`
2. Add runtime binding in `backend-c/checksum_binding.rs` and `backend-rust/checksum_binding.rs`
3. Implement runtime function in `wirespec_runtime.h` / `wirespec-rt`

**No changes to:** sema validation, codec lowering, or backend codegen patterns (they use verify_mode, not algorithm name).
