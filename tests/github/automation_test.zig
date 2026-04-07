const std = @import("std");
const github = @import("github");

test "github config creation" {
    const cfg = github.GitHubConfig{
        .token = "test-token",
        .org = "test-org",
    };
    try std.testing.expectEqualStrings("test-token", cfg.token);
    try std.testing.expectEqualStrings("test-org", cfg.org);
    try std.testing.expectEqualStrings("https://api.github.com", cfg.base_url);
}
