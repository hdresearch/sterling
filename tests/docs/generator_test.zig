const std = @import("std");
const testing = std.testing;
const docs = @import("../../src/docs/generator.zig");

test "Docs config initialization" {
    const config = docs.DocsConfig{
        .project_name = "test-project",
        .description = "Test project description",
        .output_dir = "./test-docs",
    };
    
    try testing.expect(std.mem.eql(u8, config.project_name, "test-project"));
    try testing.expect(std.mem.eql(u8, config.description, "Test project description"));
    try testing.expect(std.mem.eql(u8, config.theme, "linden"));
}

test "Docs generator initialization" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const config = docs.DocsConfig{
        .project_name = "test-project",
        .description = "Test project description",
        .output_dir = "./test-docs",
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer generator.deinit();

    try testing.expect(std.mem.eql(u8, generator.config.project_name, "test-project"));
}
