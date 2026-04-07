const std = @import("std");
const config = @import("config");

test "parse basic config" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const a = arena.allocator();

    const content =
        \\[project]
        \\name = "my-api"
        \\version = "1.0.0"
        \\
        \\[targets.typescript]
        \\language = "typescript"
        \\output_dir = "./generated/typescript"
    ;

    const cfg = try config.parseConfig(a, content);
    try std.testing.expectEqualStrings("my-api", cfg.project.name);
    try std.testing.expectEqualStrings("1.0.0", cfg.project.version);
    try std.testing.expect(cfg.targets.len > 0);
}

test "missing project section errors" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();

    const content =
        \\[targets.typescript]
        \\language = "typescript"
        \\output_dir = "./generated/typescript"
    ;

    try std.testing.expectError(config.ConfigError.MissingProjectSection, config.parseConfig(arena.allocator(), content));
}
