// Per-browser CEF request context — currently just the default,
// unoverridden handler (an empty `impl RequestContextHandler {}` block
// still has to go through `wrap_request_context_handler!` to produce a
// real `cef::RequestContextHandler`, which `browser::spawn` needs to
// hand `cef::request_context_create_context` when creating each page).

use cef::{self, rc::Rc, *};

#[derive(Clone)]
pub struct OsrRequestContextHandler {}

wrap_request_context_handler! {
    pub struct RequestContextHandlerBuilder {
        handler: OsrRequestContextHandler,
    }

    impl RequestContextHandler {}
}

impl RequestContextHandlerBuilder {
    pub fn build(handler: OsrRequestContextHandler) -> RequestContextHandler {
        Self::new(handler)
    }
}
