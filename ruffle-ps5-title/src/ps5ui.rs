//! PS5_RUFFLE_PS5UI_V1 — Ruffle's UI hooks on the PS5: a game's text box asking for a keyboard, and
//! a game hiding the mouse (it draws its own pointer, so ours stays away). The rest is as Ruffle's
//! own `NullUiBackend` does it (core/src/backend/ui.rs at the pinned revision). After
//! Ruffle-Flash-PS5's src/ps5ui.rs (GPL-3.0, by ZiZc3).
// SPDX-License-Identifier: GPL-3.0-or-later

use ruffle_core::backend::ui::{
    DialogResultFuture, FileDialogResult, FileFilter, FontDefinition, FullscreenError, LanguageIdentifier, MouseCursor, MultiDialogResultFuture,
    MultiFileDialogResult, UiBackend, US_ENGLISH,
};
use ruffle_core::font::FontQuery;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};
use std::sync::Arc;
use url::Url;

/// What the game asked of the title, read by the game loop.
#[derive(Default)]
pub struct UiState {
    /// 0 nothing new, 1 a text box wants the keyboard, 2 it no longer does.
    keyboard: AtomicU8,
    mouse_hidden: AtomicBool,
}

impl UiState {
    /// The latest keyboard request, if any (Some(true) = open).
    pub fn take_keyboard_request(&self) -> Option<bool> {
        match self.keyboard.swap(0, Ordering::Relaxed) {
            1 => Some(true),
            2 => Some(false),
            _ => None,
        }
    }

    pub fn mouse_hidden(&self) -> bool {
        self.mouse_hidden.load(Ordering::Relaxed)
    }
}

pub struct Ps5Ui {
    pub state: Arc<UiState>,
}

impl UiBackend for Ps5Ui {
    fn mouse_visible(&self) -> bool {
        !self.state.mouse_hidden()
    }

    fn set_mouse_visible(&mut self, visible: bool) {
        self.state.mouse_hidden.store(!visible, Ordering::Relaxed);
    }

    fn set_mouse_cursor(&mut self, _cursor: MouseCursor) {}

    fn clipboard_content(&mut self) -> String {
        "".into()
    }

    fn set_clipboard_content(&mut self, _content: String) {}

    fn set_fullscreen(&mut self, _is_full: bool) -> Result<(), FullscreenError> {
        Ok(())
    }

    fn display_root_movie_download_failed_message(&self, _invalid_swf: bool, _fetch_error: String) {}

    fn message(&self, _message: &str) {}

    fn open_virtual_keyboard(&self) {
        self.state.keyboard.store(1, Ordering::Relaxed);
    }

    fn close_virtual_keyboard(&self) {
        self.state.keyboard.store(2, Ordering::Relaxed);
    }

    fn language(&self) -> LanguageIdentifier {
        US_ENGLISH.clone()
    }

    fn display_unsupported_video(&self, _url: Url) {}

    fn load_device_font(&self, _query: &FontQuery, _register: &mut dyn FnMut(FontDefinition)) {}

    fn sort_device_fonts(&self, _query: &FontQuery, _register: &mut dyn FnMut(FontDefinition)) -> Vec<FontQuery> {
        Vec::new()
    }

    fn display_file_open_dialog(&mut self, _filters: Vec<FileFilter>) -> Option<DialogResultFuture> {
        Some(Box::pin(async move { Ok(FileDialogResult::Canceled) }))
    }

    fn display_file_open_dialog_multiple(&mut self, _filters: Vec<FileFilter>) -> Option<MultiDialogResultFuture> {
        Some(Box::pin(async move { Ok(MultiFileDialogResult::Canceled) }))
    }

    fn display_file_save_dialog(&mut self, _file_name: String, _title: String) -> Option<DialogResultFuture> {
        None
    }

    fn close_file_dialog(&mut self) {}
}
