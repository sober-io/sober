//! Keypair management, envelope encryption, signing, and password hashing for
//! the Sober system.
//!
//! This crate provides the cryptographic primitives used across the Sober
//! backend. All operations use audited, pure-Rust libraries with no `unsafe`
//! code.
//!
//! # Modules
//!
//! - [`password`] — Argon2id password hashing and verification.
//! - [`keys`] — Ed25519 keypair generation, signing, and verification.
//! - [`error`] — [`CryptoError`](error::CryptoError) type with [`AppError`](sober_core::error::AppError) integration.

pub mod error;
pub mod keys;
pub mod password;
