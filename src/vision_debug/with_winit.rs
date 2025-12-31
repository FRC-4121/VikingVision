use std::collections::HashMap;
use std::num::NonZero;

use crate::buffer::PixelFormat;

use super::*;
use winit::application::ApplicationHandler;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, EventLoop, EventLoopProxy, OwnedDisplayHandle};
use winit::window::{Window, WindowId};

#[derive(Clone)]
enum SenderInner {
    Winit(EventLoopProxy<Message>),
    Fallback(without_winit::Sender),
}

/// A sender to a handler.
///
/// This can be used from any thread, but the handler needs to be on the main thread.
#[derive(Clone)]
pub struct Sender(SenderInner);
impl Sender {
    pub fn send(&self, msg: Message) {
        match &self.0 {
            SenderInner::Winit(proxy) => drop(proxy.send_event(msg)),
            SenderInner::Fallback(fallback) => fallback.send(msg),
        }
    }
}

fn format_title(title: Option<&str>, default_title: &str, name: &str, id: u128) -> String {
    let pattern = match title {
        None | Some("") => default_title,
        Some(s) => s,
    };
    if pattern.is_empty() {
        format!("{name} ({id:0>32x})")
    } else {
        pattern
            .replace("%N", name)
            .replace("%i", &format!("{id:0>32x}"))
    }
}

enum DebugKind {
    Ignore,
    Ffmpeg(without_winit::FfmpegProcess),
    Window(
        softbuffer::Surface<OwnedDisplayHandle, Window>,
        Option<String>,
        String,
    ),
}

struct WinitHandler {
    default: DefaultDebug,
    context: softbuffer::Context<OwnedDisplayHandle>,
    windows: HashMap<WindowId, u128>,
    debugs: HashMap<u128, DebugKind>,
}
impl ApplicationHandler<Message> for WinitHandler {
    fn resumed(&mut self, _event_loop: &ActiveEventLoop) {}
    fn window_event(
        &mut self,
        _event_loop: &ActiveEventLoop,
        window_id: WindowId,
        event: WindowEvent,
    ) {
        if matches!(event, WindowEvent::CloseRequested | WindowEvent::Destroyed)
            && let Some(debug_id) = self.windows.remove(&window_id)
            && let Some(debug) = self.debugs.get_mut(&debug_id)
            && let DebugKind::Window(..) = debug
        {
            *debug = DebugKind::Ignore;
        }
    }
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: Message) {
        match event {
            Message::Shutdown => {
                self.debugs.clear();
                self.windows.clear();
                event_loop.exit();
            }
            Message::DebugImage(DebugImage {
                mut image,
                name,
                id,
                mut mode,
            }) => {
                let dbg = self.debugs.entry(id).or_insert_with(|| {
                    if mode == DebugMode::Auto {
                        mode = self.default.mode.map_or(DebugMode::None, From::from);
                    }
                    match mode {
                        DebugMode::Auto => unreachable!(),
                        DebugMode::None => DebugKind::Ignore,
                        DebugMode::Save { path } => without_winit::create_ffmpeg_command(
                            path.as_deref(),
                            &self.default.default_path,
                            &name,
                            id,
                            image.width,
                            image.height,
                            image.format,
                        )
                        .map_or(DebugKind::Ignore, DebugKind::Ffmpeg),
                        DebugMode::Show { mut title } => {
                            let res = event_loop.create_window(
                                Window::default_attributes()
                                    .with_title(format_title(
                                        title.as_deref(),
                                        &self.default.default_title,
                                        &name,
                                        id,
                                    ))
                                    .with_resizable(false),
                            );
                            match res {
                                Ok(window) => match softbuffer::Surface::new(&self.context, window) {
                                    Ok(surface) => DebugKind::Window(surface, title.take(), name.clone()),
                                    Err(err) => {
                                    tracing::error!(%err, "failed to create softbuffer surface");
                                        DebugKind::Ignore
                                    }
                                },
                                Err(err) => {
                                    tracing::error!(%err, "failed to create window");
                                    DebugKind::Ignore
                                }
                            }
                        }
                    }
                });
                match dbg {
                    DebugKind::Ignore => {}
                    DebugKind::Ffmpeg(proc) => proc.accept(image),
                    DebugKind::Window(surface, title, last_name) => {
                        if name != *last_name {
                            surface.window().set_title(&format_title(
                                title.as_deref(),
                                &self.default.default_title,
                                &name,
                                id,
                            ));
                        }
                        let res = surface.resize(
                            NonZero::new(image.width).unwrap_or(ONE),
                            NonZero::new(image.height).unwrap_or(ONE),
                        );
                        if let Err(err) = res {
                            tracing::error!(%err, "failed to resize buffer");
                            *dbg = DebugKind::Ignore;
                            return;
                        }
                        let mut had_err = false;
                        match surface.buffer_mut() {
                            Ok(mut buf) => {
                                match image.format {
                                    PixelFormat::ANON_1 => {
                                        for (ipx, opx) in image.data.chunks(1).zip(&mut *buf) {
                                            let [r] = *ipx else { unreachable!() };
                                            *opx = u32::from_le_bytes([0, 0, r, 0]);
                                        }
                                    }
                                    PixelFormat::ANON_2 => {
                                        for (ipx, opx) in image.data.chunks(2).zip(&mut *buf) {
                                            let [r, g] = *ipx else { unreachable!() };
                                            *opx = u32::from_le_bytes([0, g, r, 0]);
                                        }
                                    }
                                    f => {
                                        if f.is_anon() || f == PixelFormat::RGBA {
                                            for (ipx, opx) in
                                                image.data.chunks(f.pixel_size()).zip(&mut *buf)
                                            {
                                                let [r, g, b, ..] = *ipx else { unreachable!() };
                                                *opx = u32::from_le_bytes([b, g, r, 0]);
                                            }
                                        } else {
                                            image.convert_inplace(PixelFormat::RGB);
                                            for (ipx, opx) in image.data.chunks(3).zip(&mut *buf) {
                                                let [r, g, b] = *ipx else { unreachable!() };
                                                *opx = u32::from_le_bytes([b, g, r, 0]);
                                            }
                                        }
                                    }
                                }
                                if let Err(err) = buf.present() {
                                    tracing::error!(%err, "failed to present surface buffer");
                                    had_err = true;
                                }
                            }
                            Err(ref err) => {
                                tracing::error!(%err, "failed to get surface buffer");
                                had_err = true;
                            }
                        }
                        if had_err {
                            *dbg = DebugKind::Ignore;
                        }
                    }
                }
            }
        }
    }
}

const ONE: NonZero<u32> = NonZero::new(1).unwrap();

#[allow(clippy::large_enum_variant)]
enum HandlerInner {
    Winit(WinitHandler, EventLoop<Message>),
    Fallback(without_winit::Handler),
}

/// The handler for any incoming messages.
///
/// This should be run on the main thread through [`Self::run`], which will block it until it receives a [`Message::Shutdown`].
pub struct Handler {
    inner: HandlerInner,
}
impl Handler {
    /// Create a new handler and a sender.
    pub fn new(default: DefaultDebug) -> HandlerWithSender {
        match EventLoop::with_user_event().build() {
            Ok(event_loop) => {
                tracing::info!("successfully initialized event loop");
                match softbuffer::Context::new(event_loop.owned_display_handle()) {
                    Ok(context) => {
                        let proxy = event_loop.create_proxy();
                        HandlerWithSender {
                            handler: Self {
                                inner: HandlerInner::Winit(
                                    WinitHandler {
                                        default,
                                        context,
                                        windows: HashMap::new(),
                                        debugs: HashMap::new(),
                                    },
                                    event_loop,
                                ),
                            },
                            sender: Sender(SenderInner::Winit(proxy)),
                        }
                    }
                    Err(err) => {
                        tracing::error!(%err, "failed to initialize softbuffer context");
                        Self::no_gui(default)
                    }
                }
            }
            Err(err) => {
                tracing::error!(%err, "failed to initialize event loop");
                Self::no_gui(default)
            }
        }
    }
    /// Create a new handler that can't create windows.
    pub fn no_gui(default: DefaultDebug) -> HandlerWithSender {
        let (handler, sender) = without_winit::Handler::new_impl(default);
        HandlerWithSender {
            handler: Self {
                inner: HandlerInner::Fallback(handler),
            },
            sender: Sender(SenderInner::Fallback(sender)),
        }
    }
    /// Run the given handler.
    ///
    /// This blocks until a [`Message::Shutdown`] is sent.
    pub fn run(self) {
        match self.inner {
            HandlerInner::Winit(mut handler, event_loop) => {
                if let Err(err) = event_loop.run_app(&mut handler) {
                    tracing::error!(%err, "event loop exited with an error");
                }
            }
            HandlerInner::Fallback(fallback) => fallback.run(),
        }
    }
}
