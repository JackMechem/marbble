//! In-memory streams for saving and loading a plug-in's state.

use std::ffi::c_void;

use clap_sys::stream::{clap_istream, clap_ostream};

/// Collects what the plug-in writes.
pub(crate) struct OutStream {
    pub data: Vec<u8>,
}

impl OutStream {
    /// Calls `f` with a `clap_ostream` into a new buffer, and gives back what was written.
    pub fn collect(f: impl FnOnce(*const clap_ostream) -> bool) -> Option<Vec<u8>> {
        let mut s = OutStream { data: Vec::new() };
        let raw = clap_ostream { ctx: &mut s as *mut OutStream as *mut c_void, write: Some(write) };
        f(&raw).then_some(s.data)
    }
}

unsafe extern "C" fn write(stream: *const clap_ostream, buffer: *const c_void, size: u64) -> i64 {
    let s = &mut *((*stream).ctx as *mut OutStream);
    if size > 0 && !buffer.is_null() {
        s.data.extend_from_slice(std::slice::from_raw_parts(buffer as *const u8, size as usize));
    }
    size as i64
}

/// Hands the plug-in a saved state, in as many reads as it likes.
pub(crate) struct InStream<'a> {
    data: &'a [u8],
}

impl InStream<'_> {
    /// Calls `f` with a `clap_istream` over `data`.
    pub fn over(data: &[u8], f: impl FnOnce(*const clap_istream) -> bool) -> bool {
        let mut s = InStream { data };
        let raw = clap_istream { ctx: &mut s as *mut InStream as *mut c_void, read: Some(read) };
        f(&raw)
    }
}

unsafe extern "C" fn read(stream: *const clap_istream, buffer: *mut c_void, size: u64) -> i64 {
    let s = &mut *((*stream).ctx as *mut InStream);
    let n = (size as usize).min(s.data.len());
    if n > 0 && !buffer.is_null() {
        std::ptr::copy_nonoverlapping(s.data.as_ptr(), buffer as *mut u8, n);
        s.data = &s.data[n..];
    }
    n as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_state_goes_out_and_back_in_pieces() {
        let saved = OutStream::collect(|s| unsafe {
            let w = (*s).write.unwrap();
            w(s, b"hello ".as_ptr() as _, 6) == 6 && w(s, b"world".as_ptr() as _, 5) == 5
        })
        .unwrap();
        assert_eq!(saved, b"hello world");
        assert!(OutStream::collect(|_| false).is_none(), "a failed save gives nothing");
        let mut back = Vec::new();
        assert!(InStream::over(&saved, |s| unsafe {
            let r = (*s).read.unwrap();
            let mut buf = [0u8; 4];
            loop {
                let n = r(s, buf.as_mut_ptr() as _, 4);
                if n == 0 {
                    break true;
                }
                back.extend_from_slice(&buf[..n as usize]);
            }
        }));
        assert_eq!(back, saved);
    }
}
