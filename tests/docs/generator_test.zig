const std = @import("std");
const docs_gen = @import("docs_gen");

test "docs config creation" {
    const cfg = docs_gen.DocsConfig{
        .project_name = "test",
        .description = "Test project",
        .output_dir = "./docs",
    };
    try std.testing.expectEqualStrings("test", cfg.project_name);
    try std.testing.expectEqualStrings("linden", cfg.theme);
}
