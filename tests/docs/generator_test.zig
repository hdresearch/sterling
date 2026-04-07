const std = @import("std");
const testing = std.testing;
const docs = @import("docs_gen");

// =========================================================================
// Existing tests (preserved)
// =========================================================================

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

// =========================================================================
// SdkLanguage tests
// =========================================================================

test "SdkLanguage toString returns correct strings" {
    try testing.expectEqualStrings("rust", docs.SdkLanguage.rust.toString());
    try testing.expectEqualStrings("typescript", docs.SdkLanguage.typescript.toString());
    try testing.expectEqualStrings("python", docs.SdkLanguage.python.toString());
    try testing.expectEqualStrings("go", docs.SdkLanguage.go.toString());
}

test "SdkLanguage displayName returns correct names" {
    try testing.expectEqualStrings("Rust", docs.SdkLanguage.rust.displayName());
    try testing.expectEqualStrings("TypeScript", docs.SdkLanguage.typescript.displayName());
    try testing.expectEqualStrings("Python", docs.SdkLanguage.python.displayName());
    try testing.expectEqualStrings("Go", docs.SdkLanguage.go.displayName());
}

// =========================================================================
// VersDocsConfig tests
// =========================================================================

test "VersDocsConfig defaults" {
    const config = docs.VersDocsConfig{
        .repo_url = "https://github.com/hdresearch/vers-docs",
    };

    try testing.expectEqualStrings("main", config.branch);
    try testing.expectEqualStrings("docs", config.docs_path);
    try testing.expectEqualStrings("docs.json", config.mint_config_path);
}

// =========================================================================
// VersDocsPage tests
// =========================================================================

test "VersDocsPage with optional fields" {
    const page = docs.VersDocsPage{
        .title = "Rust SDK",
        .description = "Rust SDK documentation",
        .slug = "sdks/rust",
        .group = "Language SDKs",
        .sdk_language = .rust,
        .version = "1.0.0",
    };

    try testing.expectEqualStrings("Rust SDK", page.title);
    try testing.expect(page.sdk_language.? == .rust);
    try testing.expectEqualStrings("1.0.0", page.version.?);
}

test "VersDocsPage without optional fields" {
    const page = docs.VersDocsPage{
        .title = "Overview",
        .description = "Project overview",
        .slug = "overview",
        .group = "Introduction",
    };

    try testing.expect(page.sdk_language == null);
    try testing.expect(page.version == null);
}

// =========================================================================
// DocsPullRequest tests
// =========================================================================

test "DocsPullRequest defaults" {
    const files = [_][]const u8{ "docs.json", "overview.mdx" };
    const pr = docs.DocsPullRequest{
        .title = "docs: update",
        .body = "body",
        .branch = "docs/update",
        .files_changed = &files,
    };

    try testing.expectEqualStrings("main", pr.base_branch);
    try testing.expect(pr.files_changed.len == 2);
}

// =========================================================================
// generateVersDocsIntegration tests
// =========================================================================

test "generateVersDocsIntegration creates correct directory structure" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const output_dir = "./test-vers-docs-integration";
    defer std.fs.cwd().deleteTree(output_dir) catch {};

    // Create a temporary spec file
    const spec_path = "./test-vers-docs-spec.json";
    {
        const spec_file = try std.fs.cwd().createFile(spec_path, .{});
        defer spec_file.close();
        try spec_file.writeAll("{\"openapi\": \"3.0.0\", \"info\": {\"title\": \"Test API\", \"version\": \"1.0.0\"}}");
    }
    defer std.fs.cwd().deleteFile(spec_path) catch {};

    const config = docs.DocsConfig{
        .project_name = "test-project",
        .description = "Test project for vers-docs",
        .output_dir = output_dir,
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer generator.deinit();

    try generator.generateVersDocsIntegration(spec_path, output_dir);

    // Verify directory structure
    var dir = try std.fs.cwd().openDir(output_dir, .{});
    defer dir.close();

    // Check docs.json exists
    const docs_json = try dir.openFile("docs.json", .{});
    docs_json.close();

    // Check overview.mdx exists
    const overview = try dir.openFile("overview.mdx", .{});
    overview.close();

    // Check quickstart.mdx exists
    const quickstart = try dir.openFile("quickstart.mdx", .{});
    quickstart.close();

    // Check sdks directory and language files
    var sdks_dir = try dir.openDir("sdks", .{});
    defer sdks_dir.close();

    const rust_doc = try sdks_dir.openFile("rust.mdx", .{});
    rust_doc.close();
    const ts_doc = try sdks_dir.openFile("typescript.mdx", .{});
    ts_doc.close();
    const py_doc = try sdks_dir.openFile("python.mdx", .{});
    py_doc.close();
    const go_doc = try sdks_dir.openFile("go.mdx", .{});
    go_doc.close();

    // Check api-reference directory
    var api_dir = try dir.openDir("api-reference", .{});
    defer api_dir.close();
    const openapi = try api_dir.openFile("openapi.json", .{});
    openapi.close();

    // Check versions directory
    var ver_dir = try dir.openDir("versions", .{});
    ver_dir.close();
}

test "generateVersDocsIntegration produces Mintlify-compatible docs.json" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const output_dir = "./test-vers-docs-mintlify";
    defer std.fs.cwd().deleteTree(output_dir) catch {};

    const spec_path = "./test-vers-mintlify-spec.json";
    {
        const f = try std.fs.cwd().createFile(spec_path, .{});
        defer f.close();
        try f.writeAll("{}");
    }
    defer std.fs.cwd().deleteFile(spec_path) catch {};

    const config = docs.DocsConfig{
        .project_name = "mintlify-test",
        .description = "Mintlify test",
        .output_dir = output_dir,
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer generator.deinit();

    try generator.generateVersDocsIntegration(spec_path, output_dir);

    // Read and verify docs.json content
    const docs_json_path = try std.fs.path.join(allocator, &[_][]const u8{ output_dir, "docs.json" });
    defer allocator.free(docs_json_path);

    const file = try std.fs.cwd().openFile(docs_json_path, .{});
    defer file.close();
    const content = try file.readToEndAlloc(allocator, 1024 * 1024);
    defer allocator.free(content);

    // Verify key Mintlify fields
    try testing.expect(std.mem.indexOf(u8, content, "\"$schema\": \"https://mintlify.com/docs.json\"") != null);
    try testing.expect(std.mem.indexOf(u8, content, "\"theme\": \"linden\"") != null);
    try testing.expect(std.mem.indexOf(u8, content, "\"name\": \"mintlify-test - Documentation\"") != null);
    try testing.expect(std.mem.indexOf(u8, content, "\"tab\": \"SDKs\"") != null);
    try testing.expect(std.mem.indexOf(u8, content, "\"tab\": \"Versions\"") != null);
    try testing.expect(std.mem.indexOf(u8, content, "vers-docs") != null);
    try testing.expect(std.mem.indexOf(u8, content, "cross-reference") != null);
}

// =========================================================================
// updateVersionedDocs tests
// =========================================================================

test "updateVersionedDocs creates versioned directory and files" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const output_dir = "./test-vers-docs-versioned";
    std.fs.cwd().makePath(output_dir) catch {};
    std.fs.cwd().makePath(output_dir ++ "/versions") catch {};
    defer std.fs.cwd().deleteTree(output_dir) catch {};

    const config = docs.DocsConfig{
        .project_name = "versioned-test",
        .description = "Versioned test",
        .output_dir = output_dir,
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer {
        for (generator.version_history.items) |v| {
            allocator.free(v);
        }
        generator.deinit();
    }

    const sdk_paths = [_][]const u8{ "sdks/rust", "sdks/typescript" };
    try generator.updateVersionedDocs("1.0.0", &sdk_paths);

    // Verify version directory was created
    var ver_dir = try std.fs.cwd().openDir(output_dir ++ "/versions/1.0.0", .{});
    defer ver_dir.close();

    // Check manifest.json exists
    const manifest_file = try ver_dir.openFile("manifest.json", .{});
    defer manifest_file.close();
    const manifest_content = try manifest_file.readToEndAlloc(allocator, 1024 * 1024);
    defer allocator.free(manifest_content);
    try testing.expect(std.mem.indexOf(u8, manifest_content, "\"version\": \"1.0.0\"") != null);
    try testing.expect(std.mem.indexOf(u8, manifest_content, "sdks/rust") != null);

    // Check changelog.mdx exists
    const changelog_file = try ver_dir.openFile("changelog.mdx", .{});
    defer changelog_file.close();
    const changelog_content = try changelog_file.readToEndAlloc(allocator, 1024 * 1024);
    defer allocator.free(changelog_content);
    try testing.expect(std.mem.indexOf(u8, changelog_content, "Version 1.0.0") != null);

    // Check navigation.json exists
    const nav_file = try ver_dir.openFile("navigation.json", .{});
    nav_file.close();

    // Verify version was added to history
    try testing.expect(generator.version_history.items.len == 1);
    try testing.expectEqualStrings("1.0.0", generator.version_history.items[0]);
}

test "updateVersionedDocs handles multiple versions" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const output_dir = "./test-vers-docs-multi-ver";
    std.fs.cwd().makePath(output_dir) catch {};
    std.fs.cwd().makePath(output_dir ++ "/versions") catch {};
    defer std.fs.cwd().deleteTree(output_dir) catch {};

    const config = docs.DocsConfig{
        .project_name = "multi-ver-test",
        .description = "Multi-version test",
        .output_dir = output_dir,
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer {
        for (generator.version_history.items) |v| {
            allocator.free(v);
        }
        generator.deinit();
    }

    const sdk_paths = [_][]const u8{"sdks/rust"};
    try generator.updateVersionedDocs("1.0.0", &sdk_paths);
    try generator.updateVersionedDocs("1.1.0", &sdk_paths);
    try generator.updateVersionedDocs("2.0.0", &sdk_paths);

    try testing.expect(generator.version_history.items.len == 3);
    try testing.expectEqualStrings("1.0.0", generator.version_history.items[0]);
    try testing.expectEqualStrings("1.1.0", generator.version_history.items[1]);
    try testing.expectEqualStrings("2.0.0", generator.version_history.items[2]);

    // Verify each version directory exists
    var dir1 = try std.fs.cwd().openDir(output_dir ++ "/versions/1.0.0", .{});
    dir1.close();
    var dir2 = try std.fs.cwd().openDir(output_dir ++ "/versions/1.1.0", .{});
    dir2.close();
    var dir3 = try std.fs.cwd().openDir(output_dir ++ "/versions/2.0.0", .{});
    dir3.close();
}

// =========================================================================
// syncWithVersDocsRepo tests
// =========================================================================

test "syncWithVersDocsRepo creates sync manifest" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const output_dir = "./test-vers-docs-sync";
    std.fs.cwd().makePath(output_dir) catch {};
    defer std.fs.cwd().deleteTree(output_dir) catch {};

    const config = docs.DocsConfig{
        .project_name = "sync-test",
        .description = "Sync test",
        .output_dir = output_dir,
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer generator.deinit();

    const repo_url = "https://github.com/hdresearch/vers-docs";
    try generator.syncWithVersDocsRepo(repo_url);

    // Verify vers_config was set
    try testing.expect(generator.vers_config != null);
    try testing.expectEqualStrings(repo_url, generator.vers_config.?.repo_url);

    // Verify sync manifest was created
    const manifest_path = try std.fs.path.join(allocator, &[_][]const u8{ output_dir, ".vers-docs-sync.json" });
    defer allocator.free(manifest_path);

    const file = try std.fs.cwd().openFile(manifest_path, .{});
    defer file.close();
    const content = try file.readToEndAlloc(allocator, 1024 * 1024);
    defer allocator.free(content);

    try testing.expect(std.mem.indexOf(u8, content, "vers-docs") != null);
    try testing.expect(std.mem.indexOf(u8, content, "sync-test") != null);
    try testing.expect(std.mem.indexOf(u8, content, "\"mintlify_compatible\": true") != null);
    try testing.expect(std.mem.indexOf(u8, content, "\"sync_strategy\": \"pull_request\"") != null);
}

test "syncWithVersDocsRepo includes version history" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const output_dir = "./test-vers-docs-sync-hist";
    std.fs.cwd().makePath(output_dir) catch {};
    std.fs.cwd().makePath(output_dir ++ "/versions") catch {};
    defer std.fs.cwd().deleteTree(output_dir) catch {};

    const config = docs.DocsConfig{
        .project_name = "sync-hist-test",
        .description = "Sync history test",
        .output_dir = output_dir,
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer {
        for (generator.version_history.items) |v| {
            allocator.free(v);
        }
        generator.deinit();
    }

    // Add some version history
    const sdk_paths = [_][]const u8{"sdks/rust"};
    try generator.updateVersionedDocs("1.0.0", &sdk_paths);
    try generator.updateVersionedDocs("1.1.0", &sdk_paths);

    try generator.syncWithVersDocsRepo("https://github.com/hdresearch/vers-docs");

    // Read the sync manifest and check versions are included
    const manifest_path = try std.fs.path.join(allocator, &[_][]const u8{ output_dir, ".vers-docs-sync.json" });
    defer allocator.free(manifest_path);

    const file = try std.fs.cwd().openFile(manifest_path, .{});
    defer file.close();
    const content = try file.readToEndAlloc(allocator, 1024 * 1024);
    defer allocator.free(content);

    try testing.expect(std.mem.indexOf(u8, content, "1.0.0") != null);
    try testing.expect(std.mem.indexOf(u8, content, "1.1.0") != null);
}

// =========================================================================
// createDocsPullRequest tests
// =========================================================================

test "createDocsPullRequest generates correct PR metadata" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const config = docs.DocsConfig{
        .project_name = "pr-test",
        .description = "PR test",
        .output_dir = "./test-pr-output",
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer generator.deinit();

    const changed_files = [_][]const u8{ "docs.json", "overview.mdx", "sdks/rust.mdx" };
    const pr = try generator.createDocsPullRequest("2.0.0", &changed_files);
    defer {
        allocator.free(pr.title);
        allocator.free(pr.body);
        allocator.free(pr.branch);
    }

    try testing.expectEqualStrings("docs: update pr-test documentation to 2.0.0", pr.title);
    try testing.expect(std.mem.indexOf(u8, pr.branch, "docs/update-pr-test-2.0.0") != null);
    try testing.expectEqualStrings("main", pr.base_branch);

    // Check PR body contains changed files
    try testing.expect(std.mem.indexOf(u8, pr.body, "docs.json") != null);
    try testing.expect(std.mem.indexOf(u8, pr.body, "overview.mdx") != null);
    try testing.expect(std.mem.indexOf(u8, pr.body, "sdks/rust.mdx") != null);
    try testing.expect(std.mem.indexOf(u8, pr.body, "pr-test") != null);
    try testing.expect(std.mem.indexOf(u8, pr.body, "2.0.0") != null);
    try testing.expect(std.mem.indexOf(u8, pr.body, "sterling") != null);
}

// =========================================================================
// Cross-reference tests
// =========================================================================

test "generateCrossReferencePage creates cross-reference document" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const output_dir = "./test-vers-docs-crossref";
    defer std.fs.cwd().deleteTree(output_dir) catch {};

    const config = docs.DocsConfig{
        .project_name = "crossref-test",
        .description = "Cross-reference test",
        .output_dir = output_dir,
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer generator.deinit();

    try generator.generateCrossReferencePage(output_dir);

    const xref_path = try std.fs.path.join(allocator, &[_][]const u8{ output_dir, "sdks", "cross-reference.mdx" });
    defer allocator.free(xref_path);

    const file = try std.fs.cwd().openFile(xref_path, .{});
    defer file.close();
    const content = try file.readToEndAlloc(allocator, 1024 * 1024);
    defer allocator.free(content);

    // Verify cross-links are present
    try testing.expect(std.mem.indexOf(u8, content, "/sdks/rust") != null);
    try testing.expect(std.mem.indexOf(u8, content, "/sdks/typescript") != null);
    try testing.expect(std.mem.indexOf(u8, content, "/sdks/python") != null);
    try testing.expect(std.mem.indexOf(u8, content, "/sdks/go") != null);
    try testing.expect(std.mem.indexOf(u8, content, "SDK Cross-Reference") != null);
    try testing.expect(std.mem.indexOf(u8, content, "OpenAPI specification") != null);
}

// =========================================================================
// SDK doc page cross-linking tests
// =========================================================================

test "generateVersDocsIntegration SDK pages contain cross-links" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const output_dir = "./test-vers-docs-sdk-xlinks";
    defer std.fs.cwd().deleteTree(output_dir) catch {};

    const spec_path = "./test-vers-xlinks-spec.json";
    {
        const f = try std.fs.cwd().createFile(spec_path, .{});
        defer f.close();
        try f.writeAll("{}");
    }
    defer std.fs.cwd().deleteFile(spec_path) catch {};

    const config = docs.DocsConfig{
        .project_name = "xlink-test",
        .description = "Cross-link test",
        .output_dir = output_dir,
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer generator.deinit();

    try generator.generateVersDocsIntegration(spec_path, output_dir);

    // Read the Rust SDK page and verify it has cross-links to other SDKs
    const rust_path = try std.fs.path.join(allocator, &[_][]const u8{ output_dir, "sdks", "rust.mdx" });
    defer allocator.free(rust_path);

    const file = try std.fs.cwd().openFile(rust_path, .{});
    defer file.close();
    const content = try file.readToEndAlloc(allocator, 1024 * 1024);
    defer allocator.free(content);

    // Rust page should link to TS, Python, Go but NOT to itself
    try testing.expect(std.mem.indexOf(u8, content, "/sdks/typescript") != null);
    try testing.expect(std.mem.indexOf(u8, content, "/sdks/python") != null);
    try testing.expect(std.mem.indexOf(u8, content, "/sdks/go") != null);
    // Should contain the title
    try testing.expect(std.mem.indexOf(u8, content, "Rust SDK") != null);
}

// =========================================================================
// End-to-end workflow test
// =========================================================================

test "vers-docs full integration workflow" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const output_dir = "./test-vers-docs-e2e";
    defer std.fs.cwd().deleteTree(output_dir) catch {};

    // Step 1: Create a spec file
    const spec_path = "./test-vers-e2e-spec.json";
    {
        const f = try std.fs.cwd().createFile(spec_path, .{});
        defer f.close();
        try f.writeAll("{\"openapi\": \"3.0.0\", \"info\": {\"title\": \"E2E API\", \"version\": \"1.0.0\"}}");
    }
    defer std.fs.cwd().deleteFile(spec_path) catch {};

    const config = docs.DocsConfig{
        .project_name = "e2e-project",
        .description = "End-to-end test project",
        .output_dir = output_dir,
    };

    var generator = docs.DocsGenerator.init(allocator, config);
    defer {
        for (generator.version_history.items) |v| {
            allocator.free(v);
        }
        generator.deinit();
    }

    // Step 2: Generate initial vers-docs integration
    try generator.generateVersDocsIntegration(spec_path, output_dir);

    // Step 3: Generate cross-reference page
    try generator.generateCrossReferencePage(output_dir);

    // Step 4: Update with version 1.0.0
    const sdk_paths_v1 = [_][]const u8{ "sdks/rust", "sdks/typescript", "sdks/python", "sdks/go" };
    try generator.updateVersionedDocs("1.0.0", &sdk_paths_v1);

    // Step 5: Update with version 1.1.0
    const sdk_paths_v2 = [_][]const u8{ "sdks/rust", "sdks/go" };
    try generator.updateVersionedDocs("1.1.0", &sdk_paths_v2);

    // Step 6: Sync with vers-docs repo
    try generator.syncWithVersDocsRepo("https://github.com/hdresearch/vers-docs");

    // Step 7: Create a PR
    const changed_files = [_][]const u8{ "docs.json", "overview.mdx" };
    const pr = try generator.createDocsPullRequest("1.1.0", &changed_files);
    defer {
        allocator.free(pr.title);
        allocator.free(pr.body);
        allocator.free(pr.branch);
    }

    // Verify the full workflow produced expected state
    try testing.expect(generator.version_history.items.len == 2);
    try testing.expect(generator.vers_config != null);
    try testing.expect(std.mem.indexOf(u8, pr.title, "1.1.0") != null);

    // Verify files exist across the tree
    var dir = try std.fs.cwd().openDir(output_dir, .{});
    defer dir.close();

    // Root files
    const dj = try dir.openFile("docs.json", .{});
    dj.close();
    const ov = try dir.openFile("overview.mdx", .{});
    ov.close();
    const qs = try dir.openFile("quickstart.mdx", .{});
    qs.close();

    // Cross-reference
    var sdks = try dir.openDir("sdks", .{});
    defer sdks.close();
    const xref = try sdks.openFile("cross-reference.mdx", .{});
    xref.close();

    // Versioned dirs
    var v1 = try dir.openDir("versions/1.0.0", .{});
    v1.close();
    var v2 = try dir.openDir("versions/1.1.0", .{});
    v2.close();

    // Sync manifest
    const sync = try dir.openFile(".vers-docs-sync.json", .{});
    sync.close();
}
