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
/// - Total bits must be <= 64
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

    for (i, field) in fields.iter().enumerate() {
        if is_bit_field(field)
            && let Some(width) = field.wire_width_bits
        {
            current_indices.push(i);
            current_bits += width;
            continue;
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
                wire: wirespec_sema::types::PrimitiveWireType::Bit,
                ..
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
    if !total_bits.is_multiple_of(8) {
        return Err(BitGroupError {
            msg: format!("bitgroup sums to {total_bits} bits, must be multiple of 8"),
        });
    }

    let bitgroup_id = format!("{scope_id}.bitgroup[{group_index}]");

    let mut members = Vec::new();
    let mut cumulative: u16 = 0;

    for &idx in indices {
        let width = fields[idx]
            .wire_width_bits
            .expect("bitgroup member must have a known wire width");
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
