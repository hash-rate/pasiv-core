# verushash-rs

A Rust implementation and wrapper for the **VerusHash** algorithm, providing native bindings to the cryptographic hashing functions used by the VerusCoin blockchain.  
This crate is based on the official [`verushash-node`](https://github.com/VerusCoin/verushash-node) implementation and offers idiomatic Rust access to all major VerusHash versions.

---

## Features

- Implements VerusHash **V1, V2, V2.1, and V2.2**.
- Simple, safe Rust API with fixed-size `[u8; 32]` digests.
- Suitable for mining software, blockchain integrations, and verification tools.
- Designed to be easy to integrate into existing Rust projects.

---

## Installation

Add this crate to your `Cargo.toml`:

```toml
[dependencies]
verushash = "0.1"
```

Then bring the functions into scope:

```rust
use verushash::{verus_hash_v1, verus_hash_v2, verus_hash_v2_1, verus_hash_v2_2};
```

## Usage

Basic usage example demonstrating all supported versions:

```rust
use verushash::{verus_hash_v1, verus_hash_v2, verus_hash_v2_1, verus_hash_v2_2};

fn main() {
    let data = b"VerusHash Example";

    let hash_v1 = verus_hash_v1(data);
    let hash_v2 = verus_hash_v2(data);
    let hash_v2_1 = verus_hash_v2_1(data);
    let hash_v2_2 = verus_hash_v2_2(data);

    println!("VerusHash V1:   {}", hex::encode(hash_v1));
    println!("VerusHash V2:   {}", hex::encode(hash_v2));
    println!("VerusHash V2.1: {}", hex::encode(hash_v2_1));
    println!("VerusHash V2.2: {}", hex::encode(hash_v2_2));
}
```
Make sure to include the `hex` crate if you want hexadecimal output:

```rust
[dependencies]
verushash = "0.1"
hex = "0.4"
```

## API

| Function                                          | Description                       |
|---------------------------------------------------|-----------------------------------|
| `pub fn verus_hash_v1(data: &[u8]) -> [u8; 32]`   | Computes a VerusHash V1 digest.   |
| `pub fn verus_hash_v2(data: &[u8]) -> [u8; 32]`   | Computes a VerusHash V2 digest.   |
| `pub fn verus_hash_v2_1(data: &[u8]) -> [u8; 32]` | Computes a VerusHash V2.1 digest. |
| `pub fn verus_hash_v2_2(data: &[u8]) -> [u8; 32]` | Computes a VerusHash V2.2 digest. |

All functions return a 32-byte (256-bit) hash as a fixed-size array.

---

## Acknowledgements

This project is inspired by and derived from the official [VerusCoin `verushash-node` repository](https://github.com/VerusCoin/verushash-node).  
All credit for the original VerusHash algorithm and reference implementation goes to the VerusCoin developers.

---

## License

This project is licensed under the MIT License.  
See the `LICENSE` file for details.


