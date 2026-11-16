//! Placeholder editor for platforms whose native window embedding is not implemented yet.

use crate::View;

/// What a view holds on to its window by (nothing here).
#[derive(Clone)]
pub struct Frame;

impl Frame {
    pub fn resize(&self, _size: (u32, u32)) {}
    pub fn fit(&self, _size: (u32, u32)) {}
}

pub struct Editor;

impl Editor {
    pub fn open(_view: impl View + 'static, _title: &str) -> Result<Editor, String> {
        Err("plugin editor windows are not implemented on this platform yet".into())
    }
    pub fn open_at(view: impl View + 'static, title: &str, _at: Option<(i16, i16)>) -> Result<Editor, String> {
        Self::open(view, title)
    }
    pub fn pump(&mut self) -> bool {
        true
    }
    pub fn raise(&self) {}
    pub fn set_quiet(&self, _on: bool) {}
    pub fn fit(&self, _size: (u32, u32)) {}
    pub fn hide(&self) {}
    pub fn close<K: Send + 'static>(self, _keep_alive: K) {}
}
