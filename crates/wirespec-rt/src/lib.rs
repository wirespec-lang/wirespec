//! Runtime support for wirespec-generated Rust code.
//!
//! Provides `Cursor` (zero-copy reader), `Writer`, and `Error` types
//! that generated parse/serialize code depends on.

use std::fmt;

/// Errors returned by generated parse/serialize code.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Error {
    /// Input buffer too small.
    ShortBuffer,
    /// Unrecognized match tag.
    InvalidTag,
    /// `require` clause failed.
    Constraint,
    /// Length/count overflow.
    Overflow,
    /// Unhandled state machine event.
    InvalidState,
    /// Subscope consumed more than available.
    ScopeUnderflow,
    /// Array element count exceeds capacity.
    Capacity,
    /// `within` scope underconsume — trailing data.
    TrailingData,
    /// `@strict` rejects non-canonical encoding.
    Noncanonical,
    /// `@checksum` validation failed.
    Checksum,
    /// ASN.1 decode (rasn) failed.
    Asn1Decode,
    /// ASN.1 encode (rasn) failed.
    Asn1Encode,
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Error::ShortBuffer => write!(f, "short buffer"),
            Error::InvalidTag => write!(f, "invalid tag"),
            Error::Constraint => write!(f, "constraint failed"),
            Error::Overflow => write!(f, "overflow"),
            Error::InvalidState => write!(f, "invalid state"),
            Error::ScopeUnderflow => write!(f, "scope underflow"),
            Error::Capacity => write!(f, "array capacity exceeded"),
            Error::TrailingData => write!(f, "trailing data in scope"),
            Error::Noncanonical => write!(f, "non-canonical encoding"),
            Error::Checksum => write!(f, "checksum mismatch"),
            Error::Asn1Decode => write!(f, "ASN.1 decode failed"),
            Error::Asn1Encode => write!(f, "ASN.1 encode failed"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Zero-copy cursor for reading binary data.
pub struct Cursor<'a> {
    data: &'a [u8],
    pos: usize,
}

impl<'a> Cursor<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    fn require(&self, n: usize) -> Result<()> {
        let end = self.pos.checked_add(n).ok_or(Error::ShortBuffer)?;
        if end > self.data.len() {
            Err(Error::ShortBuffer)
        } else {
            Ok(())
        }
    }

    pub fn consumed(&self) -> usize {
        self.pos
    }

    pub fn remaining(&self) -> usize {
        self.data.len() - self.pos
    }

    pub fn bytes(&self) -> &'a [u8] {
        self.data
    }

    // ── Primitive reads ──

    pub fn read_u8(&mut self) -> Result<u8> {
        self.require(1)?;
        let v = self.data[self.pos];
        self.pos += 1;
        Ok(v)
    }

    pub fn read_u16be(&mut self) -> Result<u16> {
        self.require(2)?;
        let v = u16::from_be_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    pub fn read_u16le(&mut self) -> Result<u16> {
        self.require(2)?;
        let v = u16::from_le_bytes([self.data[self.pos], self.data[self.pos + 1]]);
        self.pos += 2;
        Ok(v)
    }

    pub fn read_u24be(&mut self) -> Result<u32> {
        self.require(3)?;
        let v = (self.data[self.pos] as u32) << 16
            | (self.data[self.pos + 1] as u32) << 8
            | self.data[self.pos + 2] as u32;
        self.pos += 3;
        Ok(v)
    }

    pub fn read_u24le(&mut self) -> Result<u32> {
        self.require(3)?;
        let v = self.data[self.pos] as u32
            | (self.data[self.pos + 1] as u32) << 8
            | (self.data[self.pos + 2] as u32) << 16;
        self.pos += 3;
        Ok(v)
    }

    pub fn read_u32be(&mut self) -> Result<u32> {
        self.require(4)?;
        let v = u32::from_be_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    pub fn read_u32le(&mut self) -> Result<u32> {
        self.require(4)?;
        let v = u32::from_le_bytes([
            self.data[self.pos],
            self.data[self.pos + 1],
            self.data[self.pos + 2],
            self.data[self.pos + 3],
        ]);
        self.pos += 4;
        Ok(v)
    }

    pub fn read_u64be(&mut self) -> Result<u64> {
        self.require(8)?;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.data[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(u64::from_be_bytes(buf))
    }

    pub fn read_u64le(&mut self) -> Result<u64> {
        self.require(8)?;
        let mut buf = [0u8; 8];
        buf.copy_from_slice(&self.data[self.pos..self.pos + 8]);
        self.pos += 8;
        Ok(u64::from_le_bytes(buf))
    }

    // ── Signed reads ──

    pub fn read_i8(&mut self) -> Result<i8> {
        self.read_u8().map(|v| v as i8)
    }

    pub fn read_i16be(&mut self) -> Result<i16> {
        self.read_u16be().map(|v| v as i16)
    }

    pub fn read_i16le(&mut self) -> Result<i16> {
        self.read_u16le().map(|v| v as i16)
    }

    pub fn read_i32be(&mut self) -> Result<i32> {
        self.read_u32be().map(|v| v as i32)
    }

    pub fn read_i32le(&mut self) -> Result<i32> {
        self.read_u32le().map(|v| v as i32)
    }

    pub fn read_i64be(&mut self) -> Result<i64> {
        self.read_u64be().map(|v| v as i64)
    }

    pub fn read_i64le(&mut self) -> Result<i64> {
        self.read_u64le().map(|v| v as i64)
    }

    // ── Bytes / remaining ──

    pub fn read_bytes(&mut self, n: usize) -> Result<&'a [u8]> {
        self.require(n)?;
        let slice = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(slice)
    }

    pub fn read_remaining(&mut self) -> &'a [u8] {
        let slice = &self.data[self.pos..];
        self.pos = self.data.len();
        slice
    }

    // ── Sub-cursor (within scope) ──

    pub fn sub_cursor(&mut self, n: usize) -> Result<Cursor<'a>> {
        self.require(n)?;
        let sub_data = &self.data[self.pos..self.pos + n];
        self.pos += n;
        Ok(Cursor {
            data: sub_data,
            pos: 0,
        })
    }
}

/// Writer for serializing binary data.
pub struct Writer<'a> {
    buf: &'a mut [u8],
    pos: usize,
}

impl<'a> Writer<'a> {
    pub fn new(buf: &'a mut [u8]) -> Self {
        Self { buf, pos: 0 }
    }

    fn require(&self, n: usize) -> Result<()> {
        let end = self.pos.checked_add(n).ok_or(Error::ShortBuffer)?;
        if end > self.buf.len() {
            Err(Error::ShortBuffer)
        } else {
            Ok(())
        }
    }

    pub fn written(&self) -> usize {
        self.pos
    }

    pub fn as_written_mut(&mut self) -> &mut [u8] {
        &mut self.buf[..self.pos]
    }

    // ── Primitive writes ──

    pub fn write_u8(&mut self, v: u8) -> Result<()> {
        self.require(1)?;
        self.buf[self.pos] = v;
        self.pos += 1;
        Ok(())
    }

    pub fn write_u16be(&mut self, v: u16) -> Result<()> {
        self.require(2)?;
        let bytes = v.to_be_bytes();
        self.buf[self.pos..self.pos + 2].copy_from_slice(&bytes);
        self.pos += 2;
        Ok(())
    }

    pub fn write_u16le(&mut self, v: u16) -> Result<()> {
        self.require(2)?;
        let bytes = v.to_le_bytes();
        self.buf[self.pos..self.pos + 2].copy_from_slice(&bytes);
        self.pos += 2;
        Ok(())
    }

    pub fn write_u24be(&mut self, v: u32) -> Result<()> {
        self.require(3)?;
        self.buf[self.pos] = (v >> 16) as u8;
        self.buf[self.pos + 1] = (v >> 8) as u8;
        self.buf[self.pos + 2] = v as u8;
        self.pos += 3;
        Ok(())
    }

    pub fn write_u24le(&mut self, v: u32) -> Result<()> {
        self.require(3)?;
        self.buf[self.pos] = v as u8;
        self.buf[self.pos + 1] = (v >> 8) as u8;
        self.buf[self.pos + 2] = (v >> 16) as u8;
        self.pos += 3;
        Ok(())
    }

    pub fn write_u32be(&mut self, v: u32) -> Result<()> {
        self.require(4)?;
        let bytes = v.to_be_bytes();
        self.buf[self.pos..self.pos + 4].copy_from_slice(&bytes);
        self.pos += 4;
        Ok(())
    }

    pub fn write_u32le(&mut self, v: u32) -> Result<()> {
        self.require(4)?;
        let bytes = v.to_le_bytes();
        self.buf[self.pos..self.pos + 4].copy_from_slice(&bytes);
        self.pos += 4;
        Ok(())
    }

    pub fn write_u64be(&mut self, v: u64) -> Result<()> {
        self.require(8)?;
        let bytes = v.to_be_bytes();
        self.buf[self.pos..self.pos + 8].copy_from_slice(&bytes);
        self.pos += 8;
        Ok(())
    }

    pub fn write_u64le(&mut self, v: u64) -> Result<()> {
        self.require(8)?;
        let bytes = v.to_le_bytes();
        self.buf[self.pos..self.pos + 8].copy_from_slice(&bytes);
        self.pos += 8;
        Ok(())
    }

    // ── Signed writes ──

    pub fn write_i8(&mut self, v: i8) -> Result<()> {
        self.write_u8(v as u8)
    }

    pub fn write_i16be(&mut self, v: i16) -> Result<()> {
        self.write_u16be(v as u16)
    }

    pub fn write_i16le(&mut self, v: i16) -> Result<()> {
        self.write_u16le(v as u16)
    }

    pub fn write_i32be(&mut self, v: i32) -> Result<()> {
        self.write_u32be(v as u32)
    }

    pub fn write_i32le(&mut self, v: i32) -> Result<()> {
        self.write_u32le(v as u32)
    }

    pub fn write_i64be(&mut self, v: i64) -> Result<()> {
        self.write_u64be(v as u64)
    }

    pub fn write_i64le(&mut self, v: i64) -> Result<()> {
        self.write_u64le(v as u64)
    }

    // ── Bytes ──

    pub fn write_bytes(&mut self, data: &[u8]) -> Result<()> {
        self.require(data.len())?;
        self.buf[self.pos..self.pos + data.len()].copy_from_slice(data);
        self.pos += data.len();
        Ok(())
    }
}

// ── Checksum functions ──
//
// CRITICAL: These signatures must match what the Rust backend emitter generates.
// See crates/wirespec-backend-rust/src/emit.rs emit_checksum_verify_rust / emit_checksum_compute_rust.
//
// ZeroSum verify:  internet_checksum(data: &[u8]) -> u16
// ZeroSum compute: internet_checksum_compute(data: &mut [u8], cksum_offset: usize)  [patches in-place]
// RecomputeCompare verify: {algo}_verify(data: &[u8], cksum_offset: usize, width: usize) -> T
// RecomputeCompare compute: {algo}_compute(data: &mut [u8], cksum_offset: usize) -> T

fn ones_complement_sum(data: &[u8]) -> u16 {
    let mut sum: u32 = 0;
    let mut i = 0;
    while i + 1 < data.len() {
        sum += u16::from_be_bytes([data[i], data[i + 1]]) as u32;
        i += 2;
    }
    if i < data.len() {
        sum += (data[i] as u32) << 8;
    }
    while sum >> 16 != 0 {
        sum = (sum & 0xFFFF) + (sum >> 16);
    }
    !(sum as u16)
}

/// Internet checksum (RFC 1071) verification: returns one's complement sum.
pub fn internet_checksum(data: &[u8]) -> u16 {
    ones_complement_sum(data)
}

/// Internet checksum compute: zero the field, compute, patch in-place.
pub fn internet_checksum_compute(data: &mut [u8], cksum_offset: usize) {
    let cksum_end = cksum_offset
        .checked_add(2)
        .expect("checksum offset overflow");
    let field = data
        .get_mut(cksum_offset..cksum_end)
        .expect("checksum offset out of bounds");
    field[0] = 0;
    field[1] = 0;
    let val = ones_complement_sum(data);
    let bytes = val.to_be_bytes();
    let field = data
        .get_mut(cksum_offset..cksum_end)
        .expect("checksum offset out of bounds");
    field[0] = bytes[0];
    field[1] = bytes[1];
}

/// Compute CRC-32 while treating `skip_offset..skip_offset+skip_len` as zeros.
fn raw_crc32_with_skip(data: &[u8], poly: u32, skip_offset: usize, skip_len: usize) -> u32 {
    let skip_end = skip_offset + skip_len;
    let mut crc: u32 = 0xFFFF_FFFF;
    for (i, &byte) in data.iter().enumerate() {
        let b = if i >= skip_offset && i < skip_end {
            0
        } else {
            byte
        };
        crc ^= b as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ poly;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// Compute Fletcher-16 while treating `skip_offset..skip_offset+skip_len` as zeros.
fn raw_fletcher16_with_skip(data: &[u8], skip_offset: usize, skip_len: usize) -> u16 {
    let skip_end = skip_offset + skip_len;
    let mut sum1: u16 = 0;
    let mut sum2: u16 = 0;
    for (i, &byte) in data.iter().enumerate() {
        let b = if i >= skip_offset && i < skip_end {
            0
        } else {
            byte
        };
        sum1 = (sum1 + b as u16) % 255;
        sum2 = (sum2 + sum1) % 255;
    }
    (sum2 << 8) | sum1
}

fn raw_crc32(data: &[u8], poly: u32) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &byte in data {
        crc ^= byte as u32;
        for _ in 0..8 {
            if crc & 1 != 0 {
                crc = (crc >> 1) ^ poly;
            } else {
                crc >>= 1;
            }
        }
    }
    !crc
}

/// CRC-32 (IEEE 802.3) verify: recompute skipping checksum field (no allocation).
pub fn crc32_verify(data: &[u8], cksum_offset: usize, width: usize) -> u32 {
    assert!(
        data.get(
            cksum_offset
                ..cksum_offset
                    .checked_add(width)
                    .expect("checksum offset overflow")
        )
        .is_some(),
        "checksum offset out of bounds"
    );
    raw_crc32_with_skip(data, 0xEDB8_8320, cksum_offset, width)
}

/// CRC-32 compute: zero checksum field, compute.
pub fn crc32_compute(data: &mut [u8], cksum_offset: usize) -> u32 {
    let width = 4;
    let cksum_end = cksum_offset
        .checked_add(width)
        .expect("checksum offset overflow");
    let field = data
        .get_mut(cksum_offset..cksum_end)
        .expect("checksum offset out of bounds");
    field.fill(0);
    raw_crc32(data, 0xEDB8_8320)
}

/// CRC-32C (Castagnoli) verify (no allocation).
pub fn crc32c_verify(data: &[u8], cksum_offset: usize, width: usize) -> u32 {
    assert!(
        data.get(
            cksum_offset
                ..cksum_offset
                    .checked_add(width)
                    .expect("checksum offset overflow")
        )
        .is_some(),
        "checksum offset out of bounds"
    );
    raw_crc32_with_skip(data, 0x82F6_3B78, cksum_offset, width)
}

/// CRC-32C compute.
pub fn crc32c_compute(data: &mut [u8], cksum_offset: usize) -> u32 {
    let width = 4;
    let cksum_end = cksum_offset
        .checked_add(width)
        .expect("checksum offset overflow");
    let field = data
        .get_mut(cksum_offset..cksum_end)
        .expect("checksum offset out of bounds");
    field.fill(0);
    raw_crc32(data, 0x82F6_3B78)
}

fn raw_fletcher16(data: &[u8]) -> u16 {
    let mut sum1: u16 = 0;
    let mut sum2: u16 = 0;
    for &byte in data {
        sum1 = (sum1 + byte as u16) % 255;
        sum2 = (sum2 + sum1) % 255;
    }
    (sum2 << 8) | sum1
}

/// Fletcher-16 verify: recompute skipping checksum field (no allocation).
pub fn fletcher16_verify(data: &[u8], cksum_offset: usize, width: usize) -> u16 {
    assert!(
        data.get(
            cksum_offset
                ..cksum_offset
                    .checked_add(width)
                    .expect("checksum offset overflow")
        )
        .is_some(),
        "checksum offset out of bounds"
    );
    raw_fletcher16_with_skip(data, cksum_offset, width)
}

/// Fletcher-16 compute.
pub fn fletcher16_compute(data: &mut [u8], cksum_offset: usize) -> u16 {
    let width = 2;
    let cksum_end = cksum_offset
        .checked_add(width)
        .expect("checksum offset overflow");
    let field = data
        .get_mut(cksum_offset..cksum_end)
        .expect("checksum offset out of bounds");
    field.fill(0);
    raw_fletcher16(data)
}
