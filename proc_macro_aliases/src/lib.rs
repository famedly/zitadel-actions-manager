// SPDX-FileCopyrightText: 2025 Famedly GmbH (info@famedly.com)
//
// SPDX-License-Identifier: Apache-2.0

//! Workaround for <https://github.com/tokio-rs/tracing/issues/2082>

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn instrument(_args: TokenStream, item: TokenStream) -> TokenStream {
    item
}
