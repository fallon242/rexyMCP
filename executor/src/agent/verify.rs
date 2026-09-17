//! The verifier seam. The loop depends on this trait, not on
//! `governor::verifier`'s free functions, so tests can inject a deterministic
//! mock instead of spawning a real compiler (`cargo`/`tsc`/`ruff`).

use std::path::{Path, PathBuf};

use async_trait::async_trait;

use crate::governor::verifier::{self, Baseline, VerifierResult};

/// Post-edit verification + session-start baseline capture, behind a trait for
/// test injection.
#[async_trait]
pub trait FileVerifier: Send + Sync {
    async fn verify(&self, path: &Path) -> VerifierResult;
    async fn capture_baseline(&self, paths: &[PathBuf]) -> Baseline;
}

/// Cloud-executor verifier: the same checks, run inside the bash sandbox.
pub struct SandboxedVerifier {
    pub sandbox: crate::security::Sandbox,
}

#[async_trait]
impl FileVerifier for SandboxedVerifier {
    async fn verify(&self, path: &Path) -> VerifierResult {
        verifier::verify_in(path, Some(&self.sandbox)).await
    }

    async fn capture_baseline(&self, paths: &[PathBuf]) -> Baseline {
        verifier::capture_baseline_in(paths, Some(&self.sandbox)).await
    }
}

/// The production verifier — delegates to `governor::verifier`, which shells out
/// to the per-language checker.
pub struct RealVerifier;

#[async_trait]
impl FileVerifier for RealVerifier {
    async fn verify(&self, path: &Path) -> VerifierResult {
        verifier::verify(path).await
    }

    async fn capture_baseline(&self, paths: &[PathBuf]) -> Baseline {
        verifier::capture_baseline(paths).await
    }
}
