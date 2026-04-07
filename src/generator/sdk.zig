const std = @import("std");
pub const parser = @import("openapi");
pub const config = @import("config");
pub const template = @import("template.zig");

/// Load a template file at runtime from the current working directory.
fn loadTemplate(allocator: std.mem.Allocator, path: []const u8) ![]const u8 {
    return std.fs.cwd().readFileAlloc(allocator, path, 1024 * 1024);
}

pub const SDKGenerator = struct {
    allocator: std.mem.Allocator,
    spec: parser.OpenAPISpec,
    config: config.Config,

    pub fn init(allocator: std.mem.Allocator, spec: parser.OpenAPISpec, cfg: config.Config) SDKGenerator {
        return SDKGenerator{
            .allocator = allocator,
            .spec = spec,
            .config = cfg,
        };
    }

    pub fn generateAll(self: *SDKGenerator) !void {
        for (self.config.targets) |target| {
            try self.generateTarget(target);
        }
    }

    fn generateTarget(self: *SDKGenerator, target: config.Config.Target) !void {
        std.debug.print("Generating {s} SDK to {s}\n", .{ @tagName(target.language), target.output_dir });

        switch (target.language) {
            .typescript => try self.generateTypeScript(target),
            .rust => try self.generateRust(target),
            .python => try self.generatePython(target),
            .go => try self.generateGo(target),
            .zig => try self.generateZig(target),
        }
    }

    // ── Context building ────────────────────────────────────────────────

    /// Build a template context from the OpenAPI spec.
    fn buildBaseContext(self: *SDKGenerator) !*template.Context {
        const ctx = try self.allocator.create(template.Context);
        ctx.* = template.Context.init(self.allocator);
        try ctx.putString("spec_title", self.spec.info.title);
        try ctx.putString("spec_version", self.spec.info.version);
        try ctx.putString("project_name", self.config.project.name);
        try ctx.putString("project_version", self.config.project.version);
        try ctx.putString("default_base_url", "https://api.example.com");
        return ctx;
    }

    /// Build operation contexts from the spec's paths.
    fn buildOperationContexts(self: *SDKGenerator, parent: *const template.Context) ![]const *template.Context {
        // Count operations
        var count: usize = 0;
        var count_iter = self.spec.paths.iterator();
        while (count_iter.next()) |entry| {
            const path_item = entry.value_ptr;
            inline for (.{ "get", "post", "put", "delete", "patch" }) |method| {
                if (@field(path_item, method)) |op| {
                    if (op.operationId != null) count += 1;
                }
            }
        }

        const ops = try self.allocator.alloc(*template.Context, count);
        var idx: usize = 0;

        var path_iter = self.spec.paths.iterator();
        while (path_iter.next()) |entry| {
            const path_str = entry.key_ptr.*;
            const path_item = entry.value_ptr;

            inline for (.{ "get", "post", "put", "delete", "patch" }) |method| {
                if (@field(path_item, method)) |op| {
                    if (op.operationId) |op_id| {
                        const op_ctx = try self.allocator.create(template.Context);
                        op_ctx.* = template.Context.init(self.allocator);
                        op_ctx.parent = parent;

                        try op_ctx.putString("operationId", op_id);
                        try op_ctx.putString("summary", op.summary orelse "");
                        try op_ctx.putString("path", path_str);

                        // Method (uppercase)
                        const method_upper = comptime blk: {
                            var buf: [method.len]u8 = undefined;
                            for (method, 0..) |c, i| {
                                buf[i] = std.ascii.toUpper(c);
                            }
                            break :blk buf;
                        };
                        try op_ctx.putString("method", &method_upper);
                        try op_ctx.putString("method_lower", method);

                        // Snake case name
                        var snake_buf: [256]u8 = undefined;
                        const snake = toSnakeCaseStatic(op_id, &snake_buf);
                        const snake_owned = try self.allocator.dupe(u8, snake);
                        try op_ctx.putString("snake_name", snake_owned);

                        // Pascal case name
                        var pascal_buf: [256]u8 = undefined;
                        const pascal = toPascalCaseStatic(op_id, &pascal_buf);
                        const pascal_owned = try self.allocator.dupe(u8, pascal);
                        try op_ctx.putString("pascal_name", pascal_owned);

                        // Path params
                        const has_path_params = std.mem.indexOfScalar(u8, path_str, '{') != null;
                        try op_ctx.putBool("has_path_params", has_path_params);

                        // Format path for Rust format!() macro - replace {name} with {}
                        if (has_path_params) {
                            const fmt_path = try self.buildFmtPath(path_str);
                            try op_ctx.putString("fmt_path", fmt_path);
                        }

                        // Body
                        const has_body = std.mem.eql(u8, method, "post") or
                            std.mem.eql(u8, method, "put") or
                            std.mem.eql(u8, method, "patch");
                        try op_ctx.putBool("has_body", has_body);

                        // Function parameters string
                        const fn_params = try self.buildFnParams(has_path_params, has_body);
                        try op_ctx.putString("fn_params", fn_params);

                        ops[idx] = op_ctx;
                        idx += 1;
                    }
                }
            }
        }

        return @ptrCast(ops[0..idx]);
    }

    fn buildFmtPath(self: *SDKGenerator, path_str: []const u8) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var in_brace = false;
        for (path_str) |c| {
            if (c == '{') {
                in_brace = true;
                try buf.append('{');
            } else if (c == '}') {
                in_brace = false;
                try buf.append('}');
            } else if (!in_brace) {
                try buf.append(c);
            }
        }
        return try buf.toOwnedSlice();
    }

    fn buildFnParams(self: *SDKGenerator, has_path_params: bool, has_body: bool) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        try buf.appendSlice("&self");
        if (has_path_params) {
            try buf.appendSlice(", path_param: &str");
        }
        if (has_body) {
            try buf.appendSlice(", body: &impl Serialize");
        }
        return try buf.toOwnedSlice();
    }

    // ── TypeScript generation ───────────────────────────────────────────

    fn generateTypeScript(self: *SDKGenerator, target: config.Config.Target) !void {
        const output_dir = target.output_dir;
        
        // Create directory structure
        self.makeDirRecursive(output_dir) catch {};
        const src_dir = try std.fmt.allocPrint(self.allocator, "{s}/src", .{output_dir});
        defer self.allocator.free(src_dir);
        self.makeDirRecursive(src_dir) catch {};
        
        // Build context
        const ctx = try self.buildBaseContext();
        try ctx.putString("class_name", "PetStore");
        try ctx.putString("package_name", "petstore-sdk");
        try ctx.putString("base_url", "https://petstore.swagger.io/v2");
        
        // Build operations
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        
        // Build models
        const models = try self.buildModelContexts(ctx);
        try ctx.putList("models", models);
        
        // Generate client.ts
        const client_path = try std.fmt.allocPrint(self.allocator, "{s}/src/client.ts", .{output_dir});
        defer self.allocator.free(client_path);
        try self.renderTemplate("templates/typescript/client.ts.template", client_path, ctx);
        
        // Generate models.ts
        const models_path = try std.fmt.allocPrint(self.allocator, "{s}/src/models.ts", .{output_dir});
        defer self.allocator.free(models_path);
        try self.renderTemplate("templates/typescript/models.ts.template", models_path, ctx);
        
        // Generate index.ts
        const index_path = try std.fmt.allocPrint(self.allocator, "{s}/src/index.ts", .{output_dir});
        defer self.allocator.free(index_path);
        try self.renderTemplate("templates/typescript/index.ts.template", index_path, ctx);
        
        // Generate package.json
        
        // Generate tsconfig.json
        const tsconfig_path = try std.fmt.allocPrint(self.allocator, "{s}/tsconfig.json", .{output_dir});
        defer self.allocator.free(tsconfig_path);
        try self.renderTemplate("templates/typescript/tsconfig.json.template", tsconfig_path, ctx);
        
        // Generate package.json
        const package_path = try std.fmt.allocPrint(self.allocator, "{s}/package.json", .{output_dir});
        defer self.allocator.free(package_path);
        try self.renderTemplate("templates/typescript/package.json.template", package_path, ctx);
        
        // Generate README.md
        const readme_path = try std.fmt.allocPrint(self.allocator, "{s}/README.md", .{output_dir});
        defer self.allocator.free(readme_path);
        try self.renderTemplate("templates/typescript/README.md.template", readme_path, ctx);
        
        std.debug.print("Generated TypeScript SDK at {s}\n", .{output_dir});
    }
    // ── Rust generation ─────────────────────────────────────────────────

    pub fn generateRust(self: *SDKGenerator, target: config.Config.Target) !void {
        const output_dir = target.output_dir;

        self.makeDirRecursive(output_dir) catch {};
        const src_dir = try std.fmt.allocPrint(self.allocator, "{s}/src", .{output_dir});
        defer self.allocator.free(src_dir);
        self.makeDirRecursive(src_dir) catch {};

        try self.generateCargoToml(output_dir);
        try self.generateRustLib(src_dir);
        try self.generateRustModels(src_dir);
        try self.generateRustClient(src_dir);

        std.debug.print("Generated Rust SDK at {s}\n", .{output_dir});
    }

    fn makeDirRecursive(_: *SDKGenerator, path: []const u8) !void {
        var buf: [4096]u8 = undefined;
        var pos: usize = 0;
        var it = std.mem.splitScalar(u8, path, '/');
        while (it.next()) |comp| {
            if (comp.len == 0) continue;
            if (std.mem.eql(u8, comp, ".")) continue;
            if (pos > 0) {
                buf[pos] = '/';
                pos += 1;
            }
            @memcpy(buf[pos .. pos + comp.len], comp);
            pos += comp.len;
            std.fs.cwd().makeDir(buf[0..pos]) catch |err| switch (err) {
                error.PathAlreadyExists => {},
                else => return err,
            };
        }
    }

    fn generateCargoToml(self: *SDKGenerator, output_dir: []const u8) !void {
        const cargo_template =
            \\[package]
            \\name = "{{project_name}}"
            \\version = "{{project_version}}"
            \\edition = "2021"
            \\
            \\[dependencies]
            \\reqwest = { version = "0.11", features = ["json"] }
            \\serde = { version = "1", features = ["derive"] }
            \\serde_json = "1"
            \\tokio = { version = "1", features = ["full"] }
            \\thiserror = "1"
            \\
        ;

        const ctx = try self.buildBaseContext();
        var engine = template.Engine.init(self.allocator);
        const content = try engine.render(cargo_template, ctx);

        const file_path = try std.fmt.allocPrint(self.allocator, "{s}/Cargo.toml", .{output_dir});
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(content);
    }

    fn generateRustLib(self: *SDKGenerator, src_dir: []const u8) !void {
        const lib_template =
            \\// Generated by Sterling SDK Generator
            \\// {{spec_title}} v{{spec_version}}
            \\
            \\pub mod client;
            \\pub mod models;
            \\
            \\pub use client::Client;
            \\pub use models::*;
            \\
        ;

        const ctx = try self.buildBaseContext();
        var engine = template.Engine.init(self.allocator);
        const content = try engine.render(lib_template, ctx);

        const file_path = try std.fmt.allocPrint(self.allocator, "{s}/lib.rs", .{src_dir});
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(content);
    }

    fn generateRustModels(self: *SDKGenerator, src_dir: []const u8) !void {
        const models_header =
            \\// Generated by Sterling SDK Generator
            \\use serde::{Deserialize, Serialize};
            \\
            \\
        ;

        var buf = std.array_list.Managed(u8).init(self.allocator);
        defer buf.deinit();
        const writer = buf.writer();

        try writer.writeAll(models_header);

        // Generate model structs from operations
        var path_iter = self.spec.paths.iterator();
        while (path_iter.next()) |entry| {
            const path_item = entry.value_ptr;
            inline for (.{ "get", "post", "put", "delete", "patch" }) |method| {
                if (@field(path_item, method)) |op| {
                    if (op.operationId) |op_id| {
                        var name_buf: [256]u8 = undefined;
                        const model_name = toPascalCaseStatic(op_id, &name_buf);

                        try writer.print(
                            \\#[derive(Debug, Clone, Serialize, Deserialize)]
                            \\pub struct {s}Response {{
                            \\    #[serde(flatten)]
                            \\    pub data: serde_json::Value,
                            \\}}
                            \\
                            \\
                        , .{model_name});

                        if (std.mem.eql(u8, method, "post") or std.mem.eql(u8, method, "put") or std.mem.eql(u8, method, "patch")) {
                            try writer.print(
                                \\#[derive(Debug, Clone, Serialize, Deserialize)]
                                \\pub struct {s}Request {{
                                \\    #[serde(flatten)]
                                \\    pub data: serde_json::Value,
                                \\}}
                                \\
                                \\
                            , .{model_name});
                        }
                    }
                }
            }
        }

        const file_path = try std.fmt.allocPrint(self.allocator, "{s}/models.rs", .{src_dir});
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(buf.items);
    }

    fn generateRustClient(self: *SDKGenerator, src_dir: []const u8) !void {
        // Build template context
        const ctx = try self.buildBaseContext();

        // Build operations
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);

        // Load and render template
        const tmpl = try loadTemplate(self.allocator, "templates/rust/client.rs.template");
        defer self.allocator.free(tmpl);
        var engine = template.Engine.init(self.allocator);
        const content = try engine.render(tmpl, ctx);

        const file_path = try std.fmt.allocPrint(self.allocator, "{s}/client.rs", .{src_dir});
        defer self.allocator.free(file_path);

        const file = try std.fs.cwd().createFile(file_path, .{});
        defer file.close();
        try file.writeAll(content);
    }

    // ── Case conversion utilities ───────────────────────────────────────

    pub fn toSnakeCase(self: *SDKGenerator, input: []const u8, buf: *[256]u8) []const u8 {
        _ = self;
        return toSnakeCaseStatic(input, buf);
    }

    pub fn toPascalCase(self: *SDKGenerator, input: []const u8, buf: *[256]u8) []const u8 {
        _ = self;
        return toPascalCaseStatic(input, buf);
    }

    fn toSnakeCaseStatic(input: []const u8, buf: *[256]u8) []const u8 {
        var pos: usize = 0;
        for (input, 0..) |c, i| {
            if (std.ascii.isUpper(c)) {
                if (i > 0 and pos < 255) {
                    buf[pos] = '_';
                    pos += 1;
                }
                if (pos < 256) {
                    buf[pos] = std.ascii.toLower(c);
                    pos += 1;
                }
            } else {
                if (pos < 256) {
                    buf[pos] = c;
                    pos += 1;
                }
            }
        }
        return buf[0..pos];
    }

    fn toPascalCaseStatic(input: []const u8, buf: *[256]u8) []const u8 {
        var pos: usize = 0;
        var capitalize_next = true;
        for (input) |c| {
            if (c == '_' or c == '-') {
                capitalize_next = true;
                continue;
            }
            if (pos < 256) {
                buf[pos] = if (capitalize_next) std.ascii.toUpper(c) else c;
                pos += 1;
                capitalize_next = false;
            }
        }
        return buf[0..pos];
    }

    // ── Stub generators ─────────────────────────────────────────────────

    fn generatePython(self: *SDKGenerator, target: config.Config.Target) !void {
        const output_dir = target.output_dir;

        // Create directory structure: output_dir/src/
        self.makeDirRecursive(output_dir) catch {};
        const src_dir = try std.fmt.allocPrint(self.allocator, "{s}/src", .{output_dir});
        defer self.allocator.free(src_dir);
        self.makeDirRecursive(src_dir) catch {};

        // Build context
        const ctx = try self.buildBaseContext();
        try ctx.putString("class_name", "PetStore");
        try ctx.putString("package_name", "petstore_sdk");
        try ctx.putString("base_url", "https://petstore.swagger.io/v2");

        // Build operations
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);

        // Build models
        const models = try self.buildModelContexts(ctx);
        try ctx.putList("models", models);

        // Generate src/client.py
        const client_path = try std.fmt.allocPrint(self.allocator, "{s}/src/client.py", .{output_dir});
        defer self.allocator.free(client_path);
        try self.renderTemplate("templates/python/client.py.template", client_path, ctx);

        // Generate src/models.py
        const models_path = try std.fmt.allocPrint(self.allocator, "{s}/src/models.py", .{output_dir});
        defer self.allocator.free(models_path);
        try self.renderTemplate("templates/python/models.py.template", models_path, ctx);

        // Generate src/__init__.py
        const init_path = try std.fmt.allocPrint(self.allocator, "{s}/src/__init__.py", .{output_dir});
        defer self.allocator.free(init_path);
        try self.renderTemplate("templates/python/__init__.py.template", init_path, ctx);

        // Generate pyproject.toml
        const pyproject_path = try std.fmt.allocPrint(self.allocator, "{s}/pyproject.toml", .{output_dir});
        defer self.allocator.free(pyproject_path);
        try self.renderTemplate("templates/python/pyproject.toml.template", pyproject_path, ctx);

        // Generate README.md
        const readme_path = try std.fmt.allocPrint(self.allocator, "{s}/README.md", .{output_dir});
        defer self.allocator.free(readme_path);
        try self.renderTemplate("templates/python/README.md.template", readme_path, ctx);

        std.debug.print("Generated Python SDK at {s}\n", .{output_dir});
    }
    fn generateGo(self: *SDKGenerator, target: config.Config.Target) !void {
        const output_dir = target.output_dir;
        
        // Create directory structure
        self.makeDirRecursive(output_dir) catch {};
        
        // Build context
        const ctx = try self.buildBaseContext();
        try ctx.putString("package_name", "petstore");
        try ctx.putString("module_name", "github.com/example/petstore-sdk");
        try ctx.putString("go_version", "1.21");
        try ctx.putString("base_url", "https://petstore.swagger.io/v2");
        
        // Build operations
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        
        // Build models
        const models = try self.buildModelContexts(ctx);
        try ctx.putList("models", models);
        
        // Generate client.go
        const client_path = try std.fmt.allocPrint(self.allocator, "{s}/client.go", .{output_dir});
        defer self.allocator.free(client_path);
        try self.renderTemplate("templates/go/client.go.template", client_path, ctx);
        
        // Generate models.go
        const models_path = try std.fmt.allocPrint(self.allocator, "{s}/models.go", .{output_dir});
        defer self.allocator.free(models_path);
        try self.renderTemplate("templates/go/models.go.template", models_path, ctx);
        
        // Generate go.mod
        const gomod_path = try std.fmt.allocPrint(self.allocator, "{s}/go.mod", .{output_dir});
        defer self.allocator.free(gomod_path);
        try self.renderTemplate("templates/go/go.mod.template", gomod_path, ctx);
        
        // Generate README.md
        const readme_path = try std.fmt.allocPrint(self.allocator, "{s}/README.md", .{output_dir});
        defer self.allocator.free(readme_path);
        try self.renderTemplate("templates/go/README.md.template", readme_path, ctx);
        
        std.debug.print("Generated Go SDK at {s}\n", .{output_dir});
    }

    fn generateZig(self: *SDKGenerator, target: config.Config.Target) !void {
        const output_dir = target.output_dir;

        // Create directory structure
        self.makeDirRecursive(output_dir) catch {};
        const src_dir = try std.fmt.allocPrint(self.allocator, "{s}/src", .{output_dir});
        defer self.allocator.free(src_dir);
        self.makeDirRecursive(src_dir) catch {};

        // Build context
        const ctx = try self.buildBaseContext();
        try ctx.putString("base_url", "https://petstore.swagger.io/v2");

        // Build operations
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);

        // Generate client.zig
        const client_path = try std.fmt.allocPrint(self.allocator, "{s}/src/client.zig", .{output_dir});
        defer self.allocator.free(client_path);
        try self.renderTemplate("templates/zig/client.zig.template", client_path, ctx);

        // Generate build.zig
        const build_path = try std.fmt.allocPrint(self.allocator, "{s}/build.zig", .{output_dir});
        defer self.allocator.free(build_path);
        try self.renderTemplate("templates/zig/build.zig.template", build_path, ctx);

        // Generate README.md
        const readme_path = try std.fmt.allocPrint(self.allocator, "{s}/README.md", .{output_dir});
        defer self.allocator.free(readme_path);
        try self.renderTemplate("templates/zig/README.md.template", readme_path, ctx);

        std.debug.print("Generated Zig SDK at {s}\n", .{output_dir});
    }

    fn renderTemplate(self: *SDKGenerator, template_path: []const u8, output_path: []const u8, ctx: *template.Context) !void {
        const tmpl = try loadTemplate(self.allocator, template_path);
        defer self.allocator.free(tmpl);
        
        var engine = template.Engine.init(self.allocator);
        const content = try engine.render(tmpl, ctx);
        defer self.allocator.free(content);
        
        const file = try std.fs.cwd().createFile(output_path, .{});
        defer file.close();
        try file.writeAll(content);
    }
    
    fn buildModelContexts(self: *SDKGenerator, base_ctx: *template.Context) ![]const *template.Context {
        _ = self;
        _ = base_ctx;
        // For now, return empty models - this would be expanded to parse OpenAPI schemas
        const empty_models: []const *template.Context = &.{};
        return empty_models;
    }
};
