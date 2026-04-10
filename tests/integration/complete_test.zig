const std = @import("std");

test "Sterling complete functionality test" {
    var arena = std.heap.ArenaAllocator.init(std.testing.allocator);
    defer arena.deinit();
    const allocator = arena.allocator();

    // Test 1: Basic generation works
    const basic_result = std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "./zig-out/bin/sterling", "generate", "--spec", "test-openapi.yaml", "--config", "sterling.toml" },
    }) catch |err| {
        std.debug.print("Basic generation failed: {}\n", .{err});
        return err;
    };
    
    try std.testing.expect(basic_result.term.Exited == 0);
    try std.testing.expect(std.mem.indexOf(u8, basic_result.stdout, "SDK generation completed successfully") != null);

    // Test 2: All language SDKs generated
    std.fs.cwd().access("generated/typescript/package.json", .{}) catch |err| {
        std.debug.print("TypeScript SDK missing: {}\n", .{err});
        return err;
    };
    
    std.fs.cwd().access("generated/rust/Cargo.toml", .{}) catch |err| {
        std.debug.print("Rust SDK missing: {}\n", .{err});
        return err;
    };
    
    std.fs.cwd().access("generated/python/setup.py", .{}) catch |err| {
        std.debug.print("Python SDK missing: {}\n", .{err});
        return err;
    };
    
    std.fs.cwd().access("generated/go/go.mod", .{}) catch |err| {
        std.debug.print("Go SDK missing: {}\n", .{err});
        return err;
    };

    // Test 3: Generated code quality
    const ts_client = std.fs.cwd().readFileAlloc(allocator, "generated/typescript/src/client.ts", 1024 * 1024) catch |err| {
        std.debug.print("Failed to read TypeScript client: {}\n", .{err});
        return err;
    };
    
    try std.testing.expect(std.mem.indexOf(u8, ts_client, "export class") != null);
    try std.testing.expect(std.mem.indexOf(u8, ts_client, "async") != null);
    try std.testing.expect(std.mem.indexOf(u8, ts_client, "fetch") != null);

    // Test 4: CLI help works
    const help_result = std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "./zig-out/bin/sterling", "--help" },
    }) catch |err| {
        std.debug.print("Help command failed: {}\n", .{err});
        return err;
    };
    
    try std.testing.expect(std.mem.indexOf(u8, help_result.stdout, "Sterling SDK Generator") != null);

    // Test 5: Version command works
    const version_result = std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "./zig-out/bin/sterling", "version" },
    }) catch |err| {
        std.debug.print("Version command failed: {}\n", .{err});
        return err;
    };
    
    try std.testing.expect(std.mem.indexOf(u8, version_result.stdout, "v0.1.0") != null);

    // Test 6: Init command works
    const init_result = std.process.Child.run(.{
        .allocator = allocator,
        .argv = &[_][]const u8{ "./zig-out/bin/sterling", "init" },
        .cwd = "/tmp",
    }) catch |err| {
        std.debug.print("Init command failed: {}\n", .{err});
        return err;
    };
    
    try std.testing.expect(init_result.term.Exited == 0);
    
    // Verify sterling.toml was created
    std.fs.cwd().access("/tmp/sterling.toml", .{}) catch |err| {
        std.debug.print("sterling.toml not created: {}\n", .{err});
        return err;
    };
}

test "Sterling module integration test" {
    // Test that all modules can be imported and initialized
    const allocator = std.testing.allocator;
    
    // Test config loading
    const cfg = @import("config").loadConfig(allocator, "sterling.toml") catch |err| {
        std.debug.print("Config loading failed: {}\n", .{err});
        return err;
    };
    
    try std.testing.expect(cfg.project.name.len > 0);
    
    // Test OpenAPI parsing
    const spec_content = std.fs.cwd().readFileAlloc(allocator, "test-openapi.yaml", 1024 * 1024) catch |err| {
        std.debug.print("Failed to read test spec: {}\n", .{err});
        return err;
    };
    defer allocator.free(spec_content);
    
    const spec = @import("openapi").parseOpenAPI(allocator, spec_content) catch |err| {
        std.debug.print("OpenAPI parsing failed: {}\n", .{err});
        return err;
    };
    
    try std.testing.expect(spec.info.title.len > 0);
}
