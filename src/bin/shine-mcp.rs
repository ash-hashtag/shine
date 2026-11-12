//! MCP server over stdio: `shine-mcp` (logs to stderr; `--log-file` also works).

use shine::logging::{self, LogConfig};

/// Faster than the system allocator when simulating on every thread.
#[cfg(not(target_arch = "wasm32"))]
#[global_allocator]
static ALLOCATOR: mimalloc::MiMalloc = mimalloc::MiMalloc;

fn main() -> std::io::Result<()> {
    let _log_guard = logging::init(&LogConfig {
        ansi: false,
        ..LogConfig::from_env()
    })
    .map_err(|e| eprintln!("shine-mcp: logging disabled: {e}"))
    .ok();
    tracing::info!(
        "Shine MCP server {} ready on stdio",
        env!("CARGO_PKG_VERSION")
    );
    let stdin = std::io::stdin().lock();
    let stdout = std::io::stdout().lock();
    let result = shine::mcp::serve(stdin, stdout);
    tracing::info!("Client disconnected");
    result
}
