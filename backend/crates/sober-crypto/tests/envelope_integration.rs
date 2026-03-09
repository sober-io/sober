//! Integration tests for the full envelope encryption flow.
//!
//! Simulates the complete lifecycle: MEK loaded from env -> DEK generated
//! for a user -> secrets encrypted/decrypted -> DEK rotation.

use sober_crypto::envelope::{Dek, EncryptedBlob, Mek};

#[test]
fn full_envelope_encryption_roundtrip() {
    // Simulate: system startup loads MEK from hex env var
    let mek = Mek::from_hex(&"deadbeef".repeat(8)).expect("valid MEK hex");

    // Simulate: new user created, generate their DEK
    let user_dek = Dek::generate().expect("generate DEK");

    // Simulate: wrap DEK for storage in DB
    let wrapped_dek = mek.wrap_dek(&user_dek).expect("wrap DEK");
    let wrapped_bytes = wrapped_dek.to_bytes(); // what goes into the encryption_keys table

    // Simulate: user stores a secret (e.g. an LLM API key)
    let secret_json = serde_json::json!({"api_key": "sk-ant-1234"});
    let plaintext = serde_json::to_vec(&secret_json).expect("serialize secret");
    let encrypted_secret = user_dek.encrypt(&plaintext).expect("encrypt secret");
    let encrypted_bytes = encrypted_secret.to_bytes(); // what goes into user_secrets table

    // --- Later: load and decrypt the secret ---

    let loaded_wrapped_dek = EncryptedBlob::from_bytes(&wrapped_bytes).expect("parse wrapped DEK");
    let loaded_dek = mek.unwrap_dek(&loaded_wrapped_dek).expect("unwrap DEK");

    let loaded_secret =
        EncryptedBlob::from_bytes(&encrypted_bytes).expect("parse encrypted secret");
    let decrypted = loaded_dek.decrypt(&loaded_secret).expect("decrypt secret");

    let recovered: serde_json::Value =
        serde_json::from_slice(&decrypted).expect("deserialize secret");
    assert_eq!(recovered["api_key"], "sk-ant-1234");
}

#[test]
fn dek_rotation_preserves_secrets() {
    let mek = Mek::from_hex(&"aa".repeat(32)).expect("valid MEK hex");

    // Old DEK with existing secrets
    let old_dek = Dek::generate().expect("generate old DEK");
    let secret = old_dek
        .encrypt(b"my secret data")
        .expect("encrypt with old DEK");

    // Rotate: decrypt with old DEK, re-encrypt with new DEK
    let plaintext = old_dek.decrypt(&secret).expect("decrypt with old DEK");
    let new_dek = Dek::generate().expect("generate new DEK");
    let re_encrypted = new_dek.encrypt(&plaintext).expect("encrypt with new DEK");

    // Verify new DEK can decrypt
    let result = new_dek
        .decrypt(&re_encrypted)
        .expect("decrypt with new DEK");
    assert_eq!(result, b"my secret data");

    // Old DEK cannot decrypt new ciphertext
    assert!(old_dek.decrypt(&re_encrypted).is_err());

    // Wrap new DEK with MEK and verify full chain
    let wrapped = mek.wrap_dek(&new_dek).expect("wrap new DEK");
    let unwrapped = mek.unwrap_dek(&wrapped).expect("unwrap new DEK");
    let final_result = unwrapped
        .decrypt(&re_encrypted)
        .expect("decrypt with unwrapped DEK");
    assert_eq!(final_result, b"my secret data");
}

#[test]
fn multiple_users_independent_deks() {
    let mek = Mek::from_hex(&"bb".repeat(32)).expect("valid MEK hex");

    // Alice's DEK and secret
    let alice_dek = Dek::generate().expect("generate Alice DEK");
    let alice_secret = alice_dek
        .encrypt(b"alice's api key")
        .expect("encrypt Alice secret");

    // Bob's DEK and secret
    let bob_dek = Dek::generate().expect("generate Bob DEK");
    let bob_secret = bob_dek
        .encrypt(b"bob's api key")
        .expect("encrypt Bob secret");

    // Each can only decrypt their own
    assert!(bob_dek.decrypt(&alice_secret).is_err());
    assert!(alice_dek.decrypt(&bob_secret).is_err());

    // Each can decrypt their own
    let alice_plain = alice_dek.decrypt(&alice_secret).expect("Alice decrypt");
    let bob_plain = bob_dek.decrypt(&bob_secret).expect("Bob decrypt");
    assert_eq!(alice_plain, b"alice's api key");
    assert_eq!(bob_plain, b"bob's api key");

    // Both DEKs can be wrapped with the same MEK
    let _alice_wrapped = mek.wrap_dek(&alice_dek).expect("wrap Alice DEK");
    let _bob_wrapped = mek.wrap_dek(&bob_dek).expect("wrap Bob DEK");
}
