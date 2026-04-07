const std = @import("std");
const testing = std.testing;
const github = @import("github");

test "GitHub config initialization" {
    const config = github.GitHubConfig{
        .token = "test-token",
        .org = "test-org",
    };
    
    try testing.expect(std.mem.eql(u8, config.token, "test-token"));
    try testing.expect(std.mem.eql(u8, config.org, "test-org"));
    try testing.expect(std.mem.eql(u8, config.base_url, "https://api.github.com"));
}

test "GitHub automation initialization" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const config = github.GitHubConfig{
        .token = "test-token",
        .org = "test-org",
    };

    var gh = github.GitHubAutomation.init(allocator, config);
    defer gh.deinit();

    try testing.expect(std.mem.eql(u8, gh.config.token, "test-token"));
    try testing.expect(std.mem.eql(u8, gh.config.org, "test-org"));
}

test "Workflow generation" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const config = github.GitHubConfig{
        .token = "test-token",
        .org = "test-org",
    };

    var gh = github.GitHubAutomation.init(allocator, config);
    defer gh.deinit();

    const rust_workflow = try gh.generateWorkflow("rust");
    defer allocator.free(rust_workflow);
    
    try testing.expect(std.mem.indexOf(u8, rust_workflow, "Rust CI") != null);
    try testing.expect(std.mem.indexOf(u8, rust_workflow, "cargo test") != null);
}
