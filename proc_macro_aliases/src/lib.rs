//! Workaround for <https://github.com/tokio-rs/tracing/issues/2082>

use proc_macro::TokenStream;

#[proc_macro_attribute]
pub fn instrument(_args: TokenStream, item: TokenStream) -> TokenStream {
    item
}
