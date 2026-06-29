//! Equilibrium FFI helpers for Zig
//!
//! This module provides comptime helpers for exporting Zig functions
//! to C FFI, compatible with equilibrium's automatic binding generation.
//!
//! Example:
//!   const eq = @import("equilibrium.zig");
//!   
//!   pub export fn add(a: i32, b: i32) i32 {
//!       return a + b;
//!   }
//!
const std = @import("std");

/// Type conversion helpers
pub const FFI = struct {
    /// Convert Zig int to C int
    pub fn toInt(value: anytype) i32 {
        return @intCast(value);
    }

    /// Convert C int to Zig int
    pub fn fromInt(value: i32) isize {
        return @as(isize, value);
    }

    /// Convert Zig slice to C pointer
    pub fn toPtr(slice: []const u8) [*c]const u8 {
        return @ptrCast(slice.ptr);
    }

    /// Null-terminated string helper
    pub fn toCString(allocator: std.mem.Allocator, s: []const u8) ![:0]const u8 {
        return try allocator.dupeZ(u8, s);
    }
};

/// Example FFI-exported functions
pub export fn equilibrium_version() i32 {
    return 1;
}

// Test
test "FFI helpers" {
    const value = FFI.toInt(42);
    try std.testing.expectEqual(@as(i32, 42), value);
}
