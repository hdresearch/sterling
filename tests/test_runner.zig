const std = @import("std");

// Import existing tests
const config_test = @import("config/config_test.zig");
const openapi_test = @import("parser/openapi_test.zig");
const sdk_test = @import("generator/sdk_test.zig");
const rust_test = @import("generator/rust_test.zig");

// Import new feature tests
const llm_enhancer_test = @import("llm/enhancer_test.zig");
const github_automation_test = @import("github/automation_test.zig");
const docs_generator_test = @import("docs/generator_test.zig");

test "All Sterling tests" {
    std.testing.refAllDecls(@This());
}
