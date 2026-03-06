# 004 — sober-crypto: Implementation Plan

## Steps

1. Add dependencies to `backend/crates/sober-crypto/Cargo.toml`.
2. Create module structure: `src/lib.rs`, `src/error.rs`, `src/password.rs`, `src/keys.rs`.
   (No `injection.rs` --- injection detection lives in `sober-mind`.)
3. Implement `error.rs`: `CryptoError` enum, `From<CryptoError>` for `AppError`.
4. Implement `password.rs`: `hash_password`, `verify_password` functions.
5. Implement `keys.rs`: `generate_keypair`, `sign`, `verify` functions.
6. Write unit tests:
   - Password: hash then verify succeeds, wrong password fails, hash format is PHC.
   - Keys: generate + sign + verify roundtrip, wrong key rejects signature.
7. Write property-based tests (proptest):
   - Arbitrary passwords hash and verify correctly.
   - Arbitrary messages sign and verify correctly.
   - Signing with wrong key always fails verification.
8. Run `cargo clippy -- -D warnings` and `cargo test -p sober-crypto`.

## Acceptance Criteria

- All unit tests pass.
- Property-based tests pass with default proptest config (256 cases).
- `cargo clippy` clean.
- No `unsafe` code in the crate.
- Password hashing takes measurable time (>100ms) confirming Argon2id parameters are applied.
