//! Property-based tests for sober-crypto.

use proptest::prelude::*;
use sober_crypto::{keys, password};

// Password hashing is intentionally slow (~650ms per call), so fewer cases.
proptest! {
    #![proptest_config(ProptestConfig::with_cases(32))]

    #[test]
    fn arbitrary_passwords_hash_and_verify(pw in "\\PC{0,64}") {
        let hash = password::hash_password(&pw).unwrap();
        prop_assert!(password::verify_password(&pw, &hash).unwrap());
    }
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(256))]

    #[test]
    fn arbitrary_messages_sign_and_verify(msg in prop::collection::vec(any::<u8>(), 0..1024)) {
        let (signing, verifying) = keys::generate_keypair();
        let sig = keys::sign(&signing, &msg);
        prop_assert!(keys::verify(&verifying, &msg, &sig).is_ok());
    }

    #[test]
    fn wrong_key_always_fails_verification(msg in prop::collection::vec(any::<u8>(), 0..1024)) {
        let (signing, _) = keys::generate_keypair();
        let (_, wrong_verifying) = keys::generate_keypair();
        let sig = keys::sign(&signing, &msg);
        prop_assert!(keys::verify(&wrong_verifying, &msg, &sig).is_err());
    }
}
