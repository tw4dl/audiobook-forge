#![cfg(all(target_os = "macos", target_arch = "aarch64"))]

use mlx_memory_control::{clear_cache, memory_stats, set_cache_limit};

#[test]
fn controls_the_real_apple_silicon_mlx_allocator() {
    let old_limit = set_cache_limit(64 * 1_024 * 1_024).expect("set MLX cache limit");

    clear_cache().expect("clear MLX cache");
    let stats = memory_stats().expect("read MLX memory stats");

    assert!(stats.cached_bytes <= 1_024 * 1_024);
    set_cache_limit(old_limit).expect("restore MLX cache limit");
}
