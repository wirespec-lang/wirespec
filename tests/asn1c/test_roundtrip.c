/**
 * wirespec + asn1c integration roundtrip test.
 *
 * Copyright (c) 2024-2026 wirespec contributors.
 * SPDX-License-Identifier: MIT
 */

#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <assert.h>

/* asn1c generated headers */
#include "SimpleMessage.h"
#include "per_encoder.h"
#include "per_decoder.h"

/* wirespec generated header */
#include "asn1c_test.h"

/* wirespec runtime */
#include "wirespec_runtime.h"

int main(void) {
    /* Step 1: Create ASN.1 message with asn1c */
    SimpleMessage_t msg;
    memset(&msg, 0, sizeof(msg));
    msg.id = 42;
    msg.active = 1;

    /* Step 2: Encode with UPER */
    uint8_t asn1_buf[128];
    asn_enc_rval_t enc_ret = uper_encode_to_buffer(
        &asn_DEF_SimpleMessage, &msg, asn1_buf, sizeof(asn1_buf));
    assert(enc_ret.encoded > 0);
    size_t asn1_len = (size_t)((enc_ret.encoded + 7) / 8); /* bits to bytes */
    printf("ASN.1 UPER encoded: %zu bytes (%zd bits)\n", asn1_len, enc_ret.encoded);

    /* Step 3: Build wirespec packet wrapping the ASN.1 bytes */
    asn1c_test_as_n1_wrapper_t wrapper;
    memset(&wrapper, 0, sizeof(wrapper));
    wrapper.version = 1;
    wrapper.payload_length = (uint16_t)asn1_len;
    wrapper.payload.ptr = asn1_buf;
    wrapper.payload.len = asn1_len;

    /* Step 4: Serialize with wirespec */
    uint8_t wire_buf[256];
    size_t written = 0;
    wirespec_result_t rc = asn1c_test_as_n1_wrapper_serialize(
        &wrapper, wire_buf, sizeof(wire_buf), &written);
    assert(rc == WIRESPEC_OK);
    printf("wirespec serialized: %zu bytes\n", written);

    /* Step 5: Parse back with wirespec */
    asn1c_test_as_n1_wrapper_t parsed;
    memset(&parsed, 0, sizeof(parsed));
    size_t consumed = 0;
    rc = asn1c_test_as_n1_wrapper_parse(wire_buf, written, &parsed, &consumed);
    assert(rc == WIRESPEC_OK);
    assert(consumed == written);
    assert(parsed.version == 1);
    assert(parsed.payload_length == (uint16_t)asn1_len);
    assert(parsed.payload.len == asn1_len);

    /* Step 6: Decode ASN.1 payload with asn1c */
    SimpleMessage_t *decoded = NULL;
    asn_dec_rval_t dec_ret = uper_decode(
        NULL, &asn_DEF_SimpleMessage,
        (void **)&decoded,
        parsed.payload.ptr,
        parsed.payload.len,
        0, 0);
    assert(dec_ret.code == RC_OK);
    assert(decoded != NULL);
    assert(decoded->id == 42);
    assert(decoded->active == 1);

    printf("PASS: wirespec + asn1c roundtrip successful\n");
    printf("  Original: id=%ld active=%d\n", msg.id, msg.active);
    printf("  Decoded:  id=%ld active=%d\n", decoded->id, decoded->active);

    ASN_STRUCT_FREE(asn_DEF_SimpleMessage, decoded);
    return 0;
}
