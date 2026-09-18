//! The page's side of the browser host: a canvas and the page's clock go in, frames come out.
//! Glue only — every decision is the engine's.
#![cfg(target_arch = "wasm32")]

use wasm_bindgen::prelude::*;

/// One shader node drawn on a canvas.
#[wasm_bindgen]
pub struct Stage(goofi_graphics::web::Browser);

/// Build `source` as the node `type_name` and draw it on `canvas`; `tick` paces it.
#[wasm_bindgen]
pub async fn start(canvas: web_sys::HtmlCanvasElement, type_name: &str, source: &str) -> Result<Stage, JsError> {
    // A wasm panic is a bare trap; the page's console is the only place its message can go.
    std::panic::set_hook(Box::new(|info| web_sys::console::error_1(&info.to_string().into())));
    goofi_graphics::web::Browser::open(canvas, type_name, source).await.map(Stage).map_err(|e| JsError::new(&e))
}

#[wasm_bindgen]
impl Stage {
    /// One frame at `now` seconds of the page's clock.
    pub fn tick(&mut self, now: f64) -> Result<(), JsError> {
        self.0.tick(now).map_err(|e| JsError::new(&e))
    }
}
