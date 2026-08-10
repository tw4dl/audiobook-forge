//! Small audited boundary around MLX allocation-cache control.
//!
//! This is the only `kokoro-book` crate allowed to call the unsafe MLX C API.

use std::error::Error as StdError;
use std::fmt;

/// Result type for checked MLX memory operations.
pub type Result<T> = std::result::Result<T, Error>;

/// Memory values reported by MLX in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MemoryStats {
    pub active_bytes: usize,
    pub cached_bytes: usize,
    pub peak_bytes: usize,
}

/// Failure from a checked MLX C call.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    CallFailed {
        operation: &'static str,
        status: i32,
    },
    UnsupportedPlatform,
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::CallFailed { operation, status } => {
                write!(formatter, "MLX {operation} failed with status {status}")
            }
            Self::UnsupportedPlatform => {
                formatter.write_str("MLX memory control requires Apple Silicon macOS")
            }
        }
    }
}

impl StdError for Error {}

/// Release unused MLX allocation-cache buffers.
///
/// # Errors
///
/// Returns an error when MLX reports a nonzero C status or on unsupported
/// platforms.
pub fn clear_cache() -> Result<()> {
    platform::clear_cache()
}

/// Set the MLX cache limit in bytes and return the previous limit.
///
/// # Errors
///
/// Returns an error when MLX reports a nonzero C status or on unsupported
/// platforms.
pub fn set_cache_limit(bytes: usize) -> Result<usize> {
    platform::set_cache_limit(bytes)
}

/// Read active, cached, and peak MLX allocation counts.
///
/// # Errors
///
/// Returns an error when any MLX C getter reports a nonzero status or on
/// unsupported platforms.
pub fn memory_stats() -> Result<MemoryStats> {
    platform::memory_stats()
}

#[cfg(all(target_os = "macos", target_arch = "aarch64"))]
mod platform {
    use super::{Error, MemoryStats, Result};

    pub(super) fn clear_cache() -> Result<()> {
        // SAFETY: This C call has no arguments. Its integer status is checked.
        checked("clear_cache", unsafe { mlx_sys::mlx_clear_cache() })
    }

    pub(super) fn set_cache_limit(bytes: usize) -> Result<usize> {
        let mut previous = 0_usize;
        // SAFETY: `previous` is a valid writable size_t pointer for this call.
        // The integer status is checked before the initialized value is used.
        checked("set_cache_limit", unsafe {
            mlx_sys::mlx_set_cache_limit(&raw mut previous, bytes)
        })?;
        Ok(previous)
    }

    pub(super) fn memory_stats() -> Result<MemoryStats> {
        Ok(MemoryStats {
            active_bytes: read_value("get_active_memory", |result| unsafe {
                // SAFETY: `result` is a valid writable size_t pointer for this
                // call. `read_value` checks the C status before returning it.
                mlx_sys::mlx_get_active_memory(result)
            })?,
            cached_bytes: read_value("get_cache_memory", |result| unsafe {
                // SAFETY: Same checked out-pointer contract as above.
                mlx_sys::mlx_get_cache_memory(result)
            })?,
            peak_bytes: read_value("get_peak_memory", |result| unsafe {
                // SAFETY: Same checked out-pointer contract as above.
                mlx_sys::mlx_get_peak_memory(result)
            })?,
        })
    }

    fn read_value(operation: &'static str, call: impl FnOnce(*mut usize) -> i32) -> Result<usize> {
        let mut result = 0_usize;
        checked(operation, call(&raw mut result))?;
        Ok(result)
    }

    fn checked(operation: &'static str, status: i32) -> Result<()> {
        if status == 0 {
            Ok(())
        } else {
            Err(Error::CallFailed { operation, status })
        }
    }

    #[cfg(test)]
    mod tests {
        use super::{Error, checked};

        #[test]
        fn checks_every_c_status() {
            assert_eq!(checked("test", 0), Ok(()));
            assert_eq!(
                checked("test", 7),
                Err(Error::CallFailed {
                    operation: "test",
                    status: 7,
                })
            );
        }
    }
}

#[cfg(not(all(target_os = "macos", target_arch = "aarch64")))]
mod platform {
    use super::{Error, MemoryStats, Result};

    pub(super) const fn clear_cache() -> Result<()> {
        Err(Error::UnsupportedPlatform)
    }

    pub(super) const fn set_cache_limit(_bytes: usize) -> Result<usize> {
        Err(Error::UnsupportedPlatform)
    }

    pub(super) const fn memory_stats() -> Result<MemoryStats> {
        Err(Error::UnsupportedPlatform)
    }
}
