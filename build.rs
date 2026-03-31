// SPDX-FileCopyrightText: 2026 Famedly GmbH
//
// SPDX-License-Identifier: Apache-2.0

//! Add build information (only for the CLI binary).
#![allow(clippy::expect_used)]

use std::error::Error;

fn main() -> Result<(), Box<dyn Error>> {
	// Only run vergen when building the CLI (main.rs); the lib doesn't use build
	// info and vergen can fail in environments without git (e.g. when used as a
	// dependency).
	if std::env::var("CARGO_FEATURE_CLI").is_ok() {
		vergen::EmitBuilder::builder().all_build().all_git().git_sha(false).emit()?;
	}
	Ok(())
}
