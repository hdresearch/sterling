const std = @import("std");
const github = @import("github");

test "GitHubAutomation init" {
    const cfg = github.GitHubConfig{
        .token = "test-token",
        .org = "test-org",
    };

    var automation = github.GitHubAutomation.init(std.testing.allocator, cfg);
    defer automation.deinit();
}

test "generateWorkflow rust" {
    const cfg = github.GitHubConfig{
        .token = "test-token",
        .org = "test-org",
    };

    var automation = github.GitHubAutomation.init(std.testing.allocator, cfg);
    defer automation.deinit();

    const workflow = try automation.generateWorkflow("rust");
    defer std.testing.allocator.free(workflow);

    try std.testing.expect(std.mem.indexOf(u8, workflow, "cargo test") != null);
}

test "generateWorkflow typescript" {
    const cfg = github.GitHubConfig{
        .token = "test-token",
        .org = "test-org",
    };

    var automation = github.GitHubAutomation.init(std.testing.allocator, cfg);
    defer automation.deinit();

    const workflow = try automation.generateWorkflow("typescript");
    defer std.testing.allocator.free(workflow);

    try std.testing.expect(std.mem.indexOf(u8, workflow, "npm") != null);
}

test "generateSetupInstructions" {
    const cfg = github.GitHubConfig{
        .token = "test-token",
        .org = "test-org",
    };

    var automation = github.GitHubAutomation.init(std.testing.allocator, cfg);
    defer automation.deinit();

    const instructions = try automation.generateSetupInstructions("my-sdk", "rust");
    defer std.testing.allocator.free(instructions);

    try std.testing.expect(std.mem.indexOf(u8, instructions, "my-sdk") != null);
}
