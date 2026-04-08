const std = @import("std");
const testing = std.testing;
const config = @import("../../src/config/config.zig");

test "parse sterling config" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const toml_content = 
        \\[project]
        \\name = "test-sdk"
        \\version = "1.0.0"
        \\
        \\[[targets]]
        \\language = "typescript"
        \\output_dir = "./generated/typescript"
    ;

    const cfg = try config.parseConfig(allocator, toml_content);
    try testing.expectEqualStrings("test-sdk", cfg.project.name);
    try testing.expectEqualStrings("1.0.0", cfg.project.version);
    try testing.expect(cfg.targets.len > 0);
}
