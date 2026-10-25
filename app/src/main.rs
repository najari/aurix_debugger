//! aurix-debugger: a debugger window for the probe-rs DAP server (the standalone debugger, B).
//!
//! ```text
//! aurix-debugger <attach.json> [server, default 127.0.0.1:50000]
//! ```
//!
//! The window connects to the server (`probe-rs dap-server --port 50000`), attaches with the
//! arguments in the JSON file, and shows the threads (one per core), the call stack of the
//! selected thread, the disassembly around the selected frame, and the console.

mod debugger;

use gpui::{App, Bounds, TitlebarOptions, WindowBounds, WindowOptions, prelude::*, px, size};
use gpui_platform::application;

fn main() {
    let mut args = std::env::args().skip(1);
    let attach = args.next();
    let server = args.next().unwrap_or_else(|| "127.0.0.1:50000".to_string());

    application().run(move |cx: &mut App| {
        let bounds = Bounds::centered(None, size(px(1280.), px(800.)), cx);
        cx.open_window(
            WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(TitlebarOptions {
                    title: Some("AURIX Debugger".into()),
                    ..Default::default()
                }),
                ..Default::default()
            },
            |window, cx| {
                let debugger =
                    cx.new(|cx| debugger::Debugger::new(attach.clone(), server.clone(), cx));
                let handle = debugger.downgrade();
                window.on_window_should_close(cx, move |_, cx| {
                    handle.update(cx, |debugger, _| debugger.disconnect()).ok();
                    true
                });
                debugger
            },
        )
        .expect("the window opens");
        cx.on_window_closed(|cx, _| cx.quit()).detach();
        cx.activate(true);
    });
}
