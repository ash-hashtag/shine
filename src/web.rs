//! Browser entry point: `start("canvas-id", example)` from JavaScript.

use std::cell::RefCell;

use wasm_bindgen::JsCast;
use wasm_bindgen::prelude::*;

use crate::ShineApp;

thread_local! {
    /// The runner must outlive `start`; keep it for the life of the page.
    static RUNNER: RefCell<Option<eframe::WebRunner>> = const { RefCell::new(None) };
}

fn init_logging() {
    use tracing_subscriber::prelude::*;
    let layer = tracing_subscriber::fmt::layer()
        .with_ansi(false)
        .without_time()
        .with_writer(tracing_web::MakeWebConsoleWriter::new());
    // Ignore "already set" when the page restarts the app.
    let _ = tracing_subscriber::registry()
        .with(layer.with_filter(tracing_subscriber::filter::LevelFilter::INFO))
        .try_init();
}

/// Panics also reach the page: `window.shineFatal(message)` (defined by the
/// /app page) shows them instead of leaving a frozen canvas.
fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        console_error_panic_hook::hook(info);
        let report = web_sys::window()
            .and_then(|w| js_sys::Reflect::get(&w, &JsValue::from_str("shineFatal")).ok())
            .and_then(|f| f.dyn_into::<js_sys::Function>().ok());
        if let Some(report) = report {
            let _ = report.call1(&JsValue::NULL, &JsValue::from_str(&info.to_string()));
        }
    }));
}

/// WebGL2 unless `renderer=webgpu` is in the query. WebGPU is still uneven
/// across browsers (Firefox crashes the page with it), while WebGL2 works
/// everywhere eframe runs.
fn web_options(query: &str) -> eframe::WebOptions {
    let mut options = eframe::WebOptions::default();
    let webgpu = query
        .trim_start_matches('?')
        .split('&')
        .any(|p| p == "renderer=webgpu");
    if !webgpu
        && let eframe::egui_wgpu::WgpuSetup::CreateNew(setup) = &mut options.wgpu_options.wgpu_setup
    {
        setup.instance_descriptor.backends = eframe::wgpu::Backends::GL;
    }
    tracing::info!(
        "Renderer: {}",
        if webgpu {
            "WebGPU (with WebGL fallback)"
        } else {
            "WebGL2"
        }
    );
    options
}

/// Start Shine on the canvas with id `canvas_id`. `query` is the page's URL
/// query (see `Workspace::apply_url_query`), e.g. `?example=optical-dft`.
#[wasm_bindgen]
pub async fn start(canvas_id: String, query: Option<String>) -> Result<(), JsValue> {
    install_panic_hook();
    init_logging();
    let canvas = web_sys::window()
        .and_then(|w| w.document())
        .and_then(|d| d.get_element_by_id(&canvas_id))
        .ok_or_else(|| JsValue::from_str(&format!("no element with id {canvas_id}")))?
        .dyn_into::<web_sys::HtmlCanvasElement>()?;
    let runner = eframe::WebRunner::new();
    runner
        .start(
            canvas,
            web_options(query.as_deref().unwrap_or_default()),
            Box::new(move |cc| {
                let mut app = ShineApp::new(cc);
                if let Some(query) = query.as_deref() {
                    app.workspace.apply_url_query(query);
                }
                Ok(Box::new(app))
            }),
        )
        .await?;
    tracing::info!("Shine {} started", env!("CARGO_PKG_VERSION"));
    RUNNER.with(|r| *r.borrow_mut() = Some(runner));
    Ok(())
}

/// Offer `contents` to the user as a downloaded file.
pub fn download(name: &str, contents: &[u8]) -> Result<(), String> {
    let fail = |e: JsValue| format!("{e:?}");
    let document = web_sys::window()
        .and_then(|w| w.document())
        .ok_or("no document")?;
    let parts = js_sys::Array::of1(&js_sys::Uint8Array::from(contents));
    let options = web_sys::BlobPropertyBag::new();
    options.set_type("application/octet-stream");
    let blob =
        web_sys::Blob::new_with_u8_array_sequence_and_options(&parts, &options).map_err(fail)?;
    let url = web_sys::Url::create_object_url_with_blob(&blob).map_err(fail)?;
    let anchor: web_sys::HtmlAnchorElement = document
        .create_element("a")
        .map_err(fail)?
        .dyn_into()
        .map_err(|_| "not an anchor")?;
    anchor.set_href(&url);
    anchor.set_download(name);
    anchor.click();
    web_sys::Url::revoke_object_url(&url).map_err(fail)?;
    Ok(())
}
