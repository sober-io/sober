//! Shared test helpers for all downstream crates.
//!
//! Enable with `sober-core = { ..., features = ["test-utils"] }` in
//! `[dev-dependencies]`.

// TODO(plan-003): test_db() — create test PostgreSQL pool, run migrations, transaction-per-test
// TODO(plan-003): test_config() — AppConfig with test defaults
// TODO(plan-008): MockLlmEngine — mock LLM engine for testing
// TODO(plan-012): MockGrpcServer — mock gRPC server for testing
