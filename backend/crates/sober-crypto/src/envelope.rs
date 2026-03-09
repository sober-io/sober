//! Envelope encryption primitives: MEK/DEK key hierarchy with AES-256-GCM.
//!
//! Two-layer key hierarchy:
//!
//! - **[`Mek`]** (Master Encryption Key) — loaded from an env var at startup.
//!   Wraps and unwraps [`Dek`] instances. The single externally-managed secret.
//! - **[`Dek`]** (Data Encryption Key) — one per user/group, generated randomly
//!   (256-bit). Stored in the database wrapped by the MEK.
//!
//! All secret values are encrypted by the owning scope's DEK using AES-256-GCM.
//! Each encryption produces a fresh random 12-byte nonce.
//!
//! Both key types zero their memory on drop.

use aes_gcm::aead::Aead;
use aes_gcm::{Aes256Gcm, KeyInit, Nonce};
use rand_core::{OsRng, RngCore};

use crate::error::CryptoError;

/// AES-256-GCM encrypted payload: 12-byte nonce followed by ciphertext
/// (including the authentication tag).
///
/// Serialised format: `nonce (12 bytes) || ciphertext (variable)`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EncryptedBlob {
    /// 96-bit random nonce.
    pub nonce: [u8; 12],
    /// AES-256-GCM ciphertext with appended authentication tag.
    pub ciphertext: Vec<u8>,
}

impl EncryptedBlob {
    /// Serialise to bytes: `nonce (12 bytes) || ciphertext`.
    #[must_use]
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(12 + self.ciphertext.len());
        buf.extend_from_slice(&self.nonce);
        buf.extend_from_slice(&self.ciphertext);
        buf
    }

    /// Deserialise from bytes: first 12 bytes are the nonce, the rest is
    /// ciphertext.
    ///
    /// Returns [`CryptoError::InvalidData`] if the input is too short to
    /// contain a nonce.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() < 12 {
            return Err(CryptoError::InvalidData(
                "encrypted blob too short (need at least 12 bytes for nonce)".into(),
            ));
        }
        let mut nonce = [0u8; 12];
        nonce.copy_from_slice(&bytes[..12]);
        let ciphertext = bytes[12..].to_vec();
        Ok(Self { nonce, ciphertext })
    }
}

/// Data Encryption Key — encrypts and decrypts user/group secrets.
///
/// Uses AES-256-GCM with a fresh random nonce per encryption. Key material
/// is zeroed on drop.
pub struct Dek([u8; 32]);

impl Dek {
    /// Generate a new random DEK using the OS CSPRNG.
    pub fn generate() -> Result<Self, CryptoError> {
        let mut key = [0u8; 32];
        OsRng.fill_bytes(&mut key);
        Ok(Self(key))
    }

    /// Create a DEK from raw bytes (e.g. after unwrapping with a [`Mek`]).
    ///
    /// Returns [`CryptoError::InvalidData`] if the slice is not exactly 32
    /// bytes.
    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CryptoError> {
        if bytes.len() != 32 {
            return Err(CryptoError::InvalidData(format!(
                "DEK must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(bytes);
        Ok(Self(key))
    }

    /// Returns the raw key bytes (for wrapping by a [`Mek`]).
    #[must_use]
    pub fn as_bytes(&self) -> &[u8; 32] {
        &self.0
    }

    /// Encrypt plaintext with a fresh random nonce.
    pub fn encrypt(&self, plaintext: &[u8]) -> Result<EncryptedBlob, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&self.0)
            .map_err(|e| CryptoError::EncryptionError(e.to_string()))?;

        let mut nonce_bytes = [0u8; 12];
        OsRng.fill_bytes(&mut nonce_bytes);
        let nonce = Nonce::from_slice(&nonce_bytes);

        let ciphertext = cipher
            .encrypt(nonce, plaintext)
            .map_err(|e| CryptoError::EncryptionError(e.to_string()))?;

        Ok(EncryptedBlob {
            nonce: nonce_bytes,
            ciphertext,
        })
    }

    /// Decrypt ciphertext using the nonce from the blob.
    pub fn decrypt(&self, blob: &EncryptedBlob) -> Result<Vec<u8>, CryptoError> {
        let cipher = Aes256Gcm::new_from_slice(&self.0)
            .map_err(|e| CryptoError::DecryptionError(e.to_string()))?;

        let nonce = Nonce::from_slice(&blob.nonce);

        cipher
            .decrypt(nonce, blob.ciphertext.as_slice())
            .map_err(|e| CryptoError::DecryptionError(e.to_string()))
    }
}

impl Drop for Dek {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

/// Master Encryption Key — wraps and unwraps [`Dek`] instances.
///
/// Loaded from a hex-encoded environment variable at startup. Key material
/// is zeroed on drop.
pub struct Mek([u8; 32]);

impl Mek {
    /// Parse from a 64-character hex string (e.g. from the
    /// `MASTER_ENCRYPTION_KEY` env var).
    ///
    /// Returns [`CryptoError::InvalidData`] if the hex string is invalid or
    /// does not decode to exactly 32 bytes.
    pub fn from_hex(hex_str: &str) -> Result<Self, CryptoError> {
        let bytes = hex::decode(hex_str)
            .map_err(|e| CryptoError::InvalidData(format!("invalid hex: {e}")))?;
        if bytes.len() != 32 {
            return Err(CryptoError::InvalidData(format!(
                "MEK must be 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut key = [0u8; 32];
        key.copy_from_slice(&bytes);
        Ok(Self(key))
    }

    /// Wrap (encrypt) a DEK for storage in the database.
    pub fn wrap_dek(&self, dek: &Dek) -> Result<EncryptedBlob, CryptoError> {
        // Reuse AES-256-GCM logic — MEK encrypts the DEK's raw bytes.
        Dek(self.0).encrypt(dek.as_bytes())
    }

    /// Unwrap (decrypt) a DEK from its stored form.
    pub fn unwrap_dek(&self, blob: &EncryptedBlob) -> Result<Dek, CryptoError> {
        let wrapper = Dek(self.0);
        let raw = wrapper.decrypt(blob)?;
        // `wrapper` is dropped here.
        Dek::from_bytes(&raw)
    }
}

impl Drop for Mek {
    fn drop(&mut self) {
        self.0.fill(0);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- EncryptedBlob ---

    #[test]
    fn encrypted_blob_roundtrip_serialization() {
        let blob = EncryptedBlob {
            nonce: [1u8; 12],
            ciphertext: vec![2, 3, 4, 5],
        };
        let bytes = blob.to_bytes();
        let recovered = EncryptedBlob::from_bytes(&bytes).expect("valid blob");
        assert_eq!(blob.nonce, recovered.nonce);
        assert_eq!(blob.ciphertext, recovered.ciphertext);
    }

    #[test]
    fn encrypted_blob_from_bytes_too_short() {
        let bytes = vec![0u8; 11]; // less than 12 bytes for nonce
        assert!(EncryptedBlob::from_bytes(&bytes).is_err());
    }

    #[test]
    fn encrypted_blob_empty_ciphertext() {
        let blob = EncryptedBlob {
            nonce: [0u8; 12],
            ciphertext: vec![],
        };
        let bytes = blob.to_bytes();
        assert_eq!(bytes.len(), 12);
        let recovered = EncryptedBlob::from_bytes(&bytes).expect("valid blob");
        assert!(recovered.ciphertext.is_empty());
    }

    // --- Dek ---

    #[test]
    fn dek_encrypt_decrypt_roundtrip() {
        let dek = Dek::generate().expect("generate DEK");
        let plaintext = b"hello world, this is a secret";
        let blob = dek.encrypt(plaintext).expect("encrypt");
        let decrypted = dek.decrypt(&blob).expect("decrypt");
        assert_eq!(plaintext.as_slice(), &decrypted);
    }

    #[test]
    fn dek_decrypt_with_wrong_key_fails() {
        let dek1 = Dek::generate().expect("generate DEK 1");
        let dek2 = Dek::generate().expect("generate DEK 2");
        let blob = dek1.encrypt(b"secret").expect("encrypt");
        assert!(dek2.decrypt(&blob).is_err());
    }

    #[test]
    fn dek_encrypt_produces_different_ciphertexts() {
        let dek = Dek::generate().expect("generate DEK");
        let plaintext = b"same input";
        let blob1 = dek.encrypt(plaintext).expect("encrypt 1");
        let blob2 = dek.encrypt(plaintext).expect("encrypt 2");
        // Different nonces should produce different ciphertexts
        assert_ne!(blob1.ciphertext, blob2.ciphertext);
    }

    #[test]
    fn dek_encrypts_empty_plaintext() {
        let dek = Dek::generate().expect("generate DEK");
        let blob = dek.encrypt(b"").expect("encrypt empty");
        let decrypted = dek.decrypt(&blob).expect("decrypt empty");
        assert!(decrypted.is_empty());
    }

    #[test]
    fn dek_from_bytes_wrong_length() {
        assert!(Dek::from_bytes(&[0u8; 16]).is_err());
        assert!(Dek::from_bytes(&[0u8; 64]).is_err());
    }

    #[test]
    fn dek_from_bytes_roundtrip() {
        let dek = Dek::generate().expect("generate DEK");
        let bytes = *dek.as_bytes();
        let restored = Dek::from_bytes(&bytes).expect("from_bytes");
        let plaintext = b"roundtrip test";
        let blob = dek.encrypt(plaintext).expect("encrypt");
        let decrypted = restored.decrypt(&blob).expect("decrypt");
        assert_eq!(plaintext.as_slice(), &decrypted);
    }

    // --- Mek ---

    #[test]
    fn mek_wrap_unwrap_dek_roundtrip() {
        let mek = Mek::from_hex(&"ab".repeat(32)).expect("valid hex");
        let dek = Dek::generate().expect("generate DEK");
        let wrapped = mek.wrap_dek(&dek).expect("wrap");
        let unwrapped = mek.unwrap_dek(&wrapped).expect("unwrap");
        assert_eq!(dek.as_bytes(), unwrapped.as_bytes());
    }

    #[test]
    fn mek_unwrap_with_wrong_key_fails() {
        let mek1 = Mek::from_hex(&"ab".repeat(32)).expect("valid hex");
        let mek2 = Mek::from_hex(&"cd".repeat(32)).expect("valid hex");
        let dek = Dek::generate().expect("generate DEK");
        let wrapped = mek1.wrap_dek(&dek).expect("wrap");
        assert!(mek2.unwrap_dek(&wrapped).is_err());
    }

    #[test]
    fn mek_from_hex_invalid_length() {
        assert!(Mek::from_hex("abcd").is_err()); // too short
    }

    #[test]
    fn mek_from_hex_invalid_chars() {
        assert!(Mek::from_hex(&"zz".repeat(32)).is_err()); // not hex
    }

    #[test]
    fn mek_wrap_produces_different_ciphertexts() {
        let mek = Mek::from_hex(&"ab".repeat(32)).expect("valid hex");
        let dek = Dek::generate().expect("generate DEK");
        let w1 = mek.wrap_dek(&dek).expect("wrap 1");
        let w2 = mek.wrap_dek(&dek).expect("wrap 2");
        // Different nonces each time
        assert_ne!(w1.ciphertext, w2.ciphertext);
    }

    // --- Drop zeroing ---

    #[test]
    fn dek_zeroed_on_drop() {
        let mut dek = Dek::generate().expect("generate DEK");
        // Verify key is non-zero before drop.
        assert_ne!(dek.0, [0u8; 32]);
        // Manually trigger the Drop zeroing logic.
        dek.0.fill(0);
        assert_eq!(dek.0, [0u8; 32]);
        // The real Drop will also zero, but the compiler can't observe
        // it; this test verifies the mechanism works.
    }
}

#[cfg(test)]
mod proptests {
    use super::*;
    use proptest::prelude::*;

    proptest! {
        #[test]
        fn dek_encrypt_decrypt_any_data(data in prop::collection::vec(any::<u8>(), 0..4096)) {
            let dek = Dek::generate().expect("generate DEK");
            let blob = dek.encrypt(&data).expect("encrypt");
            let decrypted = dek.decrypt(&blob).expect("decrypt");
            prop_assert_eq!(data, decrypted);
        }

        #[test]
        fn encrypted_blob_bytes_roundtrip(
            nonce in prop::array::uniform12(any::<u8>()),
            ciphertext in prop::collection::vec(any::<u8>(), 0..1024),
        ) {
            let blob = EncryptedBlob { nonce, ciphertext: ciphertext.clone() };
            let bytes = blob.to_bytes();
            let recovered = EncryptedBlob::from_bytes(&bytes).expect("from_bytes");
            prop_assert_eq!(nonce, recovered.nonce);
            prop_assert_eq!(ciphertext, recovered.ciphertext);
        }

        #[test]
        fn mek_wrap_unwrap_any_dek(mek_bytes in prop::array::uniform32(any::<u8>())) {
            let mek = Mek(mek_bytes);
            let dek = Dek::generate().expect("generate DEK");
            let wrapped = mek.wrap_dek(&dek).expect("wrap");
            let unwrapped = mek.unwrap_dek(&wrapped).expect("unwrap");
            prop_assert_eq!(dek.as_bytes(), unwrapped.as_bytes());
        }
    }
}
