const std = @import("std");
const testing = std.testing;

// Import all test modules
const config_tests = @import("config/config_test.zig");
const parser_tests = @import("parser/openapi_test.zig");
const generator_tests = @import("generator/generator_test.zig");
const github_tests = @import("github/automation_test.zig");
const webhook_tests = @import("webhook/handler_test.zig");
const integration_tests = @import("integration/end_to_end_test.zig");

test "Sterling Test Suite" {
    std.testing.refAllDecls(@This());
}

// Test discovery and execution
pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    std.log.info("Running Sterling SDK Generator Test Suite", .{});
    
    // Run all tests
    try std.testing.runTests(allocator, .{});
    
    std.log.info("All tests completed successfully!", .{});
}
