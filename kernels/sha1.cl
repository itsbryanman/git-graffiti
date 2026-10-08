uint rol(uint value, uint bits) {
    return rotate(value, bits);
}

void compress(uint state[5], uint words[80]) {
    for (uint i = 16; i < 80; i++) {
        words[i] = rol(words[i - 3] ^ words[i - 8] ^ words[i - 14] ^ words[i - 16], 1);
    }

    uint a = state[0];
    uint b = state[1];
    uint c = state[2];
    uint d = state[3];
    uint e = state[4];
    for (uint i = 0; i < 80; i++) {
        uint f;
        uint k;
        if (i < 20) {
            f = (b & c) | ((~b) & d);
            k = 0x5a827999U;
        } else if (i < 40) {
            f = b ^ c ^ d;
            k = 0x6ed9eba1U;
        } else if (i < 60) {
            f = (b & c) | (b & d) | (c & d);
            k = 0x8f1bbcdcU;
        } else {
            f = b ^ c ^ d;
            k = 0xca62c1d6U;
        }
        uint next = rol(a, 5) + f + e + k + words[i];
        e = d;
        d = c;
        c = rol(b, 30);
        b = a;
        a = next;
    }

    state[0] += a;
    state[1] += b;
    state[2] += c;
    state[3] += d;
    state[4] += e;
}

uchar digest_byte(uint state[5], uint index) {
    uint word = state[index / 4];
    return (uchar)(word >> (24 - (index % 4) * 8));
}

__kernel void mine(
    __global const uint *initial,
    __global const uchar *target,
    uint prefix_nibbles,
    ulong input_bit_length,
    ulong start_nonce,
    volatile __global int *found,
    __global ulong *found_nonce
) {
    if (*found != 0) {
        return;
    }

    ulong nonce = start_nonce + (ulong)get_global_id(0);
    uint state[5];
    for (uint i = 0; i < 5; i++) {
        state[i] = initial[i];
    }

    uint words[80];
    for (uint word = 0; word < 16; word++) {
        uint value = 0;
        for (uint byte = 0; byte < 4; byte++) {
            uint bit = word * 4 + byte;
            uchar encoded = ((nonce >> bit) & 1UL) ? '\t' : ' ';
            value |= ((uint)encoded) << (24 - byte * 8);
        }
        words[word] = value;
    }
    compress(state, words);

    for (uint i = 0; i < 16; i++) {
        words[i] = 0;
    }
    words[0] = 0x80000000U;
    words[14] = (uint)(input_bit_length >> 32);
    words[15] = (uint)input_bit_length;
    compress(state, words);

    uint whole_bytes = prefix_nibbles / 2;
    for (uint i = 0; i < whole_bytes; i++) {
        if (digest_byte(state, i) != target[i]) {
            return;
        }
    }
    if ((prefix_nibbles & 1) != 0) {
        if ((digest_byte(state, whole_bytes) & 0xf0) != (target[whole_bytes] & 0xf0)) {
            return;
        }
    }

    if (atomic_cmpxchg(found, 0, 1) == 0) {
        *found_nonce = nonce;
    }
}
