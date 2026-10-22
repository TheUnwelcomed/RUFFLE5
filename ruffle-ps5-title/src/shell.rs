//! PS5_RUFFLE_SHELL_V1 — one display, shared by the menu and every game's player.
//!
//! Ruffle's `WgpuRenderBackend` takes ownership of its render target, and a `Player` owns its
//! renderer; to play one game after another each gets a fresh `Player` (Ruffle's own
//! `replace_root_movie` says it is not a clean reset). So the swapchain lives here, in `Display`,
//! behind an `Arc`, and `ShellTarget` is a handle to it that implements Ruffle's `RenderTarget`:
//! a player is built with a new handle, dropped with it, and the display stays. In the menu there
//! is no player at all: `Display::present_scene` draws the menu's scene straight into the next
//! frame.
//!
//! Over a game's frame, at submit time, the shell paints the pointer (src/cursor.rs) and whatever
//! overlay the main loop left in `Display::overlay` (the "hold to return" bar).
// SPDX-License-Identifier: GPL-3.0-or-later

use crate::cursor::{Cursor, CursorPainter, SharedCursor};
use crate::ui::{Color, Scene, Ui};
use ruffle_render_wgpu::target::{RenderTarget, RenderTargetFrame, SwapChainTarget, SwapChainTargetFrame};
use std::sync::{Arc, Mutex};

pub struct Display {
    swap: Mutex<SwapChainTarget>,
    pub device: wgpu::Device,
    pub queue: wgpu::Queue,
    pub width: u32,
    pub height: u32,
    pub format: wgpu::TextureFormat,
    cursor: SharedCursor,
    painter: CursorPainter,
    pub ui: Mutex<Ui>,
    overlay: Mutex<Scene>,
}

impl Display {
    pub fn new(swap: SwapChainTarget, device: wgpu::Device, queue: wgpu::Queue, cursor: SharedCursor) -> Result<Arc<Display>, String> {
        let (width, height, format) = (swap.width(), swap.height(), swap.format());
        let painter = CursorPainter::new(&device, format, width, height);
        let ui = Ui::new(&device, &queue, format, width, height)?;
        Ok(Arc::new(Display {
            swap: Mutex::new(swap),
            device,
            queue,
            width,
            height,
            format,
            cursor,
            painter,
            ui: Mutex::new(ui),
            overlay: Mutex::new(Scene::new()),
        }))
    }

    /// A render target for a new player.
    pub fn target(self: &Arc<Self>) -> ShellTarget {
        ShellTarget {
            display: Arc::clone(self),
        }
    }

    /// Replace the overlay drawn over a game's frames (an empty scene draws nothing).
    pub fn set_overlay(&self, scene: Scene) {
        if let Ok(mut o) = self.overlay.lock() {
            *o = scene;
        }
    }

    /// Draw `scene` on a cleared frame and present it (the menu).
    pub fn present_scene(&self, scene: &Scene, clear: Color) -> Result<(), String> {
        let mut swap = self.swap.lock().map_err(|_| "swapchain lock poisoned")?;
        let frame = swap.get_next_texture().ok_or("no frame from the swapchain")?;
        let cmd = {
            let mut ui = self.ui.lock().map_err(|_| "ui lock poisoned")?;
            ui.render(&self.device, &self.queue, frame.view(), Some(clear), scene)
        };
        swap.submit(&self.device, &self.queue, [cmd], frame);
        Ok(())
    }
}

pub struct ShellTarget {
    display: Arc<Display>,
}

impl std::fmt::Debug for ShellTarget {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ShellTarget({}x{} {:?})", self.display.width, self.display.height, self.display.format)
    }
}

impl RenderTarget for ShellTarget {
    type Frame = SwapChainTargetFrame;

    fn resize(&mut self, device: &wgpu::Device, width: u32, height: u32) {
        if let Ok(mut swap) = self.display.swap.lock() {
            swap.resize(device, width, height);
        }
    }

    fn format(&self) -> wgpu::TextureFormat {
        self.display.format
    }

    fn width(&self) -> u32 {
        self.display.width
    }

    fn height(&self) -> u32 {
        self.display.height
    }

    fn get_next_texture(&mut self) -> Option<Self::Frame> {
        self.display.swap.lock().ok()?.get_next_texture()
    }

    fn submit<I: IntoIterator<Item = wgpu::CommandBuffer>>(&self, device: &wgpu::Device, queue: &wgpu::Queue, command_buffers: I, frame: Self::Frame) -> wgpu::SubmissionIndex {
        let d = &self.display;
        let mut buffers: Vec<wgpu::CommandBuffer> = command_buffers.into_iter().collect();
        let cursor: Option<Cursor> = d.cursor.lock().ok().map(|c| *c);
        if let Some(c) = cursor {
            if let Some(cmd) = d.painter.paint(device, queue, frame.view(), c) {
                buffers.push(cmd);
            }
        }
        if let (Ok(overlay), Ok(mut ui)) = (d.overlay.lock(), d.ui.lock()) {
            if !overlay.is_empty() {
                buffers.push(ui.render(device, queue, frame.view(), None, &overlay));
            }
        }
        match d.swap.lock() {
            Ok(swap) => swap.submit(device, queue, buffers, frame),
            Err(poisoned) => poisoned.into_inner().submit(device, queue, buffers, frame),
        }
    }
}
