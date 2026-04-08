#!/bin/bash
# wirespec + asn1c end-to-end integration test.
#
# Verifies that wirespec-generated C code can wrap and unwrap ASN.1 payloads
# that are encoded/decoded by the asn1c library.
#
# Prerequisites: asn1c, gcc, wirespec (built in release mode).
#
# Copyright (c) 2024-2026 wirespec contributors.
# SPDX-License-Identifier: MIT

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
WIRESPEC_ROOT="$(cd "$SCRIPT_DIR/../.." && pwd)"
WIRESPEC="$WIRESPEC_ROOT/target/release/wirespec"
BUILD_DIR="/tmp/wirespec-asn1c-test"

echo "=== wirespec + asn1c Integration Test ==="

# Sanity checks
if ! command -v asn1c >/dev/null 2>&1; then
    echo "SKIP: asn1c not found" >&2
    exit 0
fi
if [ ! -x "$WIRESPEC" ]; then
    echo "ERROR: wirespec not found at $WIRESPEC (run 'cargo build --release' first)" >&2
    exit 1
fi

# Clean build directory
rm -rf "$BUILD_DIR"
mkdir -p "$BUILD_DIR/asn1c" "$BUILD_DIR/wirespec"

# Step 1: Compile ASN.1 with asn1c (need -pdu to get PER support files)
echo "1. Compiling ASN.1 with asn1c..."
cd "$BUILD_DIR/asn1c"
asn1c -fcompound-names -gen-PER -pdu=SimpleMessage "$SCRIPT_DIR/test_schema.asn1" 2>/dev/null
# Remove the converter-sample.c that asn1c generates (it has its own main)
rm -f converter-sample.c
echo "   Generated $(ls *.c 2>/dev/null | wc -l) C files"

# Step 2: Compile .wspec with wirespec
echo "2. Compiling .wspec with wirespec..."
"$WIRESPEC" compile "$SCRIPT_DIR/asn1c_wrapper.wspec" -t c -o "$BUILD_DIR/wirespec"
echo "   Generated wirespec C files"

# Step 3: Compile test program
echo "3. Building test program..."
gcc -Wall -Wextra -std=c11 \
    -I "$BUILD_DIR/asn1c" \
    -I "$BUILD_DIR/wirespec" \
    -I "$WIRESPEC_ROOT/runtime" \
    -o "$BUILD_DIR/test_roundtrip" \
    "$SCRIPT_DIR/test_roundtrip.c" \
    "$BUILD_DIR/wirespec"/asn1c_test.c \
    "$BUILD_DIR/asn1c"/*.c \
    -lm
echo "   Build successful"

# Step 4: Run test
echo "4. Running roundtrip test..."
"$BUILD_DIR/test_roundtrip"

echo "=== All tests passed ==="
