const std = @import("std");

pub const DocsConfig = struct {
    project_name: []const u8,
    description: []const u8,
    output_dir: []const u8,
    theme: []const u8 = "linden",
};

pub const DocsGenerator = struct {
    allocator: std.mem.Allocator,
    config: DocsConfig,

    pub fn init(allocator: std.mem.Allocator, config: DocsConfig) DocsGenerator {
        return DocsGenerator{
            .allocator = allocator,
            .config = config,
        };
    }

    pub fn deinit(self: *DocsGenerator) void {
        _ = self;
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
};
