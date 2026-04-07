const std = @import("std");
const config = @import("config");

test "parse basic config" {
    // Use arena to avoid leak issues with TOML parser internals
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const content =
        \\[project]
        \\name = "test-api"
        \\version = "1.0.0"
        \\
        \\[targets.typescript]
        \\language = "typescript"
        \\output_dir = "./generated/typescript"
    ;

    const cfg = try config.parseConfig(allocator, content);

    try std.testing.expectEqualStrings("test-api", cfg.project.name);
    try std.testing.expectEqualStrings("1.0.0", cfg.project.version);
    try std.testing.expect(cfg.targets.len == 1);
    try std.testing.expect(cfg.targets[0].language == .typescript);
}
