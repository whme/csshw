//! Windows named pipe utilities
//!
//! This module provides Windows named pipe functionality for both client and daemon
//! using direct Windows API calls for blocking I/O operations.

#![deny(clippy::implicit_return)]
#![allow(clippy::needless_return, clippy::doc_overindented_list_items)]
#![warn(missing_docs)]

use std::io;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
use windows::Win32::Storage::FileSystem::{
    CreateFileW, ReadFile, FILE_ATTRIBUTE_NORMAL, FILE_GENERIC_READ, FILE_SHARE_NONE, OPEN_EXISTING,
};

/// Windows named pipe client
///
/// This provides a blocking interface to Windows named pipes using direct Windows API calls.
/// Suitable for both client and daemon usage.
pub struct WindowsNamedPipeClient {
    /// Windows handle to the named pipe
    handle: HANDLE,
}

impl WindowsNamedPipeClient {
    /// Create a new named pipe client and connect to the specified pipe
    ///
    /// # Arguments
    ///
    /// * `pipe_name` - The name of the named pipe to connect to
    ///
    /// # Returns
    ///
    /// A new WindowsNamedPipeClient instance or an error if connection fails
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use csshw_lib::utils::named_pipe::WindowsNamedPipeClient;
    ///
    /// let client = WindowsNamedPipeClient::connect(r"\\.\pipe\csshw").unwrap();
    /// ```
    pub fn connect(pipe_name: &str) -> io::Result<Self> {
        // Convert pipe name to wide string
        let pipe_name_wide: Vec<u16> = pipe_name.encode_utf16().chain(std::iter::once(0)).collect();

        // Create the named pipe handle
        let handle = unsafe {
            CreateFileW(
                PCWSTR(pipe_name_wide.as_ptr()),
                FILE_GENERIC_READ.0,
                FILE_SHARE_NONE,
                None,
                OPEN_EXISTING,
                FILE_ATTRIBUTE_NORMAL,
                None,
            )
        }?;

        if handle == INVALID_HANDLE_VALUE {
            return Err(io::Error::last_os_error());
        }

        return Ok(WindowsNamedPipeClient { handle });
    }

    /// Read data from the named pipe (blocking)
    ///
    /// This method blocks until data is available to read from the pipe.
    ///
    /// # Arguments
    ///
    /// * `buf` - Buffer to read data into
    ///
    /// # Returns
    ///
    /// The number of bytes read, or an error
    pub fn read(&self, buf: &mut [u8]) -> io::Result<usize> {
        let mut bytes_read = 0u32;

        let result = unsafe { ReadFile(self.handle, Some(buf), Some(&mut bytes_read), None) };

        if result.is_ok() {
            return Ok(bytes_read as usize);
        }

        return Err(io::Error::last_os_error());
    }
}

impl Drop for WindowsNamedPipeClient {
    /// Clean up the named pipe handle when the client is dropped
    fn drop(&mut self) {
        if self.handle != INVALID_HANDLE_VALUE {
            unsafe {
                let _ = CloseHandle(self.handle);
            }
        }
    }
}
