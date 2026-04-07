const std = @import("std");
const docs_gen = @import("docs_gen");

test "DocsGenerator init" {
    const cfg = docs_gen.DocsConfig{
        .project_name = "test-api",
        .description = "A test API",
        .output_dir = "/tmp/test-docs",
    };

    var gen = docs_gen.DocsGenerator.init(std.testing.allocator, cfg);
    defer gen.deinit();
}

test "SdkLanguage toString" {
    try std.testing.expectEqualStrings("rust", docs_gen.SdkLanguage.rust.toString());
    try std.testing.expectEqualStrings("typescript", docs_gen.SdkLanguage.typescript.toString());
}

test "SdkLanguage displayName" {
    try std.testing.expectEqualStrings("Rust", docs_gen.SdkLanguage.rust.displayName());
    try std.testing.expectEqualStrings("TypeScript", docs_gen.SdkLanguage.typescript.displayName());
}

test "createDocsPullRequest" {
    const cfg = docs_gen.DocsConfig{
        .project_name = "test-api",
        .description = "A test API",
        .output_dir = "/tmp/test-docs",
    };

    var gen = docs_gen.DocsGenerator.init(std.testing.allocator, cfg);
    defer gen.deinit();

    const pr = try gen.createDocsPullRequest("1.0.0", &.{ "docs/overview.mdx", "docs/api.mdx" });
    defer {
        std.testing.allocator.free(pr.title);
        std.testing.allocator.free(pr.body);
        std.testing.allocator.free(pr.branch);
    }

    try std.testing.expect(std.mem.indexOf(u8, pr.title, "1.0.0") != null);
    try std.testing.expect(std.mem.indexOf(u8, pr.body, "test-api") != null);
}
