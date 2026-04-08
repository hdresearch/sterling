const std = @import("std");
const testing = std.testing;
const sdk = @import("../../src/generator/sdk.zig");

test "generate typescript sdk" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    // Test basic SDK generation
    const generator = sdk.SDKGenerator.init(allocator);
    
    // This will test the template-based generation once implemented
    _ = generator;
}
