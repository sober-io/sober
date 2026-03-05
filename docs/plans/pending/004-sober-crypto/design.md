# 004 — sober-crypto

**Date:** 2026-03-06

---

## Password Hashing

- Argon2id via the `argon2` crate (pure Rust, uses `password-hash` traits).
- Parameters: 19 MiB memory (m=19456), 2 iterations (t=2), 1 parallelism (p=1) — OWASP recommended.
- Output: PHC string format (`$argon2id$v=19$m=19456,t=2,p=1$salt$hash`) — stores algorithm, params, salt, and hash in one string.
- API:
  - `hash_password(password: &str) -> Result<String, CryptoError>`
  - `verify_password(password: &str, hash: &str) -> Result<bool, CryptoError>`
- Salt: 16 bytes from OS CSPRNG via `rand`.
- Constant-time comparison (built into the argon2 crate's verify).

## Ed25519 Keypairs

- Via `ed25519-dalek` crate with `rand` feature.
- Used for: future replica authentication, message signing (not in v1 agent loop, but the primitives are ready).
- API:
  - `generate_keypair() -> (SigningKey, VerifyingKey)`
  - `sign(key: &SigningKey, message: &[u8]) -> Signature`
  - `verify(key: &VerifyingKey, message: &[u8], signature: &Signature) -> Result<(), CryptoError>`
- Key serialization: raw bytes (32 bytes each for signing/verifying keys).

## Injection Detection (stub)

- v1 is a logging-only stub — detects patterns but does not block.
- API: `check_injection(input: &str) -> InjectionReport` where `InjectionReport` contains:
  - `suspicious: bool`
  - `patterns: Vec<String>` (matched pattern names)
  - `score: f32` (0.0--1.0 confidence)
- Pattern matching: simple string matching against known injection prefixes ("ignore previous instructions", "system prompt:", etc.) and unusual Unicode/control characters.
- Full ML-based detection deferred to post-v1.

## Error Type

- `CryptoError` enum with `thiserror`: `HashError`, `VerificationError`, `KeyGenerationError`, `SignatureError`.
- Implements `From<CryptoError>` for `AppError` (maps to `Internal`).

## Dependencies

- `argon2` (with `std` feature)
- `ed25519-dalek` (with `rand_core` feature)
- `rand` (for CSPRNG)
- `thiserror`
- `serde` (for serializing injection reports)
- `sober-core` (for `AppError`, types)
