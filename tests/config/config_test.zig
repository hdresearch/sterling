const std = @import("std");
const testing = std.testing;
const config = @import("../../src/config/config.zig");

test "config parsing - valid TOML" {
    const allocator = testing.allocator;
    
    const test_config = 
        \\[project]
        \\name = "test-api"
        \\version = "1.0.0"
        \\
        \\[languages]
        \\typescript = true
        \\rust = true
        \\python = true
        \\go = true
        \\zig = true
    ;
    
    // Test config parsing
    const parsed = config.parseConfig(allocator, test_config) catch |err| {
        std.log.err("Failed to parse config: {}", .{err});
        return err;
    };
    defer parsed.deinit();
    
    try testing.expectEqualStrings("test-api", parsed.project.name);
    try testing.expectEqualStrings("1.0.0", parsed.project.version);
    try testing.expect(parsed.languages.typescript);
    try testing.expect(parsed.languages.rust);
}

test "config validation - missing required fields" {
    const allocator = testing.allocator;
    
    const invalid_config = 
        \\[project]
        \\# Missing name and version
        \\
        \\[languages]
        \\typescript = true
    ;
    
    const result = config.parseConfig(allocator, invalid_config);
    try testing.expectError(config.ConfigError.MissingRequiredField, result);
}

test "config file loading" {
    const allocator = testing.allocator;
    
    // Test loading from file
    const result = config.loadConfigFile(allocator, "sterling.toml") catch |err| {
        // File might not exist in test environment, that's ok
        if (err == error.FileNotFound) return;
        return err;
    };
    defer if (result) |r| r.deinit();
    
    if (result) |cfg| {
        try testing.expect(cfg.project.name.len > 0);
    }
}
