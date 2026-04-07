const std = @import("std");

pub const DocsConfig = struct {
    project_name: []const u8,
    description: []const u8,
    output_dir: []const u8,
    theme: []const u8 = "linden",
};

/// Supported SDK languages for cross-linking and documentation generation.
pub const SdkLanguage = enum {
    rust,
    typescript,
    python,
    go,

    pub fn toString(self: SdkLanguage) []const u8 {
        return switch (self) {
            .rust => "rust",
            .typescript => "typescript",
            .python => "python",
            .go => "go",
        };
    }

    pub fn displayName(self: SdkLanguage) []const u8 {
        return switch (self) {
            .rust => "Rust",
            .typescript => "TypeScript",
            .python => "Python",
            .go => "Go",
        };
    }
};

/// Represents a versioned documentation entry.
pub const VersionedDoc = struct {
    version: []const u8,
    sdk_language: SdkLanguage,
    content: []const u8,
    last_updated: []const u8,
};

/// Metadata for a vers-docs compatible documentation page.
pub const VersDocsPage = struct {
    title: []const u8,
    description: []const u8,
    slug: []const u8,
    group: []const u8,
    sdk_language: ?SdkLanguage = null,
    version: ?[]const u8 = null,
};

/// Configuration for vers-docs repository integration.
pub const VersDocsConfig = struct {
    repo_url: []const u8,
    branch: []const u8 = "main",
    docs_path: []const u8 = "docs",
    mint_config_path: []const u8 = "docs.json",
};

/// Represents a pull request for documentation updates.
pub const DocsPullRequest = struct {
    title: []const u8,
    body: []const u8,
    branch: []const u8,
    base_branch: []const u8 = "main",
    files_changed: []const []const u8,
};

pub const DocsGenerator = struct {
    allocator: std.mem.Allocator,
    config: DocsConfig,
    vers_config: ?VersDocsConfig = null,
    version_history: std.array_list.AlignedManaged([]const u8, null),

    pub fn init(allocator: std.mem.Allocator, config: DocsConfig) DocsGenerator {
        return DocsGenerator{
            .allocator = allocator,
            .config = config,
            .vers_config = null,
            .version_history = std.array_list.AlignedManaged([]const u8, null).init(allocator),
        };
    }

    pub fn deinit(self: *DocsGenerator) void {
        self.version_history.deinit();
    }

    /// Generate complete Mintlify documentation structure.
    pub fn generateDocs(self: *DocsGenerator, api_spec: []const u8, generated_sdks: []const []const u8) !void {
        // Create output directory
        std.fs.cwd().makeDir(self.config.output_dir) catch |err| switch (err) {
            error.PathAlreadyExists => {},
            else => return err,
        };

        try self.generateDocsJson();
        try self.generateOverviewPages();
        try self.generateApiReference(api_spec);

        for (generated_sdks) |sdk_path| {
            try self.generateSdkDocs(sdk_path);
        }
    }

    fn generateDocsJson(self: *DocsGenerator) !void {
        const docs_json = try std.fmt.allocPrint(self.allocator,
            \\{{
            \\  "$schema": "https://mintlify.com/docs.json",
            \\  "theme": "{s}",
            \\  "name": "{s} - Documentation",
            \\  "navigation": {{
            \\    "tabs": [
            \\      {{ "tab": "Getting Started", "groups": [{{ "group": "Introduction", "pages": ["overview", "quickstart"] }}] }},
            \\      {{ "tab": "SDKs", "groups": [{{ "group": "Language SDKs", "pages": ["sdks/rust", "sdks/typescript", "sdks/python", "sdks/go"] }}] }},
            \\      {{ "tab": "API Reference", "groups": [{{ "group": "Endpoints", "openapi": ["api-reference/openapi.json"] }}] }}
            \\    ]
            \\  }}
            \\}}
        , .{ self.config.theme, self.config.project_name });
        defer self.allocator.free(docs_json);

        const file_path = try std.fs.path.join(self.allocator, &[_][]const u8{ self.config.output_dir, "docs.json" });
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(docs_json);
    }

    fn generateOverviewPages(self: *DocsGenerator) !void {
        const overview = try std.fmt.allocPrint(self.allocator,
            \\---
            \\title: "{s} SDK Overview"
            \\description: "Comprehensive SDK for the {s} API"
            \\---
            \\
            \\# {s} SDK
            \\
            \\Multi-language SDK with support for Rust, TypeScript, Python, and Go.
            \\
            \\## Supported Languages
            \\
            \\- **Rust**: Type-safe, high-performance SDK
            \\- **TypeScript**: Modern JavaScript/TypeScript SDK
            \\- **Python**: Pythonic SDK with async support
            \\- **Go**: Idiomatic Go SDK
        , .{ self.config.project_name, self.config.project_name, self.config.project_name });
        defer self.allocator.free(overview);

        const overview_path = try std.fs.path.join(self.allocator, &[_][]const u8{ self.config.output_dir, "overview.mdx" });
        defer self.allocator.free(overview_path);

        const overview_file = try std.fs.cwd().createFile(overview_path, .{});
        defer overview_file.close();
        try overview_file.writeAll(overview);
    }

    fn generateApiReference(self: *DocsGenerator, api_spec: []const u8) !void {
        const api_ref_dir = try std.fs.path.join(self.allocator, &[_][]const u8{ self.config.output_dir, "api-reference" });
        defer self.allocator.free(api_ref_dir);

        std.fs.cwd().makeDir(api_ref_dir) catch |err| switch (err) {
            error.PathAlreadyExists => {},
            else => return err,
        };

        const openapi_path = try std.fs.path.join(self.allocator, &[_][]const u8{ api_ref_dir, "openapi.json" });
        defer self.allocator.free(openapi_path);

        const openapi_file = try std.fs.cwd().createFile(openapi_path, .{});
        defer openapi_file.close();
        try openapi_file.writeAll(api_spec);
    }

    fn generateSdkDocs(self: *DocsGenerator, sdk_path: []const u8) !void {
        _ = self;
        _ = sdk_path;
        // Generate language-specific documentation
    }

    // =========================================================================
    // vers-docs integration methods
    // =========================================================================

    /// Generate vers-docs compatible integration output from an OpenAPI spec.
    /// Creates Mintlify-compatible MDX files in the vers-docs repository structure.
    pub fn generateVersDocsIntegration(self: *DocsGenerator, spec_path: []const u8, output_path: []const u8) !void {
        // Read the spec file
        const spec_content = try readFileAlloc(self.allocator, spec_path);
        defer self.allocator.free(spec_content);

        // Create output directory structure matching vers-docs layout
        try makeDirRecursive(output_path);

        const sdks_dir = try std.fs.path.join(self.allocator, &[_][]const u8{ output_path, "sdks" });
        defer self.allocator.free(sdks_dir);
        try makeDirRecursive(sdks_dir);

        const api_dir = try std.fs.path.join(self.allocator, &[_][]const u8{ output_path, "api-reference" });
        defer self.allocator.free(api_dir);
        try makeDirRecursive(api_dir);

        const versions_dir = try std.fs.path.join(self.allocator, &[_][]const u8{ output_path, "versions" });
        defer self.allocator.free(versions_dir);
        try makeDirRecursive(versions_dir);

        // Generate the Mintlify docs.json for vers-docs
        try self.generateVersDocsJson(output_path);

        // Generate overview page
        try self.generateVersDocsOverview(output_path);

        // Generate quickstart page
        try self.generateVersDocsQuickstart(output_path);

        // Copy the OpenAPI spec into the api-reference folder
        const openapi_dest = try std.fs.path.join(self.allocator, &[_][]const u8{ api_dir, "openapi.json" });
        defer self.allocator.free(openapi_dest);
        const api_file = try std.fs.cwd().createFile(openapi_dest, .{});
        defer api_file.close();
        try api_file.writeAll(spec_content);

        // Generate per-language SDK doc stubs with cross-links
        const languages = [_]SdkLanguage{ .rust, .typescript, .python, .go };
        for (languages) |lang| {
            try self.generateSdkDocPage(sdks_dir, lang);
        }
    }

    /// Update versioned documentation when SDKs are regenerated.
    /// Records the version and regenerates docs for each SDK path.
    pub fn updateVersionedDocs(self: *DocsGenerator, version: []const u8, sdk_paths: []const []const u8) !void {
        // Record the version in history
        const ver_copy = try self.allocator.dupe(u8, version);
        try self.version_history.append(ver_copy);

        // Create the versioned output directory
        const versioned_dir = try std.fmt.allocPrint(self.allocator, "{s}/versions/{s}", .{ self.config.output_dir, version });
        defer self.allocator.free(versioned_dir);
        try makeDirRecursive(versioned_dir);

        // Generate a version manifest file
        try self.generateVersionManifest(version, sdk_paths);

        // Generate a changelog entry for this version
        try self.generateChangelogEntry(version, sdk_paths);

        // Update the navigation to include the new version
        try self.generateVersionedNavigation(version, sdk_paths);
    }

    /// Sync documentation with the vers-docs repository.
    /// Generates the commands/metadata needed to push to the remote repo.
    pub fn syncWithVersDocsRepo(self: *DocsGenerator, repo_url: []const u8) !void {
        self.vers_config = VersDocsConfig{
            .repo_url = repo_url,
            .branch = "main",
            .docs_path = "docs",
            .mint_config_path = "docs.json",
        };

        // Generate a sync manifest that describes what to push
        const sync_manifest = try self.generateSyncManifest(repo_url);
        defer self.allocator.free(sync_manifest);

        const manifest_path = try std.fs.path.join(self.allocator, &[_][]const u8{ self.config.output_dir, ".vers-docs-sync.json" });
        defer self.allocator.free(manifest_path);

        const file = try std.fs.cwd().createFile(manifest_path, .{});
        defer file.close();
        try file.writeAll(sync_manifest);
    }

    /// Build a Mintlify-compatible docs.json with vers-docs navigation structure.
    pub fn generateVersDocsJson(self: *DocsGenerator, output_path: []const u8) !void {
        const content = try std.fmt.allocPrint(self.allocator,
            \\{{
            \\  "$schema": "https://mintlify.com/docs.json",
            \\  "theme": "{s}",
            \\  "name": "{s} - Documentation",
            \\  "logo": {{
            \\    "light": "/logo/light.svg",
            \\    "dark": "/logo/dark.svg"
            \\  }},
            \\  "favicon": "/favicon.svg",
            \\  "colors": {{
            \\    "primary": "#0D9373",
            \\    "light": "#07C983",
            \\    "dark": "#0D9373"
            \\  }},
            \\  "navigation": {{
            \\    "tabs": [
            \\      {{
            \\        "tab": "Getting Started",
            \\        "groups": [
            \\          {{
            \\            "group": "Introduction",
            \\            "pages": ["overview", "quickstart"]
            \\          }}
            \\        ]
            \\      }},
            \\      {{
            \\        "tab": "SDKs",
            \\        "groups": [
            \\          {{
            \\            "group": "Language SDKs",
            \\            "pages": ["sdks/rust", "sdks/typescript", "sdks/python", "sdks/go"]
            \\          }},
            \\          {{
            \\            "group": "Cross-Reference",
            \\            "pages": ["sdks/cross-reference"]
            \\          }}
            \\        ]
            \\      }},
            \\      {{
            \\        "tab": "API Reference",
            \\        "groups": [
            \\          {{
            \\            "group": "Endpoints",
            \\            "openapi": ["api-reference/openapi.json"]
            \\          }}
            \\        ]
            \\      }},
            \\      {{
            \\        "tab": "Versions",
            \\        "groups": [
            \\          {{
            \\            "group": "Version History",
            \\            "pages": ["versions/changelog"]
            \\          }}
            \\        ]
            \\      }}
            \\    ]
            \\  }},
            \\  "footerSocials": {{
            \\    "github": "https://github.com/hdresearch/vers-docs"
            \\  }}
            \\}}
        , .{ self.config.theme, self.config.project_name });
        defer self.allocator.free(content);

        const file_path = try std.fs.path.join(self.allocator, &[_][]const u8{ output_path, "docs.json" });
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(content);
    }

    /// Generate the overview page for vers-docs.
    fn generateVersDocsOverview(self: *DocsGenerator, output_path: []const u8) !void {
        const content = try std.fmt.allocPrint(self.allocator,
            \\---
            \\title: "{s} SDK Overview"
            \\description: "{s}"
            \\---
            \\
            \\# {s}
            \\
            \\{s}
            \\
            \\## Quick Links
            \\
            \\<CardGroup cols={{2}}>
            \\  <Card title="Rust SDK" icon="rust" href="/sdks/rust">
            \\    Type-safe, high-performance Rust SDK
            \\  </Card>
            \\  <Card title="TypeScript SDK" icon="js" href="/sdks/typescript">
            \\    Modern TypeScript SDK with full type support
            \\  </Card>
            \\  <Card title="Python SDK" icon="python" href="/sdks/python">
            \\    Pythonic SDK with async support
            \\  </Card>
            \\  <Card title="Go SDK" icon="golang" href="/sdks/go">
            \\    Idiomatic Go SDK
            \\  </Card>
            \\</CardGroup>
            \\
            \\## Version History
            \\
            \\See the [changelog](/versions/changelog) for the latest updates.
        , .{ self.config.project_name, self.config.description, self.config.project_name, self.config.description });
        defer self.allocator.free(content);

        const file_path = try std.fs.path.join(self.allocator, &[_][]const u8{ output_path, "overview.mdx" });
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(content);
    }

    /// Generate quickstart page for vers-docs.
    fn generateVersDocsQuickstart(self: *DocsGenerator, output_path: []const u8) !void {
        const content = try std.fmt.allocPrint(self.allocator,
            \\---
            \\title: "Quickstart"
            \\description: "Get started with the {s} SDK in minutes"
            \\---
            \\
            \\# Quickstart
            \\
            \\## Installation
            \\
            \\<CodeGroup>
            \\```bash Rust
            \\cargo add {s}
            \\```
            \\
            \\```bash TypeScript
            \\npm install {s}
            \\```
            \\
            \\```bash Python
            \\pip install {s}
            \\```
            \\
            \\```bash Go
            \\go get github.com/hdresearch/{s}
            \\```
            \\</CodeGroup>
        , .{ self.config.project_name, self.config.project_name, self.config.project_name, self.config.project_name, self.config.project_name });
        defer self.allocator.free(content);

        const file_path = try std.fs.path.join(self.allocator, &[_][]const u8{ output_path, "quickstart.mdx" });
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(content);
    }

    /// Generate an individual SDK documentation page with cross-links.
    fn generateSdkDocPage(self: *DocsGenerator, sdks_dir: []const u8, language: SdkLanguage) !void {
        const lang_str = language.toString();
        const lang_display = language.displayName();

        // Build cross-links to other SDKs
        const cross_links = try self.buildCrossLinks(language);
        defer self.allocator.free(cross_links);

        const content = try std.fmt.allocPrint(self.allocator,
            \\---
            \\title: "{s} SDK"
            \\description: "{s} SDK for the {s} API"
            \\---
            \\
            \\# {s} SDK
            \\
            \\## Installation
            \\
            \\See the [quickstart guide](/quickstart) for installation instructions.
            \\
            \\## Usage
            \\
            \\Refer to the [API Reference](/api-reference/openapi.json) for available endpoints.
            \\
            \\## Other SDKs
            \\
            \\{s}
        , .{ lang_display, lang_display, self.config.project_name, lang_display, cross_links });
        defer self.allocator.free(content);

        const filename = try std.fmt.allocPrint(self.allocator, "{s}.mdx", .{lang_str});
        defer self.allocator.free(filename);

        const file_path = try std.fs.path.join(self.allocator, &[_][]const u8{ sdks_dir, filename });
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(content);
    }

    /// Build cross-reference links to other SDK pages (excluding the given language).
    fn buildCrossLinks(self: *DocsGenerator, exclude: SdkLanguage) ![]const u8 {
        var links = std.array_list.Managed(u8).init(self.allocator);
        defer links.deinit();

        const languages = [_]SdkLanguage{ .rust, .typescript, .python, .go };
        for (languages) |lang| {
            if (lang == exclude) continue;
            const line = try std.fmt.allocPrint(self.allocator, "- [{s} SDK](/sdks/{s})\n", .{ lang.displayName(), lang.toString() });
            defer self.allocator.free(line);
            try links.appendSlice(line);
        }

        return self.allocator.dupe(u8, links.items);
    }

    /// Generate a version manifest JSON file for a specific version.
    fn generateVersionManifest(self: *DocsGenerator, version: []const u8, sdk_paths: []const []const u8) !void {
        var manifest = std.array_list.Managed(u8).init(self.allocator);
        defer manifest.deinit();

        try manifest.appendSlice("{\n  \"version\": \"");
        try manifest.appendSlice(version);
        try manifest.appendSlice("\",\n  \"sdks\": [\n");

        for (sdk_paths, 0..) |path, i| {
            try manifest.appendSlice("    \"");
            try manifest.appendSlice(path);
            try manifest.appendSlice("\"");
            if (i < sdk_paths.len - 1) {
                try manifest.appendSlice(",");
            }
            try manifest.appendSlice("\n");
        }

        try manifest.appendSlice("  ],\n  \"generated_by\": \"sterling-docs-generator\"\n}");

        const versioned_dir = try std.fmt.allocPrint(self.allocator, "{s}/versions/{s}", .{ self.config.output_dir, version });
        defer self.allocator.free(versioned_dir);

        const manifest_path = try std.fs.path.join(self.allocator, &[_][]const u8{ versioned_dir, "manifest.json" });
        defer self.allocator.free(manifest_path);

        const file = try std.fs.cwd().createFile(manifest_path, .{});
        defer file.close();
        try file.writeAll(manifest.items);
    }

    /// Generate a changelog entry for a new version.
    fn generateChangelogEntry(self: *DocsGenerator, version: []const u8, sdk_paths: []const []const u8) !void {
        var content = std.array_list.Managed(u8).init(self.allocator);
        defer content.deinit();

        const header = try std.fmt.allocPrint(self.allocator,
            \\---
            \\title: "Version {s}"
            \\description: "Changelog for version {s}"
            \\---
            \\
            \\# Version {s}
            \\
            \\## Updated SDKs
            \\
            \\
        , .{ version, version, version });
        defer self.allocator.free(header);
        try content.appendSlice(header);

        for (sdk_paths) |path| {
            try content.appendSlice("- `");
            try content.appendSlice(path);
            try content.appendSlice("`\n");
        }

        const versioned_dir = try std.fmt.allocPrint(self.allocator, "{s}/versions/{s}", .{ self.config.output_dir, version });
        defer self.allocator.free(versioned_dir);

        const changelog_path = try std.fs.path.join(self.allocator, &[_][]const u8{ versioned_dir, "changelog.mdx" });
        defer self.allocator.free(changelog_path);

        const file = try std.fs.cwd().createFile(changelog_path, .{});
        defer file.close();
        try file.writeAll(content.items);
    }

    /// Generate versioned navigation that includes SDK version tabs.
    fn generateVersionedNavigation(self: *DocsGenerator, version: []const u8, sdk_paths: []const []const u8) !void {
        _ = sdk_paths;
        const content = try std.fmt.allocPrint(self.allocator,
            \\{{
            \\  "version": "{s}",
            \\  "navigation": {{
            \\    "tabs": [
            \\      {{
            \\        "tab": "Version {s}",
            \\        "groups": [
            \\          {{
            \\            "group": "Changelog",
            \\            "pages": ["versions/{s}/changelog"]
            \\          }}
            \\        ]
            \\      }}
            \\    ]
            \\  }}
            \\}}
        , .{ version, version, version });
        defer self.allocator.free(content);

        const versioned_dir = try std.fmt.allocPrint(self.allocator, "{s}/versions/{s}", .{ self.config.output_dir, version });
        defer self.allocator.free(versioned_dir);

        const nav_path = try std.fs.path.join(self.allocator, &[_][]const u8{ versioned_dir, "navigation.json" });
        defer self.allocator.free(nav_path);

        const file = try std.fs.cwd().createFile(nav_path, .{});
        defer file.close();
        try file.writeAll(content);
    }

    /// Generate a sync manifest describing what should be pushed to vers-docs.
    fn generateSyncManifest(self: *DocsGenerator, repo_url: []const u8) ![]const u8 {
        var content = std.array_list.Managed(u8).init(self.allocator);
        defer content.deinit();

        const header = try std.fmt.allocPrint(self.allocator,
            \\{{
            \\  "repo_url": "{s}",
            \\  "project": "{s}",
            \\  "output_dir": "{s}",
            \\  "versions": [
        , .{ repo_url, self.config.project_name, self.config.output_dir });
        defer self.allocator.free(header);
        try content.appendSlice(header);

        // Include version history
        for (self.version_history.items, 0..) |ver, i| {
            const entry = try std.fmt.allocPrint(self.allocator, "\n    \"{s}\"", .{ver});
            defer self.allocator.free(entry);
            try content.appendSlice(entry);
            if (i < self.version_history.items.len - 1) {
                try content.appendSlice(",");
            }
        }

        try content.appendSlice(
            \\
            \\  ],
            \\  "sync_strategy": "pull_request",
            \\  "mintlify_compatible": true
            \\}
        );

        return self.allocator.dupe(u8, content.items);
    }

    /// Create a pull request description for documentation updates.
    pub fn createDocsPullRequest(self: *DocsGenerator, version: []const u8, changed_files: []const []const u8) !DocsPullRequest {
        const title = try std.fmt.allocPrint(self.allocator, "docs: update {s} documentation to {s}", .{ self.config.project_name, version });
        const branch = try std.fmt.allocPrint(self.allocator, "docs/update-{s}-{s}", .{ self.config.project_name, version });

        var body = std.array_list.Managed(u8).init(self.allocator);
        defer body.deinit();

        const body_header = try std.fmt.allocPrint(self.allocator,
            \\## Documentation Update
            \\
            \\Automated documentation update for **{s}** version **{s}**.
            \\
            \\### Changed Files
            \\
            \\
        , .{ self.config.project_name, version });
        defer self.allocator.free(body_header);
        try body.appendSlice(body_header);

        for (changed_files) |f| {
            try body.appendSlice("- `");
            try body.appendSlice(f);
            try body.appendSlice("`\n");
        }

        try body.appendSlice("\n---\n*Generated by sterling docs generator*\n");

        const body_owned = try self.allocator.dupe(u8, body.items);

        return DocsPullRequest{
            .title = title,
            .body = body_owned,
            .branch = branch,
            .base_branch = "main",
            .files_changed = changed_files,
        };
    }

    /// Generate cross-reference documentation page linking all SDK docs together.
    pub fn generateCrossReferencePage(self: *DocsGenerator, output_path: []const u8) !void {
        const content = try std.fmt.allocPrint(self.allocator,
            \\---
            \\title: "SDK Cross-Reference"
            \\description: "Cross-reference guide between {s} SDK implementations"
            \\---
            \\
            \\# SDK Cross-Reference
            \\
            \\This page provides a cross-reference between the different SDK implementations
            \\for the {s} API.
            \\
            \\## SDK Comparison
            \\
            \\| Feature | [Rust](/sdks/rust) | [TypeScript](/sdks/typescript) | [Python](/sdks/python) | [Go](/sdks/go) |
            \\|---------|------|------------|--------|-----|
            \\| Async Support | ✅ | ✅ | ✅ | ✅ |
            \\| Type Safety | ✅ | ✅ | ⚠️ | ✅ |
            \\| Auto-generated | ✅ | ✅ | ✅ | ✅ |
            \\
            \\## API Reference
            \\
            \\All SDKs are generated from the same [OpenAPI specification](/api-reference/openapi.json).
        , .{ self.config.project_name, self.config.project_name });
        defer self.allocator.free(content);

        const sdks_dir = try std.fs.path.join(self.allocator, &[_][]const u8{ output_path, "sdks" });
        defer self.allocator.free(sdks_dir);

        try makeDirRecursive(sdks_dir);

        const file_path = try std.fs.path.join(self.allocator, &[_][]const u8{ sdks_dir, "cross-reference.mdx" });
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(content);
    }

    // =========================================================================
    // Helpers
    // =========================================================================

    fn readFileAlloc(allocator: std.mem.Allocator, path: []const u8) ![]const u8 {
        const file = std.fs.cwd().openFile(path, .{}) catch |err| {
            return err;
        };
        defer file.close();

        const stat = try file.stat();
        const size = stat.size;
        if (size == 0) return allocator.dupe(u8, "");

        const buf = try allocator.alloc(u8, size);
        const bytes_read = try file.readAll(buf);
        if (bytes_read < size) {
            const result = try allocator.dupe(u8, buf[0..bytes_read]);
            allocator.free(buf);
            return result;
        }
        return buf;
    }

    fn makeDirRecursive(path: []const u8) !void {
        std.fs.cwd().makePath(path) catch |err| switch (err) {
            error.PathAlreadyExists => {},
            else => return err,
        };
    }
};
