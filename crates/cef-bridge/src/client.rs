// Ties every handler in this crate together into the one `cef::Client`
// CEF hands each spawned browser. `render_handler` is per-page (built
// from the `OsrRenderHandler` browser.rs constructs for that specific
// page's texture slot); every other handler is stateless and shared.

use crate::display::{DisplayHandlerBuilder, OsrDisplayHandler};
use crate::download::{DownloadHandlerBuilder, OsrDownloadHandler};
use crate::life_span::{LifeSpanHandlerBuilder, OsrLifeSpanHandler};
use crate::load::{LoadHandlerBuilder, OsrLoadHandler};
use crate::navigation::{OsrRequestHandler, RequestHandlerBuilder};
use crate::render::{OsrRenderHandler, RenderHandlerBuilder};
use cef::{self, rc::Rc, *};

wrap_client! {
    pub struct ClientBuilder {
        render_handler: RenderHandler,
        display_handler: DisplayHandler,
        request_handler: cef::RequestHandler,
        download_handler: cef::DownloadHandler,
        life_span_handler: cef::LifeSpanHandler,
        load_handler: cef::LoadHandler,
    }

    impl Client {
        fn render_handler(&self) -> Option<cef::RenderHandler> {
            Some(self.render_handler.clone())
        }

        fn display_handler(&self) -> Option<cef::DisplayHandler> {
            Some(self.display_handler.clone())
        }

        fn request_handler(&self) -> Option<cef::RequestHandler> {
            Some(self.request_handler.clone())
        }

        fn download_handler(&self) -> Option<cef::DownloadHandler> {
            Some(self.download_handler.clone())
        }

        fn life_span_handler(&self) -> Option<cef::LifeSpanHandler> {
            Some(self.life_span_handler.clone())
        }

        fn load_handler(&self) -> Option<cef::LoadHandler> {
            Some(self.load_handler.clone())
        }
    }
}

impl ClientBuilder {
    pub fn build(render_handler: OsrRenderHandler) -> Client {
        Self::new(
            RenderHandlerBuilder::build(render_handler),
            DisplayHandlerBuilder::build(OsrDisplayHandler {}),
            RequestHandlerBuilder::build(OsrRequestHandler {}),
            DownloadHandlerBuilder::build(OsrDownloadHandler {}),
            LifeSpanHandlerBuilder::build(OsrLifeSpanHandler {}),
            LoadHandlerBuilder::build(OsrLoadHandler {}),
        )
    }
}
