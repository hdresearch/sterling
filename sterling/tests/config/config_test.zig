const std = @import("std");
const testing = std.testing;
const config = @import("config");

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

test "parse multiple targets" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const toml_content =
        \\[project]
        \\name = "multi-sdk"
        \\version = "2.0.0"
        \\
        \\[[targets]]
        \\language = "typescript"
        \\output_dir = "./generated/typescript"
        \\repository = "https://github.com/org/ts-sdk"
        \\branch = "main"
        \\
        \\[[targets]]
        \\language = "python"
        \\output_dir = "./generated/python"
        \\repository = "https://github.com/org/py-sdk"
        \\
        \\[[targets]]
        \\language = "rust"
        \\output_dir = "./generated/rust"
    ;

    const cfg = try config.parseConfig(allocator, toml_content);
    try testing.expectEqualStrings("multi-sdk", cfg.project.name);
    try testing.expectEqualStrings("2.0.0", cfg.project.version);
    try testing.expectEqual(@as(usize, 3), cfg.targets.len);
    try testing.expectEqual(config.Config.Target.Language.typescript, cfg.targets[0].language);
    try testing.expectEqual(config.Config.Target.Language.python, cfg.targets[1].language);
    try testing.expectEqual(config.Config.Target.Language.rust, cfg.targets[2].language);
    try testing.expectEqualStrings("./generated/typescript", cfg.targets[0].output_dir);
    try testing.expectEqualStrings("https://github.com/org/ts-sdk", cfg.targets[0].repository);
}

test "parse llm config" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const toml_content =
        \\[project]
        \\name = "llm-test"
        \\version = "1.0.0"
        \\
        \\[[targets]]
        \\language = "go"
        \\output_dir = "./generated/go"
        \\
        \\[llm]
        \\provider = "anthropic"
        \\api_key = "sk-test-key"
        \\model = "claude-3-sonnet"
    ;

    const cfg = try config.parseConfig(allocator, toml_content);
    try testing.expect(cfg.llm != null);
    try testing.expectEqual(config.Config.LLMConfig.Provider.anthropic, cfg.llm.?.provider);
    try testing.expectEqualStrings("sk-test-key", cfg.llm.?.api_key);
    try testing.expectEqualStrings("claude-3-sonnet", cfg.llm.?.model);
}

test "missing project section returns error" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const toml_content =
        \\[[targets]]
        \\language = "typescript"
        \\output_dir = "./generated/typescript"
    ;

    const result = config.parseConfig(allocator, toml_content);
    try testing.expectError(error.MissingProjectSection, result);
}

test "missing targets returns error" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const toml_content =
        \\[project]
        \\name = "no-targets"
        \\version = "1.0.0"
    ;

    const result = config.parseConfig(allocator, toml_content);
    try testing.expectError(error.MissingTargets, result);
}

test "optional repository defaults to empty" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const toml_content =
        \\[project]
        \\name = "minimal"
        \\version = "0.1.0"
        \\
        \\[[targets]]
        \\language = "typescript"
        \\output_dir = "./out"
    ;

    const cfg = try config.parseConfig(allocator, toml_content);
    try testing.expectEqualStrings("", cfg.targets[0].repository);
    try testing.expectEqualStrings("main", cfg.targets[0].branch);
}

test "comments and blank lines are ignored" {
    var arena = std.heap.ArenaAllocator.init(testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    const toml_content =
        \\# This is a comment
        \\
        \\[project]
        \\# Project name
        \\name = "commented"
        \\version = "1.0.0"
        \\
        \\# Targets section
        \\[[targets]]
        \\language = "python"
        \\output_dir = "./gen/python"
    ;

    const cfg = try config.parseConfig(allocator, toml_content);
    try testing.expectEqualStrings("commented", cfg.project.name);
    try testing.expectEqual(@as(usize, 1), cfg.targets.len);
}
