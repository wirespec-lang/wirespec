// Copyright (c) wirespec contributors
// SPDX-License-Identifier: MIT OR Apache-2.0

//! Comprehensive tests for wirespec-rt: Cursor, Writer, round-trips, and checksums.

use wirespec_rt::{
    Cursor, Error, Writer, crc32_compute, crc32_verify, crc32c_compute, crc32c_verify,
    fletcher16_compute, fletcher16_verify, internet_checksum, internet_checksum_compute,
};

// ═══════════════════════════════════════════════════════════════════════════
// Cursor tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_cursor_read_u8() {
    let data = [0xAB];
    let mut cur = Cursor::new(&data);
    assert_eq!(cur.read_u8().unwrap(), 0xAB);
    assert_eq!(cur.consumed(), 1);
    assert_eq!(cur.remaining(), 0);
}

#[test]
fn test_cursor_read_u16be() {
    let data = [0x01, 0x02];
    let mut cur = Cursor::new(&data);
    assert_eq!(cur.read_u16be().unwrap(), 0x0102);
    assert_eq!(cur.consumed(), 2);
}

#[test]
fn test_cursor_read_u16le() {
    let data = [0x01, 0x02];
    let mut cur = Cursor::new(&data);
    assert_eq!(cur.read_u16le().unwrap(), 0x0201);
    assert_eq!(cur.consumed(), 2);
}

#[test]
fn test_cursor_read_u32be() {
    let data = [0xDE, 0xAD, 0xBE, 0xEF];
    let mut cur = Cursor::new(&data);
    assert_eq!(cur.read_u32be().unwrap(), 0xDEADBEEF);
    assert_eq!(cur.consumed(), 4);
}

#[test]
fn test_cursor_read_bytes() {
    let data = [0x01, 0x02, 0x03, 0x04, 0x05];
    let mut cur = Cursor::new(&data);
    let bytes = cur.read_bytes(3).unwrap();
    assert_eq!(bytes, &[0x01, 0x02, 0x03]);
    assert_eq!(cur.consumed(), 3);
    assert_eq!(cur.remaining(), 2);
}

#[test]
fn test_cursor_read_remaining() {
    let data = [0x01, 0x02, 0x03, 0x04, 0x05];
    let mut cur = Cursor::new(&data);
    let _ = cur.read_u8().unwrap(); // consume first byte
    let rest = cur.read_remaining();
    assert_eq!(rest, &[0x02, 0x03, 0x04, 0x05]);
    assert_eq!(cur.consumed(), 5);
    assert_eq!(cur.remaining(), 0);
}

#[test]
fn test_cursor_short_buffer() {
    let data = [0x01];
    let mut cur = Cursor::new(&data);
    let err = cur.read_u16be().unwrap_err();
    assert_eq!(err, Error::ShortBuffer);
}

#[test]
fn test_cursor_overflow_require() {
    // require with usize::MAX should trigger the checked_add overflow path
    let data = [0x01, 0x02];
    let mut cur = Cursor::new(&data);
    let _ = cur.read_u8().unwrap(); // pos = 1
    // Now try to read usize::MAX bytes — pos + usize::MAX overflows
    let err = cur.read_bytes(usize::MAX).unwrap_err();
    assert_eq!(err, Error::ShortBuffer);
}

#[test]
fn test_cursor_empty() {
    let data: [u8; 0] = [];
    let mut cur = Cursor::new(&data);
    assert_eq!(cur.remaining(), 0);
    assert_eq!(cur.consumed(), 0);
    assert_eq!(cur.read_u8().unwrap_err(), Error::ShortBuffer);
    assert_eq!(cur.read_u16be().unwrap_err(), Error::ShortBuffer);
    assert_eq!(cur.read_u32be().unwrap_err(), Error::ShortBuffer);
    assert_eq!(cur.read_bytes(1).unwrap_err(), Error::ShortBuffer);
    // read_remaining on empty is fine — returns empty slice
    assert_eq!(cur.read_remaining(), &[] as &[u8]);
}

#[test]
fn test_cursor_sub_cursor() {
    let data = [0x01, 0x02, 0x03, 0x04, 0x05];
    let mut cur = Cursor::new(&data);
    let _ = cur.read_u8().unwrap(); // consume first byte
    let mut sub = cur.sub_cursor(3).unwrap();
    // sub should see bytes [0x02, 0x03, 0x04]
    assert_eq!(sub.read_u8().unwrap(), 0x02);
    assert_eq!(sub.read_u8().unwrap(), 0x03);
    assert_eq!(sub.read_u8().unwrap(), 0x04);
    assert_eq!(sub.remaining(), 0);
    // outer cursor should have advanced past the sub_cursor region
    assert_eq!(cur.consumed(), 4);
    assert_eq!(cur.remaining(), 1);
    assert_eq!(cur.read_u8().unwrap(), 0x05);
}

#[test]
fn test_cursor_sub_cursor_too_large() {
    let data = [0x01, 0x02];
    let mut cur = Cursor::new(&data);
    match cur.sub_cursor(5) {
        Err(Error::ShortBuffer) => {} // expected
        Err(e) => panic!("expected ShortBuffer, got {:?}", e),
        Ok(_) => panic!("expected error, got Ok"),
    }
}

// ═══════════════════════════════════════════════════════════════════════════
// Writer tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_writer_write_u8() {
    let mut buf = [0u8; 4];
    let mut w = Writer::new(&mut buf);
    w.write_u8(0xFF).unwrap();
    assert_eq!(w.written(), 1);
    assert_eq!(buf[0], 0xFF);
}

#[test]
fn test_writer_write_u16be() {
    let mut buf = [0u8; 4];
    let mut w = Writer::new(&mut buf);
    w.write_u16be(0xCAFE).unwrap();
    assert_eq!(w.written(), 2);
    assert_eq!(&buf[..2], &[0xCA, 0xFE]);
}

#[test]
fn test_writer_write_u16le() {
    let mut buf = [0u8; 4];
    let mut w = Writer::new(&mut buf);
    w.write_u16le(0xCAFE).unwrap();
    assert_eq!(w.written(), 2);
    assert_eq!(&buf[..2], &[0xFE, 0xCA]);
}

#[test]
fn test_writer_write_u32be() {
    let mut buf = [0u8; 4];
    let mut w = Writer::new(&mut buf);
    w.write_u32be(0xDEADBEEF).unwrap();
    assert_eq!(w.written(), 4);
    assert_eq!(&buf, &[0xDE, 0xAD, 0xBE, 0xEF]);
}

#[test]
fn test_writer_write_bytes() {
    let mut buf = [0u8; 8];
    let mut w = Writer::new(&mut buf);
    w.write_bytes(&[0x01, 0x02, 0x03]).unwrap();
    assert_eq!(w.written(), 3);
    assert_eq!(&buf[..3], &[0x01, 0x02, 0x03]);
}

#[test]
fn test_writer_short_buffer() {
    let mut buf = [0u8; 1];
    let mut w = Writer::new(&mut buf);
    let err = w.write_u16be(0x1234).unwrap_err();
    assert_eq!(err, Error::ShortBuffer);
}

#[test]
fn test_writer_overflow_require() {
    let mut buf = [0u8; 4];
    let mut w = Writer::new(&mut buf);
    w.write_u8(0x01).unwrap(); // pos = 1
    // Writing 4 bytes with only 3 remaining should fail with ShortBuffer
    let err = w.write_bytes(&[0; 4]).unwrap_err();
    assert_eq!(err, Error::ShortBuffer);
}

#[test]
fn test_writer_empty() {
    let mut buf = [0u8; 0];
    let mut w = Writer::new(&mut buf);
    assert_eq!(w.written(), 0);
    assert_eq!(w.write_u8(0x01).unwrap_err(), Error::ShortBuffer);
    assert_eq!(w.write_u16be(0x0102).unwrap_err(), Error::ShortBuffer);
    assert_eq!(w.write_u32be(0x01020304).unwrap_err(), Error::ShortBuffer);
    assert_eq!(w.write_bytes(&[0x01]).unwrap_err(), Error::ShortBuffer);
    // Writing empty bytes to empty buffer should succeed
    w.write_bytes(&[]).unwrap();
}

// ═══════════════════════════════════════════════════════════════════════════
// Round-trip tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_roundtrip_u8() {
    let mut buf = [0u8; 1];
    let mut w = Writer::new(&mut buf);
    w.write_u8(0x42).unwrap();

    let mut cur = Cursor::new(&buf);
    assert_eq!(cur.read_u8().unwrap(), 0x42);
}

#[test]
fn test_roundtrip_u16be() {
    let mut buf = [0u8; 2];
    let mut w = Writer::new(&mut buf);
    w.write_u16be(0xBEEF).unwrap();

    let mut cur = Cursor::new(&buf);
    assert_eq!(cur.read_u16be().unwrap(), 0xBEEF);
}

#[test]
fn test_roundtrip_u16le() {
    let mut buf = [0u8; 2];
    let mut w = Writer::new(&mut buf);
    w.write_u16le(0xBEEF).unwrap();

    let mut cur = Cursor::new(&buf);
    assert_eq!(cur.read_u16le().unwrap(), 0xBEEF);
}

#[test]
fn test_roundtrip_u32be() {
    let mut buf = [0u8; 4];
    let mut w = Writer::new(&mut buf);
    w.write_u32be(0xCAFEBABE).unwrap();

    let mut cur = Cursor::new(&buf);
    assert_eq!(cur.read_u32be().unwrap(), 0xCAFEBABE);
}

// ═══════════════════════════════════════════════════════════════════════════
// Checksum tests
// ═══════════════════════════════════════════════════════════════════════════

#[test]
fn test_internet_checksum_roundtrip() {
    // Build a buffer: 2 bytes payload + 2 bytes checksum field at offset 2
    let mut data = [0x00, 0x01, 0x00, 0x00, 0xF2, 0x03];
    let cksum_offset = 2;

    // Compute checksum — patches the field in-place
    internet_checksum_compute(&mut data, cksum_offset);

    // Verify: one's complement sum of the whole buffer should be 0x0000 (all ones folded)
    let verify = internet_checksum(&data);
    assert_eq!(
        verify, 0x0000,
        "internet checksum verify should return 0 for valid data"
    );
}

#[test]
fn test_crc32_roundtrip() {
    // Buffer: 4 bytes payload + 4 bytes checksum field at offset 4
    let mut data = [0xDE, 0xAD, 0xBE, 0xEF, 0x00, 0x00, 0x00, 0x00];
    let cksum_offset = 4;

    // Compute CRC-32 and store result in the checksum field
    let computed = crc32_compute(&mut data, cksum_offset);
    data[cksum_offset..cksum_offset + 4].copy_from_slice(&computed.to_le_bytes());

    // Verify: recompute skipping checksum field, should match stored value
    let verified = crc32_verify(&data, cksum_offset, 4);
    assert_eq!(
        verified, computed,
        "CRC-32 verify should match computed value"
    );
}

#[test]
fn test_crc32c_roundtrip() {
    let mut data = [0xCA, 0xFE, 0xBA, 0xBE, 0x00, 0x00, 0x00, 0x00];
    let cksum_offset = 4;

    let computed = crc32c_compute(&mut data, cksum_offset);
    data[cksum_offset..cksum_offset + 4].copy_from_slice(&computed.to_le_bytes());

    let verified = crc32c_verify(&data, cksum_offset, 4);
    assert_eq!(
        verified, computed,
        "CRC-32C verify should match computed value"
    );
}

#[test]
fn test_fletcher16_roundtrip() {
    let mut data = [0x01, 0x02, 0x00, 0x00, 0x03, 0x04];
    let cksum_offset = 2;

    let computed = fletcher16_compute(&mut data, cksum_offset);
    data[cksum_offset..cksum_offset + 2].copy_from_slice(&computed.to_le_bytes());

    let verified = fletcher16_verify(&data, cksum_offset, 2);
    assert_eq!(
        verified, computed,
        "Fletcher-16 verify should match computed value"
    );
}

#[test]
fn test_checksum_offset_at_end() {
    // Checksum field at the very end of the buffer
    let mut data = [0x01, 0x02, 0x03, 0x04, 0x00, 0x00, 0x00, 0x00];
    let cksum_offset = 4; // last 4 bytes

    let computed = crc32_compute(&mut data, cksum_offset);
    data[cksum_offset..cksum_offset + 4].copy_from_slice(&computed.to_le_bytes());

    let verified = crc32_verify(&data, cksum_offset, 4);
    assert_eq!(verified, computed);
}

#[test]
fn test_checksum_known_value() {
    // crc32_compute computes over the entire buffer (with checksum field zeroed).
    // For "123456789" followed by 4 zero bytes, the CRC-32 is 0xF3C1CE60.
    // (The classic 0xCBF43926 is CRC-32 of just "123456789" without trailing bytes.)
    let mut buf = Vec::new();
    buf.extend_from_slice(b"123456789");
    buf.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]); // checksum field at end
    let cksum_offset = 9;

    let computed = crc32_compute(&mut buf, cksum_offset);
    assert_eq!(
        computed, 0xF3C1CE60,
        "CRC-32 of '123456789' + 4 zeroed checksum bytes should be 0xF3C1CE60"
    );

    // Store the checksum and verify round-trip
    buf[cksum_offset..cksum_offset + 4].copy_from_slice(&computed.to_le_bytes());
    let verified = crc32_verify(&buf, cksum_offset, 4);
    assert_eq!(verified, computed);
}
