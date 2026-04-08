const std = @import("std");
const testing = std.testing;

// Import all test modules
const parser_tests = @import("parser/openapi_test.zig");
const config_tests = @import("config/config_test.zig");
const generator_tests = @import("generator/sdk_test.zig");

test "all tests" {
    testing.refAllDecls(@This());
}
