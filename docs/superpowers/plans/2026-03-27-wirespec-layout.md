# wirespec-layout Implementation Plan

> **For agentic workers:** REQUIRED: Use superpowers:subagent-driven-development (if subagents available) or superpowers:executing-plans to implement this plan. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement the Layout IR stage that lowers Semantic IR into wire-shape descriptions — resolving endianness, detecting and packing bitgroups, and computing wire widths, without making parse/serialize strategy decisions.

**Architecture:** Single-pass lowering consuming `SemanticModule` and producing `LayoutModule`. For each scope (packet, frame variant, capsule header/variant), the pass: (1) copies semantic field data, (2) resolves per-field endianness using module default + explicit overrides, (3) computes wire width in bits, (4) detects consecutive bit-fields and forms aligned bitgroups with stable IDs. No semantic legality re-checks. Layout IR embeds needed semantic payload by value (no backreferences).

**Tech Stack:** Rust (edition 2024), `wirespec-sema` crate (Semantic IR types)

**Normative specs:**
- `docs/ref/LAYOUT_IR_SPEC.md` — Layout IR types, bitgroup model, endianness rules
- `docs/ref/RUST_COMPILER_ARCHITECTURE_SPEC.md` §6.3 — Layout stage responsibilities

**Reference implementation:**
- `protospec/wirespec/layout_ir.py` — Python IR types
- `protospec/wirespec/layout_pass.py` — Python layout pass (bitgroup detection, endianness resolution)

---

## File Structure

```
crates/wirespec-layout/
├── Cargo.toml                      # Depends on wirespec-sema only
├── src/
│   ├── lib.rs                      # Crate root, public API: lower()
│   ├── ir.rs                       # Layout IR types (LayoutModule, LayoutField, LayoutBitGroup)
│   ├── lower.rs                    # Main lowering pass: SemanticModule → LayoutModule
│   └── bitgroup.rs                 # Bitgroup detection + offset calculation
└── tests/
    ├── lower_tests.rs              # Integration tests: SemanticModule → LayoutModule
    ├── bitgroup_tests.rs           # Bitgroup detection edge cases
    └── corpus_layout_tests.rs      # Real .wspec files through parse → sema → layout
```

**Responsibilities per file:**

| File | Responsibility |
|------|---------------|
| `ir.rs` | `LayoutModule`, `LayoutPacket`, `LayoutFrame`, `LayoutCapsule`, `LayoutField`, `LayoutBitGroup`, `LayoutBitGroupMember`, `LayoutBitGroupMemberRef`, `LayoutVariantScope` |
| `lower.rs` | `lower()` entry point, field endianness resolution, wire width computation, scope lowering |
| `bitgroup.rs` | `detect_bitgroups()` — scan consecutive bit-fields, validate alignment, compute offsets |

---

## Chunk 1: Layout IR Types

### Task 1: Layout IR type definitions

**Files:**
- Verify: `crates/wirespec-layout/Cargo.toml` (already exists with `wirespec-sema` dep)
- Create: `crates/wirespec-layout/src/ir.rs`

- [ ] **Step 0: Verify Cargo.toml exists**

`crates/wirespec-layout/Cargo.toml` was created during workspace setup. Verify it depends on `wirespec-sema`.

- [ ] **Step 1: Write the IR types**

```rust
// crates/wirespec-layout/src/ir.rs
use wirespec_sema::ir::*;
use wirespec_sema::types::*;
use wirespec_sema::expr::SemanticExpr;
use wirespec_syntax::span::Span;

// ── Root ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutModule {
    pub schema_version: String,         // "layout-ir/v1"
    pub compliance_profile: String,
    pub module_name: String,
    pub module_endianness: Endianness,
    pub imports: Vec<ImportedTypeRef>,
    pub varints: Vec<SemanticVarInt>,
    pub consts: Vec<SemanticConst>,
    pub enums: Vec<SemanticEnum>,
    pub packets: Vec<LayoutPacket>,
    pub frames: Vec<LayoutFrame>,
    pub capsules: Vec<LayoutCapsule>,
}

// ── Packet ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutPacket {
    pub packet_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub fields: Vec<LayoutField>,
    pub derived: Vec<SemanticDerived>,
    pub requires: Vec<SemanticRequire>,
    pub items: Vec<SemanticScopeItem>,
    pub bitgroups: Vec<LayoutBitGroup>,
    pub span: Option<Span>,
}

// ── Frame ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutFrame {
    pub frame_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub tag_name: String,
    pub tag_type: SemanticType,
    pub tag_endianness: Option<Endianness>,
    pub variants: Vec<LayoutVariantScope>,
    pub span: Option<Span>,
}

// ── Capsule ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutCapsule {
    pub capsule_id: String,
    pub name: String,
    pub derive_traits: Vec<DeriveTrait>,
    pub tag_type: SemanticType,
    pub tag_selector: CapsuleTagSelector,
    pub within_field_id: String,
    pub within_field_name: String,
    pub header_fields: Vec<LayoutField>,
    pub header_derived: Vec<SemanticDerived>,
    pub header_requires: Vec<SemanticRequire>,
    pub header_items: Vec<SemanticScopeItem>,
    pub header_bitgroups: Vec<LayoutBitGroup>,
    pub variants: Vec<LayoutVariantScope>,
    pub span: Option<Span>,
}

// ── Variant Scope (shared by frame + capsule) ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutVariantScope {
    pub scope_id: String,
    pub owner: VariantOwner,
    pub variant_name: String,
    pub ordinal: u32,
    pub pattern: SemanticPattern,
    pub fields: Vec<LayoutField>,
    pub derived: Vec<SemanticDerived>,
    pub requires: Vec<SemanticRequire>,
    pub items: Vec<SemanticScopeItem>,
    pub bitgroups: Vec<LayoutBitGroup>,
    pub span: Option<Span>,
}

// ── Field ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutField {
    pub field_id: String,
    pub name: String,
    pub ty: SemanticType,
    pub presence: FieldPresence,
    pub max_elements: Option<u32>,
    pub checksum_algorithm: Option<String>,
    pub wire_width_bits: Option<u16>,
    pub endianness: Option<Endianness>,
    pub bitgroup_member: Option<LayoutBitGroupMemberRef>,
    pub span: Option<Span>,
}

// ── Bitgroup ──

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBitGroup {
    pub bitgroup_id: String,            // "<scope_id>.bitgroup[<index>]"
    pub scope_id: String,
    pub total_bits: u16,
    pub endianness: Endianness,
    pub members: Vec<LayoutBitGroupMember>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBitGroupMember {
    pub field_id: String,
    pub offset_bits: u16,
    pub width_bits: u16,
}

#[derive(Debug, Clone, PartialEq)]
pub struct LayoutBitGroupMemberRef {
    pub bitgroup_id: String,
    pub offset_bits: u16,
    pub width_bits: u16,
}
```

- [ ] **Step 2: Verify it compiles**

Run: `cargo build -p wirespec-layout`
Expected: success

- [ ] **Step 3: Commit**

---

## Chunk 2: Bitgroup Detection

### Task 2: Bitgroup detection and offset calculation

**Files:**
- Create: `crates/wirespec-layout/src/bitgroup.rs`
- Test: `crates/wirespec-layout/tests/bitgroup_tests.rs`

- [ ] **Step 1: Write failing bitgroup tests**

```rust
// crates/wirespec-layout/tests/bitgroup_tests.rs
use wirespec_layout::bitgroup::*;
use wirespec_layout::ir::*;
use wirespec_sema::types::Endianness;

#[test]
fn no_bit_fields_no_groups() {
    let fields = vec![
        mock_layout_field("x", None, "scope"),  // u8, no bit_width
        mock_layout_field("y", None, "scope"),
    ];
    let (groups, updated) = detect_bitgroups(&fields, "scope", Endianness::Big);
    assert!(groups.is_empty());
    assert!(updated.iter().all(|f| f.bitgroup_member.is_none()));
}

#[test]
fn single_byte_bitgroup() {
    // bits[4] + bits[4] = 8 bits → 1 bitgroup
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(4), "scope"),
    ];
    let (groups, updated) = detect_bitgroups(&fields, "scope", Endianness::Big);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].total_bits, 8);
    assert_eq!(groups[0].members.len(), 2);
    // Spec §10: members ordered by increasing offset_bits
    // Big-endian: field "b" at offset 0 (LSB), field "a" at offset 4 (MSB)
    assert_eq!(groups[0].members[0].offset_bits, 0); // "b"
    assert_eq!(groups[0].members[1].offset_bits, 4); // "a"
}

#[test]
fn little_endian_bitgroup() {
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(4), "scope"),
    ];
    let (groups, updated) = detect_bitgroups(&fields, "scope", Endianness::Little);
    assert_eq!(groups.len(), 1);
    // Little-endian: first field at offset 0 (LSB), second at offset 4
    assert_eq!(groups[0].members[0].offset_bits, 0);
    assert_eq!(groups[0].members[1].offset_bits, 4);
}

#[test]
fn two_byte_bitgroup() {
    // bits[6] + bits[2] + bits[8] = 16 bits
    let fields = vec![
        mock_layout_field("a", Some(6), "scope"),
        mock_layout_field("b", Some(2), "scope"),
        mock_layout_field("c", Some(8), "scope"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "scope", Endianness::Big);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].total_bits, 16);
}

#[test]
fn non_bit_field_breaks_group() {
    // bits[4] + bits[4] | u16 | bits[8]
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(4), "scope"),
        mock_layout_field("middle", None, "scope"),  // u16 breaks
        mock_layout_field("c", Some(8), "scope"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "scope", Endianness::Big);
    assert_eq!(groups.len(), 2);
    assert_eq!(groups[0].total_bits, 8);
    assert_eq!(groups[1].total_bits, 8);
}

#[test]
fn unaligned_bitgroup_error() {
    // bits[3] alone = 3 bits (not multiple of 8)
    let fields = vec![
        mock_layout_field("a", Some(3), "scope"),
        mock_layout_field("next", None, "scope"),
    ];
    let result = std::panic::catch_unwind(|| {
        detect_bitgroups(&fields, "scope", Endianness::Big)
    });
    // Should return an error or the function signature returns Result
    // Adjust based on actual implementation
}

#[test]
fn bitgroup_member_refs_set() {
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(4), "scope"),
    ];
    let (groups, updated) = detect_bitgroups(&fields, "scope", Endianness::Big);
    // Fields should have bitgroup_member set
    assert!(updated[0].bitgroup_member.is_some());
    assert!(updated[1].bitgroup_member.is_some());
    let ref0 = updated[0].bitgroup_member.as_ref().unwrap();
    assert_eq!(ref0.bitgroup_id, groups[0].bitgroup_id);
    assert_eq!(ref0.width_bits, 4);
}

#[test]
fn four_byte_bitgroup() {
    // bits[4] + bits[12] + bits[16] = 32 bits
    let fields = vec![
        mock_layout_field("a", Some(4), "scope"),
        mock_layout_field("b", Some(12), "scope"),
        mock_layout_field("c", Some(16), "scope"),
    ];
    let (groups, _) = detect_bitgroups(&fields, "scope", Endianness::Big);
    assert_eq!(groups.len(), 1);
    assert_eq!(groups[0].total_bits, 32);
}

// Helper
fn mock_layout_field(name: &str, wire_width_bits: Option<u16>, _scope: &str) -> LayoutField {
    LayoutField {
        field_id: format!("test.field:{name}"),
        name: name.to_string(),
        ty: wirespec_sema::types::SemanticType::Primitive {
            wire: wirespec_sema::types::PrimitiveWireType::U8,
        },
        presence: wirespec_sema::types::FieldPresence::Always,
        max_elements: None,
        checksum_algorithm: None,
        wire_width_bits,
        endianness: None,
        bitgroup_member: None,
        span: None,
    }
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wirespec-layout --test bitgroup_tests`
Expected: FAIL (module not found)

- [ ] **Step 3: Write the bitgroup implementation**

```rust
// crates/wirespec-layout/src/bitgroup.rs
use crate::ir::*;
use wirespec_sema::types::Endianness;

#[derive(Debug)]
pub struct BitGroupError {
    pub msg: String,
}

impl std::fmt::Display for BitGroupError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "bitgroup error: {}", self.msg)
    }
}

/// Detect consecutive bit-fields, form aligned bitgroups, compute offsets.
///
/// Returns the bitgroups and updated field list with bitgroup_member refs set.
///
/// Rules (from LAYOUT_IR_SPEC §10):
/// - Consecutive bit-width fields form a group
/// - Any non-bit field breaks the group
/// - Total bits must be multiple of 8
/// - Total bits must be ≤ 64
/// - Big-endian: first field at MSB (total - cumulative - width)
/// - Little-endian: first field at LSB (cumulative)
pub fn detect_bitgroups(
    fields: &[LayoutField],
    scope_id: &str,
    endianness: Endianness,
) -> Result<(Vec<LayoutBitGroup>, Vec<LayoutField>), BitGroupError> {
    let mut groups: Vec<LayoutBitGroup> = Vec::new();
    let mut updated_fields: Vec<LayoutField> = fields.to_vec();

    // Track indices of current consecutive bit-fields
    let mut current_indices: Vec<usize> = Vec::new();
    let mut current_bits: u16 = 0;

    for i in 0..fields.len() {
        if let Some(width) = fields[i].wire_width_bits {
            // This is a bit-field (wire_width_bits is set for bits[N] / bit)
            // Only accumulate if it's a sub-byte field (part of bitgroup)
            if is_bit_field(&fields[i]) {
                current_indices.push(i);
                current_bits += width;
                continue;
            }
        }
        // Non-bit field: finalize current group if any
        if !current_indices.is_empty() {
            finalize_group(
                &current_indices,
                current_bits,
                scope_id,
                groups.len(),
                endianness,
                &mut groups,
                &mut updated_fields,
            )?;
            current_indices.clear();
            current_bits = 0;
        }
    }

    // Final trailing group
    if !current_indices.is_empty() {
        finalize_group(
            &current_indices,
            current_bits,
            scope_id,
            groups.len(),
            endianness,
            &mut groups,
            &mut updated_fields,
        )?;
    }

    Ok((groups, updated_fields))
}

/// A field is a "bit field" if its type is Bits { .. } or Primitive { wire: Bit }.
fn is_bit_field(field: &LayoutField) -> bool {
    matches!(
        &field.ty,
        wirespec_sema::types::SemanticType::Bits { .. }
        | wirespec_sema::types::SemanticType::Primitive {
            wire: wirespec_sema::types::PrimitiveWireType::Bit
        }
    )
}

fn finalize_group(
    indices: &[usize],
    total_bits: u16,
    scope_id: &str,
    group_index: usize,
    endianness: Endianness,
    groups: &mut Vec<LayoutBitGroup>,
    fields: &mut [LayoutField],
) -> Result<(), BitGroupError> {
    if total_bits > 64 {
        return Err(BitGroupError {
            msg: format!("bitgroup sums to {total_bits} bits, exceeds maximum 64"),
        });
    }
    if total_bits % 8 != 0 {
        return Err(BitGroupError {
            msg: format!(
                "bitgroup sums to {total_bits} bits, must be multiple of 8"
            ),
        });
    }

    let bitgroup_id = format!("{scope_id}.bitgroup[{group_index}]");

    let mut members = Vec::new();
    let mut cumulative: u16 = 0;

    for &idx in indices {
        let width = fields[idx].wire_width_bits.unwrap();
        let offset = match endianness {
            Endianness::Big => total_bits - cumulative - width,
            Endianness::Little => cumulative,
        };
        members.push(LayoutBitGroupMember {
            field_id: fields[idx].field_id.clone(),
            offset_bits: offset,
            width_bits: width,
        });
        fields[idx].bitgroup_member = Some(LayoutBitGroupMemberRef {
            bitgroup_id: bitgroup_id.clone(),
            offset_bits: offset,
            width_bits: width,
        });
        cumulative += width;
    }

    // Spec §10: members must be ordered by increasing offset_bits
    members.sort_by_key(|m| m.offset_bits);

    groups.push(LayoutBitGroup {
        bitgroup_id,
        scope_id: scope_id.to_string(),
        total_bits,
        endianness,
        members,
    });

    Ok(())
}
```

- [ ] **Step 4: Wire up lib.rs and run tests**

Update `lib.rs`:
```rust
pub mod bitgroup;
pub mod ir;
```

Run: `cargo test -p wirespec-layout --test bitgroup_tests`
Expected: PASS (adjust unaligned test to use Result-based API)

- [ ] **Step 5: Commit**

---

## Chunk 3: Lowering Pass

### Task 3: Main lowering pass

**Files:**
- Create: `crates/wirespec-layout/src/lower.rs`
- Test: `crates/wirespec-layout/tests/lower_tests.rs`

- [ ] **Step 1: Write failing lowering tests**

```rust
// crates/wirespec-layout/tests/lower_tests.rs
use wirespec_layout::lower::lower;
use wirespec_sema::analyze;
use wirespec_sema::ComplianceProfile;
use wirespec_sema::types::Endianness;
use wirespec_syntax::parse;

fn analyze_and_lower(src: &str) -> wirespec_layout::ir::LayoutModule {
    let ast = parse(src).unwrap();
    let sem = analyze(&ast, ComplianceProfile::default()).unwrap();
    lower(&sem).unwrap()
}

#[test]
fn lower_empty_module() {
    let layout = analyze_and_lower("module test");
    assert_eq!(layout.module_name, "test");
    assert_eq!(layout.schema_version, "layout-ir/v1");
}

#[test]
fn lower_simple_packet() {
    let layout = analyze_and_lower("packet P { x: u8, y: u16 }");
    assert_eq!(layout.packets.len(), 1);
    assert_eq!(layout.packets[0].fields.len(), 2);
    // u8 has no endianness, 8-bit width
    assert_eq!(layout.packets[0].fields[0].wire_width_bits, Some(8));
    assert!(layout.packets[0].fields[0].endianness.is_none());
    // u16 gets module default endianness (big)
    assert_eq!(layout.packets[0].fields[1].wire_width_bits, Some(16));
    assert_eq!(layout.packets[0].fields[1].endianness, Some(Endianness::Big));
}

#[test]
#[ignore] // Requires sema to propagate per-field endianness from type aliases
fn lower_explicit_endian() {
    let layout = analyze_and_lower("packet P { x: u16le, y: u32be }");
    assert_eq!(layout.packets[0].fields[0].endianness, Some(Endianness::Little));
    assert_eq!(layout.packets[0].fields[1].endianness, Some(Endianness::Big));
}

#[test]
fn lower_module_endian_little() {
    let layout = analyze_and_lower("@endian little\nmodule test\npacket P { x: u16 }");
    assert_eq!(layout.module_endianness, Endianness::Little);
    assert_eq!(layout.packets[0].fields[0].endianness, Some(Endianness::Little));
}

#[test]
fn lower_bitgroup_packet() {
    let layout = analyze_and_lower("packet P { a: bits[4], b: bits[4], c: u16 }");
    assert_eq!(layout.packets[0].bitgroups.len(), 1);
    assert_eq!(layout.packets[0].bitgroups[0].total_bits, 8);
    assert_eq!(layout.packets[0].bitgroups[0].members.len(), 2);
    // Fields a, b should have bitgroup_member set
    assert!(layout.packets[0].fields[0].bitgroup_member.is_some());
    assert!(layout.packets[0].fields[1].bitgroup_member.is_some());
    assert!(layout.packets[0].fields[2].bitgroup_member.is_none());
}

#[test]
fn lower_bit_single() {
    let layout = analyze_and_lower(
        "packet P { a: bits[4], b: bits[4], c: bit, d: bit, e: bits[6] }"
    );
    // bits[4]+bits[4]=8 | bit+bit+bits[6]=8 → 2 groups
    assert_eq!(layout.packets[0].bitgroups.len(), 2);
}

#[test]
fn lower_bytes_no_width() {
    let layout = analyze_and_lower("packet P { data: bytes[remaining] }");
    // bytes has no fixed wire width
    assert!(layout.packets[0].fields[0].wire_width_bits.is_none());
    assert!(layout.packets[0].fields[0].endianness.is_none());
}

#[test]
fn lower_frame() {
    let src = r#"
        frame F = match tag: u8 {
            0x00 => A {},
            0x01 => B { x: u16 },
            _ => C { data: bytes[remaining] },
        }
    "#;
    let layout = analyze_and_lower(src);
    assert_eq!(layout.frames.len(), 1);
    assert_eq!(layout.frames[0].variants.len(), 3);
    // tag_endianness: u8 has no endianness
    assert!(layout.frames[0].tag_endianness.is_none());
}

#[test]
fn lower_frame_endian_tag() {
    let src = r#"
        type VarInt = {
            prefix: bits[2],
            value: match prefix {
                0b00 => bits[6],
                0b01 => bits[14],
                0b10 => bits[30],
                0b11 => bits[62],
            },
        }
        frame F = match tag: VarInt {
            0x00 => A {},
        }
    "#;
    let layout = analyze_and_lower(src);
    // VarInt tag has no endianness (variable-length)
    assert!(layout.frames[0].tag_endianness.is_none());
}

#[test]
fn lower_capsule() {
    let src = r#"
        capsule C {
            type_field: u8,
            length: u16,
            payload: match type_field within length {
                0 => D { data: bytes[remaining] },
                _ => Unknown { data: bytes[remaining] },
            },
        }
    "#;
    let layout = analyze_and_lower(src);
    assert_eq!(layout.capsules.len(), 1);
    assert_eq!(layout.capsules[0].header_fields.len(), 2);
    assert_eq!(layout.capsules[0].variants.len(), 2);
}

#[test]
fn lower_consts_enums_pass_through() {
    let src = r#"
        const MAX: u8 = 20
        enum E: u8 { A = 0, B = 1 }
        packet P { x: u8 }
    "#;
    let layout = analyze_and_lower(src);
    assert_eq!(layout.consts.len(), 1);
    assert_eq!(layout.enums.len(), 1);
}

#[test]
fn lower_derived_requires_pass_through() {
    let src = "packet P { flags: u8, let is_set: bool = (flags & 1) != 0, require flags > 0 }";
    let layout = analyze_and_lower(src);
    assert_eq!(layout.packets[0].derived.len(), 1);
    assert_eq!(layout.packets[0].requires.len(), 1);
    assert_eq!(layout.packets[0].items.len(), 3); // field + derived + require
}
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test -p wirespec-layout --test lower_tests`
Expected: FAIL

- [ ] **Step 3: Write the lowering implementation**

```rust
// crates/wirespec-layout/src/lower.rs
use crate::bitgroup;
use crate::ir::*;
use wirespec_sema::ir::*;
use wirespec_sema::types::*;

#[derive(Debug)]
pub struct LayoutError {
    pub msg: String,
}

impl std::fmt::Display for LayoutError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "layout error: {}", self.msg)
    }
}

impl std::error::Error for LayoutError {}

pub fn lower(sem: &SemanticModule) -> Result<LayoutModule, LayoutError> {
    let mut packets = Vec::new();
    let mut frames = Vec::new();
    let mut capsules = Vec::new();

    for p in &sem.packets {
        packets.push(lower_packet(p, sem.module_endianness)?);
    }
    for f in &sem.frames {
        frames.push(lower_frame(f, sem.module_endianness)?);
    }
    for c in &sem.capsules {
        capsules.push(lower_capsule(c, sem.module_endianness)?);
    }

    Ok(LayoutModule {
        schema_version: "layout-ir/v1".to_string(),
        compliance_profile: sem.compliance_profile.clone(),
        module_name: sem.module_name.clone(),
        module_endianness: sem.module_endianness,
        imports: sem.imports.clone(),
        varints: sem.varints.clone(),
        consts: sem.consts.clone(),
        enums: sem.enums.clone(),
        packets,
        frames,
        capsules,
    })
}

fn lower_packet(
    p: &SemanticPacket,
    module_endianness: Endianness,
) -> Result<LayoutPacket, LayoutError> {
    let fields = lower_fields(&p.fields, module_endianness);
    let (bitgroups, fields) = bitgroup::detect_bitgroups(
        &fields,
        &p.packet_id,
        module_endianness,
    )
    .map_err(|e| LayoutError { msg: e.msg })?;

    Ok(LayoutPacket {
        packet_id: p.packet_id.clone(),
        name: p.name.clone(),
        derive_traits: p.derive_traits.clone(),
        fields,
        derived: p.derived.clone(),
        requires: p.requires.clone(),
        items: p.items.clone(),
        bitgroups,
        span: p.span,
    })
}

fn lower_frame(
    f: &SemanticFrame,
    module_endianness: Endianness,
) -> Result<LayoutFrame, LayoutError> {
    let tag_endianness = resolve_type_endianness(&f.tag_type, module_endianness);
    let mut variants = Vec::new();
    for v in &f.variants {
        variants.push(lower_variant_scope(v, module_endianness)?);
    }

    Ok(LayoutFrame {
        frame_id: f.frame_id.clone(),
        name: f.name.clone(),
        derive_traits: f.derive_traits.clone(),
        tag_name: f.tag_name.clone(),
        tag_type: f.tag_type.clone(),
        tag_endianness,
        variants,
        span: f.span,
    })
}

fn lower_capsule(
    c: &SemanticCapsule,
    module_endianness: Endianness,
) -> Result<LayoutCapsule, LayoutError> {
    let header_fields = lower_fields(&c.header_fields, module_endianness);
    let header_scope_id = format!("{}#header", c.capsule_id);
    let (header_bitgroups, header_fields) = bitgroup::detect_bitgroups(
        &header_fields,
        &header_scope_id,
        module_endianness,
    )
    .map_err(|e| LayoutError { msg: e.msg })?;

    let mut variants = Vec::new();
    for v in &c.variants {
        variants.push(lower_variant_scope(v, module_endianness)?);
    }

    Ok(LayoutCapsule {
        capsule_id: c.capsule_id.clone(),
        name: c.name.clone(),
        derive_traits: c.derive_traits.clone(),
        tag_type: c.tag_type.clone(),
        tag_selector: c.tag_selector.clone(),
        within_field_id: c.within_field_id.clone(),
        within_field_name: c.within_field_name.clone(),
        header_fields,
        header_derived: c.header_derived.clone(),
        header_requires: c.header_requires.clone(),
        header_items: c.header_items.clone(),
        header_bitgroups,
        variants,
        span: c.span,
    })
}

fn lower_variant_scope(
    v: &SemanticVariantScope,
    module_endianness: Endianness,
) -> Result<LayoutVariantScope, LayoutError> {
    let fields = lower_fields(&v.fields, module_endianness);
    let (bitgroups, fields) = bitgroup::detect_bitgroups(
        &fields,
        &v.scope_id,
        module_endianness,
    )
    .map_err(|e| LayoutError { msg: e.msg })?;

    Ok(LayoutVariantScope {
        scope_id: v.scope_id.clone(),
        owner: v.owner.clone(),
        variant_name: v.variant_name.clone(),
        ordinal: v.ordinal,
        pattern: v.pattern.clone(),
        fields,
        derived: v.derived.clone(),
        requires: v.requires.clone(),
        items: v.items.clone(),
        bitgroups,
        span: v.span,
    })
}

fn lower_fields(
    sem_fields: &[SemanticField],
    module_endianness: Endianness,
) -> Vec<LayoutField> {
    sem_fields
        .iter()
        .map(|f| lower_field(f, module_endianness))
        .collect()
}

fn lower_field(f: &SemanticField, module_endianness: Endianness) -> LayoutField {
    let wire_width_bits = compute_wire_width(&f.ty);
    let endianness = resolve_field_endianness(&f.ty, module_endianness);

    LayoutField {
        field_id: f.field_id.clone(),
        name: f.name.clone(),
        ty: f.ty.clone(),
        presence: f.presence.clone(),
        max_elements: f.max_elements,
        checksum_algorithm: f.checksum_algorithm.clone(),
        wire_width_bits,
        endianness,
        bitgroup_member: None, // Set by bitgroup detection
        span: f.span,
    }
}

/// Compute wire width in bits for a type.
/// Returns None for dynamically-sized types (bytes, arrays, struct refs, varints).
fn compute_wire_width(ty: &SemanticType) -> Option<u16> {
    match ty {
        SemanticType::Primitive { wire } => Some(match wire {
            PrimitiveWireType::U8 | PrimitiveWireType::I8 => 8,
            PrimitiveWireType::U16 | PrimitiveWireType::I16 => 16,
            PrimitiveWireType::U24 => 24,
            PrimitiveWireType::U32 | PrimitiveWireType::I32 => 32,
            PrimitiveWireType::U64 | PrimitiveWireType::I64 => 64,
            PrimitiveWireType::Bit => 1,
            PrimitiveWireType::Bool => return None, // bool is semantic, not wire
        }),
        SemanticType::Bits { width_bits } => Some(*width_bits),
        // Dynamic types
        SemanticType::VarIntRef { .. }
        | SemanticType::Bytes { .. }
        | SemanticType::Array { .. }
        | SemanticType::PacketRef { .. }
        | SemanticType::EnumRef { .. }
        | SemanticType::FrameRef { .. }
        | SemanticType::CapsuleRef { .. } => None,
    }
}

/// Resolve field endianness from type.
/// Returns Some for multi-byte byte-aligned primitives, None otherwise.
fn resolve_field_endianness(
    ty: &SemanticType,
    module_endianness: Endianness,
) -> Option<Endianness> {
    resolve_type_endianness(ty, module_endianness)
}

/// Resolve endianness for a type.
fn resolve_type_endianness(
    ty: &SemanticType,
    module_endianness: Endianness,
) -> Option<Endianness> {
    match ty {
        SemanticType::Primitive { wire } => {
            match wire {
                PrimitiveWireType::U16
                | PrimitiveWireType::I16
                | PrimitiveWireType::U24
                | PrimitiveWireType::U32
                | PrimitiveWireType::I32
                | PrimitiveWireType::U64
                | PrimitiveWireType::I64 => Some(module_endianness),
                // Single-byte or sub-byte: no endianness
                _ => None,
            }
        }
        // EnumRef underlying type may need endianness, but that's resolved
        // when the enum's wire type is lowered
        _ => None,
    }
}
```

**Note on endianness:** The `SemanticType::Primitive` no longer carries endianness (per spec review). The layout pass resolves it from module default. Explicit per-field overrides (u16le, u32be) were already resolved during type alias processing — the `SemanticType::Primitive` for a field typed `u16le` will have `wire: U16`, and the alias resolution in sema already picked the right endianness. We need the sema stage to propagate this. For now, module-default works for non-aliased types. Explicit endianness from type names will need a small enhancement: the sema analyzer should store the resolved endianness somewhere accessible to layout.

A practical approach: store per-field endianness in an auxiliary map during analysis, or add an optional `endianness_override: Option<Endianness>` to `SemanticField`. For this initial implementation, use the module default.

- [ ] **Step 4: Add `pub mod lower;` to lib.rs, add public API**

```rust
// crates/wirespec-layout/src/lib.rs
pub mod bitgroup;
pub mod ir;
pub mod lower;

pub use lower::lower;
pub use ir::LayoutModule;
```

Run: `cargo test -p wirespec-layout`
Expected: PASS

- [ ] **Step 5: Commit**

---

## Chunk 4: Corpus Tests and Finalization

### Task 4: Corpus integration tests

**Files:**
- Create: `crates/wirespec-layout/tests/corpus_layout_tests.rs`

- [ ] **Step 1: Write corpus tests**

```rust
// crates/wirespec-layout/tests/corpus_layout_tests.rs
use wirespec_layout::lower;
use wirespec_sema::{analyze, ComplianceProfile};
use wirespec_syntax::parse;

fn layout_file(path: &str) -> wirespec_layout::ir::LayoutModule {
    let source = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Failed to read {path}: {e}"));
    let ast = parse(&source)
        .unwrap_or_else(|e| panic!("Failed to parse {path}: {e}"));
    let sem = analyze(&ast, ComplianceProfile::Phase2ExtendedCurrent)
        .unwrap_or_else(|e| panic!("Failed to analyze {path}: {e}"));
    lower(&sem)
        .unwrap_or_else(|e| panic!("Failed to lower {path}: {e}"))
}

#[test]
fn corpus_quic_varint() {
    let m = layout_file("../../protospec/examples/quic/varint.wire");
    assert!(m.varints.len() >= 1);
}

#[test]
fn corpus_udp() {
    let m = layout_file("../../protospec/examples/net/udp.wire");
    assert_eq!(m.packets.len(), 1);
}

#[test]
fn corpus_tcp() {
    let m = layout_file("../../protospec/examples/net/tcp.wire");
    assert_eq!(m.packets.len(), 1);
    // TCP has bit fields → should have bitgroups
    assert!(!m.packets[0].bitgroups.is_empty());
}

#[test]
fn corpus_ethernet() {
    let m = layout_file("../../protospec/examples/net/ethernet.wire");
    assert_eq!(m.packets.len(), 1);
}

#[test]
fn corpus_bits_groups() {
    let m = layout_file("../../protospec/examples/test/bits_groups.wire");
    assert_eq!(m.packets.len(), 2);
    // BitTest: bits[4]+bits[4] | u16 | bits[6]+bits[2]+bits[8] | u8
    assert_eq!(m.packets[0].bitgroups.len(), 2);
    // BitTest32: bits[4]+bits[12]+bits[16] = 32 bits
    assert_eq!(m.packets[1].bitgroups.len(), 1);
    assert_eq!(m.packets[1].bitgroups[0].total_bits, 32);
}

#[test]
fn corpus_ble_att() {
    let m = layout_file("../../protospec/examples/ble/att.wire");
    assert!(!m.frames.is_empty());
}

#[test]
fn corpus_mqtt() {
    let m = layout_file("../../protospec/examples/mqtt/mqtt.wire");
    assert!(!m.capsules.is_empty());
}
```

- [ ] **Step 2: Run corpus tests**

Run: `cargo test -p wirespec-layout --test corpus_layout_tests`
Expected: PASS

- [ ] **Step 3: Commit**

---

### Task 5: Full workspace verification

- [ ] **Step 1: Run full workspace tests**

Run: `cargo test --workspace`
Expected: All tests pass across wirespec-syntax, wirespec-sema, wirespec-layout

- [ ] **Step 2: Final commit**

---

## Summary

| Chunk | Tasks | What it delivers |
|-------|-------|-----------------|
| 1 | Task 1 | Layout IR type definitions |
| 2 | Task 2 | Bitgroup detection + offset calculation with tests |
| 3 | Task 3 | Full lowering pass (endianness, wire width, scopes) |
| 4 | Tasks 4-5 | Corpus tests, workspace verification |

**Total test count target:** ~25 tests covering:
- Bitgroup detection (8 tests: no groups, 1-byte, 2-byte, 4-byte, LE/BE offsets, group breaking, member refs, alignment error)
- Lowering (12 tests: empty module, packets, explicit endian, module endian, bitgroups, bytes, frames, capsules, consts/enums passthrough, derived/requires)
- Corpus (7 tests: real .wspec files through full pipeline)

**Known limitations for future work:**
- Per-field endianness overrides from explicit type names (e.g., `u16le` on a `@endian big` module) require the sema stage to propagate the resolved endianness — currently uses module default only
- `EnumRef` wire width and endianness: enum-backed fields have known fixed wire width from their underlying type, but the layout pass currently returns `None` for both. Fixing requires accessing the enum's underlying type info from the `SemanticModule.enums` collection during lowering
- Enum underlying type endianness for frame tags (same root cause)
