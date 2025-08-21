// SPDX-FileCopyrightText: 2025 Famedly GmbH (info@famedly.com)
//
// SPDX-License-Identifier: Apache-2.0

//! Add build information.
#![allow(clippy::expect_used)]

use std::error::Error;

use vergen::EmitBuilder;

fn main() -> Result<(), Box<dyn Error>> {
    EmitBuilder::builder().fail_on_error().all_build().all_git().git_sha(false).emit()?;
    Ok(())
}
