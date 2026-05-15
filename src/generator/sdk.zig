const std = @import("std");
pub const parser = @import("../parser/openapi.zig");
pub const config = @import("../config/config.zig");
pub const template = @import("template.zig");
pub const enhancer_mod = @import("../llm/enhancer.zig");

fn loadTemplate(allocator: std.mem.Allocator, path: []const u8) ![]const u8 {
    return std.fs.cwd().readFileAlloc(allocator, path, 1024 * 1024);
}

pub const SDKGenerator = struct {
    allocator: std.mem.Allocator,
    spec: parser.OpenAPISpec,
    cfg: config.Config,
    enhance: bool = false,
    enhancer: ?enhancer_mod.Enhancer = null,

    pub fn init(allocator: std.mem.Allocator, spec: parser.OpenAPISpec, cfg: config.Config) SDKGenerator {
        return .{ .allocator = allocator, .spec = spec, .cfg = cfg };
    }

    pub fn enableEnhancement(self: *SDKGenerator, api_key: []const u8, model: []const u8) void {
        self.enhance = true;
        self.enhancer = enhancer_mod.Enhancer.init(self.allocator, .{
            .api_key = api_key,
            .model = model,
        });
    }

    pub fn generateAll(self: *SDKGenerator) !void {
        for (self.cfg.targets) |target| {
            try self.generateTarget(target);
        }
    }

    pub fn generateTarget(self: *SDKGenerator, target: config.Config.Target) !void {
        switch (target.language) {
            .typescript => try self.generateTypeScript(target),
            .rust => try self.generateRust(target),
            .python => try self.generatePython(target),
            .go => try self.generateGo(target),
            .zig => try self.generateZig(target),
            .java => try self.generateJava(target),
            .kotlin => try self.generateKotlin(target),
            .ruby => try self.generateRuby(target),
            .php => try self.generatePhp(target),
            .csharp => try self.generateCsharp(target),
            .dart => try self.generateDart(target),
            .scala => try self.generateScala(target),
            .swift => try self.generateSwift(target),
        }
    }

    // ── Derive names from spec + config ─────────────────────────────────

    fn deriveClassName(self: *SDKGenerator) []const u8 {
        // Use project name from config, PascalCased
        var buf: [256]u8 = undefined;
        return self.allocator.dupe(u8, toPascalCaseStatic(self.cfg.project.name, &buf)) catch "Client";
    }

    fn deriveBaseUrl(self: *SDKGenerator) []const u8 {
        // Use first server URL from spec if available, else fallback
        if (self.spec.servers.items.len > 0) return self.spec.servers.items[0];
        return "https://api.vers.sh";
    }

    fn derivePackageName(self: *SDKGenerator, lang: []const u8) []const u8 {
        if (std.mem.eql(u8, lang, "go")) {
            // Go modules use the GitHub repository path
            for (self.cfg.targets) |t| {
                if (t.language == .go) {
                    if (t.repository.len > 0) {
                        return std.fmt.allocPrint(self.allocator, "github.com/{s}", .{t.repository}) catch self.cfg.project.name;
                    }
                }
            }
        }
        return self.cfg.project.name;
    }

    // ── Context building ────────────────────────────────────────────────

    fn buildBaseContext(self: *SDKGenerator) !*template.Context {
        const ctx = try self.allocator.create(template.Context);
        ctx.* = template.Context.init(self.allocator);
        try ctx.putString("spec_title", self.spec.info.title);
        try ctx.putString("spec_version", self.spec.info.version);
        try ctx.putString("project_name", self.cfg.project.name);
        try ctx.putString("project_version", self.cfg.project.version);

        const class_name = self.deriveClassName();
        try ctx.putString("class_name", class_name);
        try ctx.putString("base_url", self.deriveBaseUrl());
        try ctx.putString("package_name", self.derivePackageName(""));
        try ctx.putString("module_name", self.derivePackageName("go"));
        // Go package names can't have hyphens — replace with underscores
        const pkg = self.derivePackageName("");
        var go_pkg_buf: [256]u8 = undefined;
        var go_pkg_len: usize = 0;
        for (pkg) |c| {
            if (go_pkg_len < 256) {
                go_pkg_buf[go_pkg_len] = if (c == '-') '_' else c;
                go_pkg_len += 1;
            }
        }
        try ctx.putString("go_package_name", try self.allocator.dupe(u8, go_pkg_buf[0..go_pkg_len]));
        try ctx.putString("go_version", "1.21");
        return ctx;
    }

    fn buildOperationContexts(self: *SDKGenerator, parent: *const template.Context) ![]const *template.Context {
        var count: usize = 0;
        var count_iter = self.spec.paths.iterator();
        while (count_iter.next()) |entry| {
            const pi = entry.value_ptr;
            inline for (.{ "get", "post", "put", "delete", "patch" }) |m| {
                if (@field(pi, m)) |op| {
                    if (op.operationId != null) count += 1;
                }
            }
        }

        const ops = try self.allocator.alloc(*template.Context, count);
        var idx: usize = 0;

        // Track seen operationIds to skip duplicates (e.g. /vm and /vms both list_vms)
        var seen_ops = std.StringHashMap(void).init(self.allocator);
        defer seen_ops.deinit();

        var path_iter = self.spec.paths.iterator();
        while (path_iter.next()) |entry| {
            const path_str = entry.key_ptr.*;
            const pi = entry.value_ptr;

            inline for (.{ "get", "post", "put", "delete", "patch" }) |method| {
                if (@field(pi, method)) |op| {
                    if (op.operationId) |op_id| {
                        // Skip duplicate operationIds
                        if (seen_ops.contains(op_id)) {
                            std.debug.print("⚠️  Skipping duplicate operationId: {s} ({s} {s})\n", .{ op_id, method, path_str });
                        } else {
                        try seen_ops.put(op_id, {});
                        const c = try self.allocator.create(template.Context);
                        c.* = template.Context.init(self.allocator);
                        c.parent = parent;

                        try c.putString("operationId", op_id);
                        try c.putString("summary", self.sanitiseOneLine(op.summary orelse ""));
                        try c.putString("path", path_str);

                        const method_upper = comptime blk: {
                            var buf: [method.len]u8 = undefined;
                            for (method, 0..) |ch, i| buf[i] = std.ascii.toUpper(ch);
                            break :blk buf;
                        };
                        try c.putString("method", &method_upper);
                        try c.putString("method_lower", method);

                        var snake_buf: [256]u8 = undefined;
                        try c.putString("snake_name", try self.allocator.dupe(u8, toSnakeCaseStatic(op_id, &snake_buf)));
                        var pascal_buf: [256]u8 = undefined;
                        try c.putString("pascal_name", try self.allocator.dupe(u8, toPascalCaseStatic(op_id, &pascal_buf)));
                        var camel_buf: [256]u8 = undefined;
                        try c.putString("camel_name", try self.allocator.dupe(u8, toCamelCaseStatic(op_id, &camel_buf)));

                        // Path params
                        const has_path_params = std.mem.indexOfScalar(u8, path_str, '{') != null;
                        try c.putBool("has_path_params", has_path_params);

                        // Build path param names list for function signatures
                        if (has_path_params) {
                            const param_names = try self.extractPathParamNames(path_str);
                            try c.putString("path_params_ts", param_names.ts_params);
                            try c.putString("path_params_ts_args", param_names.ts_args);
                            try c.putString("path_params_py", param_names.py_params);
                            try c.putString("path_params_go", param_names.go_params);
                            try c.putString("path_params_rust", param_names.rust_params);
                            try c.putString("path_interpolate_ts", param_names.ts_interpolate);
                            try c.putString("path_interpolate_py", param_names.py_interpolate);
                            try c.putString("path_interpolate_go", param_names.go_interpolate);
                            try c.putString("path_interpolate_rust", param_names.rust_interpolate);
                            try c.putString("go_format_args", param_names.go_format_args);
                            try c.putString("rust_format_args", param_names.rust_format_args);
                            // New language path params
                            try c.putString("path_params_java", param_names.java_params);
                            try c.putString("path_params_kotlin", param_names.kotlin_params);
                            try c.putString("path_params_php", param_names.php_params);
                            try c.putString("path_params_csharp", param_names.csharp_params);
                            try c.putString("path_interpolate_csharp", param_names.csharp_interpolate);
                            try c.putString("php_format_args", param_names.php_format_args);
                            // Dart, Scala, Swift path params
                            try c.putString("path_params_dart", param_names.dart_params);
                            try c.putString("path_interpolate_dart", param_names.dart_interpolate);
                            try c.putString("path_params_scala", param_names.scala_params);
                            try c.putString("path_interpolate_scala", param_names.scala_interpolate);
                            try c.putString("path_params_swift", param_names.swift_params);
                            try c.putString("path_interpolate_swift", param_names.swift_interpolate);
                            try c.putString("path_params_zig", param_names.zig_params);
                            try c.putString("path_interpolate_zig", param_names.zig_interpolate);
                            try c.putString("zig_format_args", param_names.zig_format_args);
                        }

                        // Body
                        const has_body = std.mem.eql(u8, method, "post") or
                            std.mem.eql(u8, method, "put") or
                            std.mem.eql(u8, method, "patch");
                        try c.putBool("has_body", has_body);

                        // Request body type from $ref
                        if (op.requestBody) |rb| {
                            if (rb.schema_ref) |ref| {
                                try c.putString("request_type", ref);
                                try c.putBool("has_typed_body", true);
                            } else {
                                try c.putBool("has_typed_body", false);
                            }
                        } else {
                            try c.putBool("has_typed_body", false);
                        }

                        // Response type from success response $ref
                        var response_type: []const u8 = "";
                        var is_array_response = false;
                        var array_item_type: []const u8 = "";
                        var resp_iter = op.responses.iterator();
                        while (resp_iter.next()) |re| {
                            // Look for 200/201 response with schema ref
                            if (std.mem.startsWith(u8, re.key_ptr.*, "2")) {
                                if (re.value_ptr.schema_ref) |ref| {
                                    response_type = ref;
                                    break;
                                }
                                // Handle array responses: type: array, items: { $ref: ... }
                                if (re.value_ptr.is_array) {
                                    is_array_response = true;
                                    if (re.value_ptr.array_item_ref) |item_ref| {
                                        array_item_type = item_ref;
                                    }
                                    break;
                                }
                            }
                        }
                        if (response_type.len > 0) {
                            try c.putString("response_type", response_type);
                            try c.putBool("has_typed_response", true);
                            try c.putBool("is_array_response", false);
                        } else if (is_array_response and array_item_type.len > 0) {
                            try c.putString("response_type", array_item_type);
                            try c.putBool("has_typed_response", true);
                            try c.putBool("is_array_response", true);
                        } else {
                            try c.putBool("has_typed_response", false);
                            try c.putBool("is_array_response", false);
                        }

                        // Query params
                        var query_params_list = std.array_list.Managed(*template.Context).init(self.allocator);
                        for (op.parameters.items) |param| {
                            if (param.in == .query) {
                                const qc = try self.allocator.create(template.Context);
                                qc.* = template.Context.init(self.allocator);
                                qc.parent = c;
                                try qc.putString("name", param.name);
                                var qp_pascal_buf: [256]u8 = undefined;
                                try qc.putString("pascal_name", try self.allocator.dupe(u8, toPascalCaseStatic(param.name, &qp_pascal_buf)));
                                try qc.putString("description", self.sanitiseOneLine(param.description orelse ""));
                                try qc.putBool("required", param.required);
                                const schema_type = param.schema_type orelse "string";
                                try qc.putString("ts_type", self.queryParamTypeTS(schema_type));
                                try qc.putString("py_type", self.queryParamTypePython(schema_type));
                                try qc.putString("go_type", self.queryParamTypeGo(schema_type));
                                try qc.putString("rust_type", self.queryParamTypeRust(schema_type));
                                try qc.putString("java_type", self.queryParamTypeJava(schema_type));
                                try qc.putString("kotlin_type", self.queryParamTypeKotlin(schema_type));
                                try qc.putString("ruby_type", self.queryParamTypeRuby(schema_type));
                                try qc.putString("php_type", self.queryParamTypePhp(schema_type));
                                try qc.putString("csharp_type", self.queryParamTypeCsharp(schema_type));
                                try qc.putString("dart_type", self.queryParamTypeDart(schema_type));
                                try qc.putString("scala_type", self.queryParamTypeScala(schema_type));
                                try qc.putString("swift_type", self.queryParamTypeSwift(schema_type));
                                try qc.putString("zig_type", self.queryParamTypeZig(schema_type));
                                // Go format helper: how to convert to string for URL
                                try qc.putString("go_format", self.queryParamGoFormat(param.name, schema_type));
                                try query_params_list.append(qc);
                            }
                        }
                        const has_query_params = query_params_list.items.len > 0;
                        try c.putBool("has_query_params", has_query_params);
                        try c.putBool("query_needs_comma", has_query_params and (has_path_params or has_body));
                        try c.putBool("has_any_params", has_path_params or has_body or has_query_params);
                        if (has_query_params) {
                            const qp_slice = try query_params_list.toOwnedSlice();
                            try c.putList("query_params", @ptrCast(qp_slice));
                            // Build per-language param strings for function signatures
                            try c.putString("query_params_ts", try self.buildQueryParamStringTS(op));
                            try c.putString("query_params_ts_args", try self.buildQueryParamArgsTS(op));
                            try c.putString("query_params_py", try self.buildQueryParamStringPython(op));
                            try c.putString("query_params_go", try self.buildQueryParamStringGo(op));
                            try c.putString("query_params_rust", try self.buildQueryParamStringRust(op));
                            try c.putString("query_params_dart", try self.buildQueryParamStringDart(op));
                            try c.putString("query_params_scala", try self.buildQueryParamStringScala(op));
                            try c.putString("query_params_swift", try self.buildQueryParamStringSwift(op));
                            try c.putString("query_params_zig", try self.buildQueryParamStringZig(op));

                            // Params type name for bundled query param interfaces
                            var pascal_buf2: [256]u8 = undefined;
                            const pascal_name = toPascalCaseStatic(op_id, &pascal_buf2);
                            const params_type = try std.fmt.allocPrint(self.allocator, "{s}Params", .{pascal_name});
                            try c.putString("params_type_name", params_type);
                        }

                        // Rust fn_params
                        try c.putString("fn_params", try self.buildRustFnParams(has_path_params, has_body, has_query_params, op, path_str));

                        // Test call arguments for each language
                        try c.putString("test_args_ts", try self.buildTestArgsTS(path_str, has_body));
                        try c.putString("test_args_py", try self.buildTestArgsPy(path_str, has_body));
                        try c.putString("test_args_go", try self.buildTestArgsGo(path_str, has_body, op));
                        try c.putString("test_args_rust", try self.buildTestArgsRust(path_str, has_body, op));

                        // Mark first operation for error test generation
                        try c.putBool("is_first", idx == 0);

                        ops[idx] = c;
                        idx += 1;
                    } // else (dedup)
                    }
                }
            }
        }
        return @ptrCast(ops[0..idx]);
    }

    const PathParamInfo = struct {
        ts_params: []const u8,
        ts_args: []const u8,
        py_params: []const u8,
        go_params: []const u8,
        rust_params: []const u8,
        ts_interpolate: []const u8,
        py_interpolate: []const u8,
        go_interpolate: []const u8,
        rust_interpolate: []const u8,
        go_format_args: []const u8,
        rust_format_args: []const u8,
        // New languages
        java_params: []const u8,
        kotlin_params: []const u8,
        php_params: []const u8,
        csharp_params: []const u8,
        csharp_interpolate: []const u8,
        php_format_args: []const u8,
        // Dart, Scala, Swift
        dart_params: []const u8,
        dart_interpolate: []const u8,
        scala_params: []const u8,
        scala_interpolate: []const u8,
        swift_params: []const u8,
        swift_interpolate: []const u8,
        // Zig
        zig_params: []const u8,
        zig_interpolate: []const u8,
        zig_format_args: []const u8,
    };

    fn extractPathParamNames(self: *SDKGenerator, path: []const u8) !PathParamInfo {
        var ts_params = std.array_list.Managed(u8).init(self.allocator);
        var ts_args = std.array_list.Managed(u8).init(self.allocator);
        var py_params = std.array_list.Managed(u8).init(self.allocator);
        var go_params = std.array_list.Managed(u8).init(self.allocator);
        var rust_params = std.array_list.Managed(u8).init(self.allocator);
        var java_params = std.array_list.Managed(u8).init(self.allocator);
        var kotlin_params = std.array_list.Managed(u8).init(self.allocator);
        var php_params = std.array_list.Managed(u8).init(self.allocator);
        var csharp_params = std.array_list.Managed(u8).init(self.allocator);
        var dart_params = std.array_list.Managed(u8).init(self.allocator);
        var scala_params = std.array_list.Managed(u8).init(self.allocator);
        var swift_params = std.array_list.Managed(u8).init(self.allocator);
        var ts_interp = std.array_list.Managed(u8).init(self.allocator);
        var py_interp = std.array_list.Managed(u8).init(self.allocator);
        var go_interp = std.array_list.Managed(u8).init(self.allocator);
        var rust_interp = std.array_list.Managed(u8).init(self.allocator);
        var csharp_interp = std.array_list.Managed(u8).init(self.allocator);
        var dart_interp = std.array_list.Managed(u8).init(self.allocator);
        var scala_interp = std.array_list.Managed(u8).init(self.allocator);
        var swift_interp = std.array_list.Managed(u8).init(self.allocator);
        var zig_params = std.array_list.Managed(u8).init(self.allocator);
        var zig_interp = std.array_list.Managed(u8).init(self.allocator);
        var zig_fmt_args = std.array_list.Managed(u8).init(self.allocator);
        var go_fmt_args = std.array_list.Managed(u8).init(self.allocator);
        var rust_fmt_args = std.array_list.Managed(u8).init(self.allocator);
        var php_fmt_args = std.array_list.Managed(u8).init(self.allocator);

        // Build interpolated path and param lists
        var i: usize = 0;
        var param_count: usize = 0;
        while (i < path.len) {
            if (path[i] == '{') {
                const end = std.mem.indexOfScalarPos(u8, path, i + 1, '}') orelse break;
                const name = path[i + 1 .. end];

                if (param_count > 0) {
                    try ts_params.appendSlice(", ");
                    try ts_args.appendSlice(", ");
                    try py_params.appendSlice(", ");
                    try go_params.appendSlice(", ");
                    try rust_params.appendSlice(", ");
                    try java_params.appendSlice(", ");
                    try kotlin_params.appendSlice(", ");
                    try php_params.appendSlice(", ");
                    try csharp_params.appendSlice(", ");
                    try dart_params.appendSlice(", ");
                    try scala_params.appendSlice(", ");
                    try swift_params.appendSlice(", ");
                    try zig_params.appendSlice(", ");
                }
                // TS: name: string
                try ts_params.appendSlice(name);
                try ts_params.appendSlice(": string");
                // TS args (names only)
                try ts_args.appendSlice(name);
                // Python: name: str
                try py_params.appendSlice(name);
                try py_params.appendSlice(": str");
                // Go: name string
                try go_params.appendSlice(name);
                try go_params.appendSlice(" string");
                // Rust: name: &str
                try rust_params.appendSlice(name);
                try rust_params.appendSlice(": &str");
                // Java: String name
                try java_params.appendSlice("String ");
                try java_params.appendSlice(name);
                // Kotlin: name: String
                try kotlin_params.appendSlice(name);
                try kotlin_params.appendSlice(": String");
                // PHP: string $name
                try php_params.appendSlice("string $");
                try php_params.appendSlice(name);
                // C#: string name
                try csharp_params.appendSlice("string ");
                try csharp_params.appendSlice(name);
                // Dart: String name
                try dart_params.appendSlice("String ");
                try dart_params.appendSlice(name);
                // Scala: name: String
                try scala_params.appendSlice(name);
                try scala_params.appendSlice(": String");
                // Swift: name: String
                try swift_params.appendSlice(name);
                try swift_params.appendSlice(": String");
                // Zig: name: []const u8
                try zig_params.appendSlice(name);
                try zig_params.appendSlice(": []const u8");

                // Interpolation patterns
                try ts_interp.appendSlice("${");
                try ts_interp.appendSlice(name);
                try ts_interp.append('}');

                try py_interp.appendSlice("{");
                try py_interp.appendSlice(name);
                try py_interp.append('}');

                // Go: use fmt.Sprintf
                try go_interp.appendSlice("%s");

                // Rust: use format!
                try rust_interp.appendSlice("{}");

                // C#: use string interpolation {name}
                try csharp_interp.append('{');
                try csharp_interp.appendSlice(name);
                try csharp_interp.append('}');

                // Dart: use string interpolation $name
                try dart_interp.append('$');
                try dart_interp.appendSlice(name);

                // Scala: use string interpolation $name
                try scala_interp.append('$');
                try scala_interp.appendSlice(name);

                // Swift: use string interpolation \(name)
                try swift_interp.appendSlice("\\(");
                try swift_interp.appendSlice(name);
                try swift_interp.append(')');

                // Zig: use std.fmt.allocPrint with {s} placeholders
                try zig_interp.appendSlice("{s}");

                // Format arguments for Go, Rust, PHP, Zig
                if (param_count > 0) {
                    try go_fmt_args.appendSlice(", ");
                    try rust_fmt_args.appendSlice(", ");
                    try php_fmt_args.appendSlice(", ");
                    try zig_fmt_args.appendSlice(", ");
                }
                try go_fmt_args.appendSlice(name);
                try rust_fmt_args.appendSlice(name);
                try php_fmt_args.appendSlice("$");
                try php_fmt_args.appendSlice(name);
                try zig_fmt_args.appendSlice(name);

                param_count += 1;
                i = end + 1;
            } else {
                try ts_interp.append(path[i]);
                try py_interp.append(path[i]);
                try go_interp.append(path[i]);
                try rust_interp.append(path[i]);
                try csharp_interp.append(path[i]);
                try dart_interp.append(path[i]);
                try scala_interp.append(path[i]);
                try swift_interp.append(path[i]);
                try zig_interp.append(path[i]);
                i += 1;
            }
        }

        return .{
            .ts_params = try ts_params.toOwnedSlice(),
            .ts_args = try ts_args.toOwnedSlice(),
            .py_params = try py_params.toOwnedSlice(),
            .go_params = try go_params.toOwnedSlice(),
            .rust_params = try rust_params.toOwnedSlice(),
            .ts_interpolate = try ts_interp.toOwnedSlice(),
            .py_interpolate = try py_interp.toOwnedSlice(),
            .go_interpolate = try go_interp.toOwnedSlice(),
            .rust_interpolate = try rust_interp.toOwnedSlice(),
            .go_format_args = try go_fmt_args.toOwnedSlice(),
            .rust_format_args = try rust_fmt_args.toOwnedSlice(),
            .java_params = try java_params.toOwnedSlice(),
            .kotlin_params = try kotlin_params.toOwnedSlice(),
            .php_params = try php_params.toOwnedSlice(),
            .csharp_params = try csharp_params.toOwnedSlice(),
            .csharp_interpolate = try csharp_interp.toOwnedSlice(),
            .php_format_args = try php_fmt_args.toOwnedSlice(),
            .dart_params = try dart_params.toOwnedSlice(),
            .dart_interpolate = try dart_interp.toOwnedSlice(),
            .scala_params = try scala_params.toOwnedSlice(),
            .scala_interpolate = try scala_interp.toOwnedSlice(),
            .swift_params = try swift_params.toOwnedSlice(),
            .swift_interpolate = try swift_interp.toOwnedSlice(),
            .zig_params = try zig_params.toOwnedSlice(),
            .zig_interpolate = try zig_interp.toOwnedSlice(),
            .zig_format_args = try zig_fmt_args.toOwnedSlice(),
        };
    }

    fn buildRustFnParams(self: *SDKGenerator, has_path_params: bool, has_body: bool, has_query_params: bool, op: parser.Operation, path_str: []const u8) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        try buf.appendSlice("&self");
        if (has_path_params) {
            // First try declared parameters
            var found_path_param = false;
            for (op.parameters.items) |param| {
                if (param.in == .path) {
                    try buf.appendSlice(", ");
                    try buf.appendSlice(param.name);
                    try buf.appendSlice(": &str");
                    found_path_param = true;
                }
            }
            // Fallback: infer path params from URL {placeholders} when not declared
            if (!found_path_param) {
                var i: usize = 0;
                while (i < path_str.len) {
                    if (path_str[i] == '{') {
                        const end = std.mem.indexOfScalarPos(u8, path_str, i + 1, '}') orelse break;
                        const name = path_str[i + 1 .. end];
                        try buf.appendSlice(", ");
                        try buf.appendSlice(name);
                        try buf.appendSlice(": &str");
                        i = end + 1;
                    } else {
                        i += 1;
                    }
                }
            }
        }
        if (has_body) {
            if (op.requestBody) |rb| {
                if (rb.schema_ref) |ref| {
                    try buf.appendSlice(", body: &");
                    try buf.appendSlice(ref);
                } else {
                    try buf.appendSlice(", body: &impl serde::Serialize");
                }
            } else {
                try buf.appendSlice(", body: &impl serde::Serialize");
            }
        }
        if (has_query_params) {
            const op_id = op.operationId orelse "";
            var pascal_buf2: [256]u8 = undefined;
            const pascal_name = toPascalCaseStatic(op_id, &pascal_buf2);
            const params_type = try std.fmt.allocPrint(self.allocator, ", params: Option<&{s}Params>", .{pascal_name});
            try buf.appendSlice(params_type);
        }
        try buf.appendSlice(", options: Option<&RequestOptions>");
        return try buf.toOwnedSlice();
    }

    // ── Test argument builders ────────────────────────────────────────

    fn countPathParams(_: *SDKGenerator, path: []const u8) usize {
        var count: usize = 0;
        for (path) |ch| {
            if (ch == '{') count += 1;
        }
        return count;
    }

    fn buildTestArgsTS(self: *SDKGenerator, path: []const u8, has_body: bool) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        const n = self.countPathParams(path);
        for (0..n) |i| {
            if (i > 0) try buf.appendSlice(", ");
            try buf.appendSlice("\"test-id\"");
        }
        if (has_body) {
            if (n > 0) try buf.appendSlice(", ");
            try buf.appendSlice("{} as any");
        }
        return try buf.toOwnedSlice();
    }

    fn buildTestArgsPy(self: *SDKGenerator, path: []const u8, has_body: bool) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        const n = self.countPathParams(path);
        for (0..n) |i| {
            if (i > 0) try buf.appendSlice(", ");
            try buf.appendSlice("\"test-id\"");
        }
        if (has_body) {
            if (n > 0) try buf.appendSlice(", ");
            try buf.appendSlice("{}");
        }
        return try buf.toOwnedSlice();
    }

    fn buildTestArgsGo(self: *SDKGenerator, path: []const u8, has_body: bool, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var has_prev = false;
        const n = self.countPathParams(path);
        for (0..n) |i| {
            if (i > 0) try buf.appendSlice(", ");
            try buf.appendSlice("\"test-id\"");
            has_prev = true;
        }
        if (has_body) {
            if (has_prev) try buf.appendSlice(", ");
            try buf.appendSlice("nil");
            has_prev = true;
        }
        // Single nil for optional *Params pointer
        var has_qp = false;
        for (op.parameters.items) |param| {
            if (param.in == .query) { has_qp = true; break; }
        }
        if (has_qp) {
            if (has_prev) try buf.appendSlice(", ");
            try buf.appendSlice("nil");
        }
        return try buf.toOwnedSlice();
    }

    fn buildTestArgsRust(self: *SDKGenerator, path: []const u8, has_body: bool, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var has_prev = false;
        const n = self.countPathParams(path);
        for (0..n) |i| {
            if (i > 0) try buf.appendSlice(", ");
            try buf.appendSlice("\"test-id\"");
            has_prev = true;
        }
        if (has_body) {
            if (has_prev) try buf.appendSlice(", ");
            try buf.appendSlice("&serde_json::json!({})");
            has_prev = true;
        }
        // Single None for Option<&Params>
        var has_qp = false;
        for (op.parameters.items) |param| {
            if (param.in == .query) { has_qp = true; break; }
        }
        if (has_qp) {
            if (has_prev) try buf.appendSlice(", ");
            try buf.appendSlice("None");
            has_prev = true;
        }
        // None for Option<&RequestOptions>
        if (has_prev) try buf.appendSlice(", ");
        try buf.appendSlice("None");
        return try buf.toOwnedSlice();
    }

    // ── Query param type helpers ─────────────────────────────────────────

    fn scalaSafeName(self: *SDKGenerator, name: []const u8) ![]const u8 {
        const scala_keywords = [_][]const u8{
            "abstract", "case", "catch", "class", "def", "do", "else",
            "extends", "false", "final", "finally", "for", "forSome", "if",
            "implicit", "import", "lazy", "match", "new", "null", "object",
            "override", "package", "private", "protected", "return", "sealed",
            "super", "this", "throw", "trait", "true", "try", "type", "val",
            "var", "while", "with", "yield",
        };
        for (scala_keywords) |kw| {
            if (std.mem.eql(u8, name, kw)) {
                return try std.fmt.allocPrint(self.allocator, "`{s}`", .{name});
            }
        }
        return name;
    }

    fn rustSafeName(self: *SDKGenerator, name: []const u8) ![]const u8 {
        const rust_keywords = [_][]const u8{
            "as", "break", "const", "continue", "crate", "else", "enum",
            "extern", "false", "fn", "for", "if", "impl", "in", "let",
            "loop", "match", "mod", "move", "mut", "pub", "ref", "return",
            "self", "Self", "static", "struct", "super", "trait", "true",
            "type", "unsafe", "use", "where", "while", "async", "await",
            "dyn", "abstract", "become", "box", "do", "final", "macro",
            "override", "priv", "typeof", "unsized", "virtual", "yield",
        };
        for (rust_keywords) |kw| {
            if (std.mem.eql(u8, name, kw)) {
                return try std.fmt.allocPrint(self.allocator, "r#{s}", .{name});
            }
        }
        return name;
    }

    fn queryParamTypeTS(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "boolean";
        if (std.mem.eql(u8, schema_type, "integer")) return "number";
        if (std.mem.eql(u8, schema_type, "number")) return "number";
        return "string";
    }

    fn queryParamTypePython(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "bool";
        if (std.mem.eql(u8, schema_type, "integer")) return "int";
        if (std.mem.eql(u8, schema_type, "number")) return "float";
        return "str";
    }

    fn queryParamTypeGo(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "*bool";
        if (std.mem.eql(u8, schema_type, "integer")) return "*int64";
        if (std.mem.eql(u8, schema_type, "number")) return "*float64";
        return "*string";
    }

    fn queryParamTypeRust(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "bool";
        if (std.mem.eql(u8, schema_type, "integer")) return "i64";
        if (std.mem.eql(u8, schema_type, "number")) return "f64";
        return "String";
    }

    fn queryParamGoFormat(_: *SDKGenerator, name: []const u8, schema_type: []const u8) []const u8 {
        // Returns the Go format expression e.g. fmt.Sprintf("%v", *name)
        _ = name;
        _ = schema_type;
        return "%v";
    }

    fn buildQueryParamStringTS(self: *SDKGenerator, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var first = true;
        for (op.parameters.items) |param| {
            if (param.in == .query) {
                if (!first) try buf.appendSlice(", ");
                first = false;
                try buf.appendSlice(param.name);
                try buf.appendSlice("?: ");
                try buf.appendSlice(self.queryParamTypeTS(param.schema_type orelse "string"));
            }
        }
        return try buf.toOwnedSlice();
    }

    fn buildQueryParamArgsTS(self: *SDKGenerator, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var first = true;
        for (op.parameters.items) |param| {
            if (param.in == .query) {
                if (!first) try buf.appendSlice(", ");
                first = false;
                try buf.appendSlice(param.name);
            }
        }
        return try buf.toOwnedSlice();
    }

    fn buildQueryParamStringPython(self: *SDKGenerator, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var first = true;
        for (op.parameters.items) |param| {
            if (param.in == .query) {
                if (!first) try buf.appendSlice(", ");
                first = false;
                try buf.appendSlice(param.name);
                try buf.appendSlice(": ");
                try buf.appendSlice(self.queryParamTypePython(param.schema_type orelse "string"));
                try buf.appendSlice(" | None = None");
            }
        }
        return try buf.toOwnedSlice();
    }

    fn buildQueryParamStringGo(self: *SDKGenerator, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var first = true;
        for (op.parameters.items) |param| {
            if (param.in == .query) {
                if (!first) try buf.appendSlice(", ");
                first = false;
                try buf.appendSlice(param.name);
                try buf.appendSlice(" ");
                try buf.appendSlice(self.queryParamTypeGo(param.schema_type orelse "string"));
            }
        }
        return try buf.toOwnedSlice();
    }

    fn buildQueryParamStringRust(self: *SDKGenerator, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var first = true;
        for (op.parameters.items) |param| {
            if (param.in == .query) {
                if (!first) try buf.appendSlice(", ");
                first = false;
                try buf.appendSlice(param.name);
                try buf.appendSlice(": Option<");
                try buf.appendSlice(self.queryParamTypeRust(param.schema_type orelse "string"));
                try buf.appendSlice(">");
            }
        }
        return try buf.toOwnedSlice();
    }

    fn buildQueryParamStringDart(self: *SDKGenerator, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var first = true;
        for (op.parameters.items) |param| {
            if (param.in == .query) {
                if (!first) try buf.appendSlice(", ");
                first = false;
                try buf.appendSlice(self.queryParamTypeDart(param.schema_type orelse "string"));
                try buf.appendSlice(" ");
                try buf.appendSlice(param.name);
            }
        }
        return try buf.toOwnedSlice();
    }

    fn buildQueryParamStringScala(self: *SDKGenerator, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var first = true;
        for (op.parameters.items) |param| {
            if (param.in == .query) {
                if (!first) try buf.appendSlice(", ");
                first = false;
                try buf.appendSlice(param.name);
                try buf.appendSlice(": ");
                try buf.appendSlice(self.queryParamTypeScala(param.schema_type orelse "string"));
                try buf.appendSlice(" = None");
            }
        }
        return try buf.toOwnedSlice();
    }

    fn buildQueryParamStringSwift(self: *SDKGenerator, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var first = true;
        for (op.parameters.items) |param| {
            if (param.in == .query) {
                if (!first) try buf.appendSlice(", ");
                first = false;
                try buf.appendSlice(param.name);
                try buf.appendSlice(": ");
                try buf.appendSlice(self.queryParamTypeSwift(param.schema_type orelse "string"));
                try buf.appendSlice(" = nil");
            }
        }
        return try buf.toOwnedSlice();
    }

    // ── Model context building from components/schemas ──────────────────

    fn buildModelContexts(self: *SDKGenerator, base_ctx: *template.Context) ![]const *template.Context {
        const comps = self.spec.components orelse return &.{};
        var schema_iter = comps.schemas.iterator();

        var count: usize = 0;
        var count_iter = comps.schemas.iterator();
        while (count_iter.next()) |_| count += 1;

        if (count == 0) return &.{};

        const models = try self.allocator.alloc(*template.Context, count);
        var idx: usize = 0;

        schema_iter = comps.schemas.iterator();
        while (schema_iter.next()) |entry| {
            const name = entry.key_ptr.*;
            const schema = entry.value_ptr.*;

            const m = try self.allocator.create(template.Context);
            m.* = template.Context.init(self.allocator);
            m.parent = base_ctx;

            try m.putString("name", name);
            var pascal_buf: [256]u8 = undefined;
            try m.putString("pascal_name", try self.allocator.dupe(u8, toPascalCaseStatic(name, &pascal_buf)));
            var snake_buf: [256]u8 = undefined;
            try m.putString("snake_name", try self.allocator.dupe(u8, toSnakeCaseStatic(name, &snake_buf)));

            // Is it an enum?
            const is_enum = schema.enum_values.items.len > 0;
            try m.putBool("is_enum", is_enum);
            // Is it a union (oneOf)?
            const is_union = schema.one_of_variants.items.len > 0;
            try m.putBool("is_union", is_union);
            try m.putString("type_name", schema.type_name orelse "object");
            try m.putString("description", self.sanitiseOneLine(schema.description orelse ""));

            // Enum values
            if (is_enum) {
                const ev_len = schema.enum_values.items.len;
                var enum_ctxs = try self.allocator.alloc(*template.Context, ev_len);
                for (schema.enum_values.items, 0..) |ev, ei| {
                    const ec = try self.allocator.create(template.Context);
                    ec.* = template.Context.init(self.allocator);
                    ec.parent = m;
                    try ec.putString("value", ev);
                    var ev_pascal_buf: [256]u8 = undefined;
                    try ec.putString("pascal_value", try self.allocator.dupe(u8, toPascalCaseStatic(ev, &ev_pascal_buf)));
                    var ev_upper_buf: [256]u8 = undefined;
                    try ec.putString("upper_value", try self.allocator.dupe(u8, toUpperStatic(ev, &ev_upper_buf)));
                    try ec.putBool("is_last", ei == ev_len - 1);
                    enum_ctxs[ei] = ec;
                }
                try m.putList("enum_values", @ptrCast(enum_ctxs));
            }

            // oneOf variants (for union types)
            if (is_union) {
                const variant_count = schema.one_of_variants.items.len;
                var variant_ctxs = try self.allocator.alloc(*template.Context, variant_count);
                for (schema.one_of_variants.items, 0..) |variant, vi| {
                    const vc = try self.allocator.create(template.Context);
                    vc.* = template.Context.init(self.allocator);
                    vc.parent = m;

                    // Derive variant name from first required field (or first property)
                    const variant_key = if (variant.required_fields.items.len > 0)
                        variant.required_fields.items[0]
                    else if (variant.properties.items.len > 0)
                        variant.properties.items[0].name
                    else
                        "Unknown";

                    try vc.putString("variant_key", variant_key);
                    var vk_pascal_buf: [256]u8 = undefined;
                    try vc.putString("variant_pascal", try self.allocator.dupe(u8, toPascalCaseStatic(variant_key, &vk_pascal_buf)));
                    try vc.putBool("is_last", vi == variant_count - 1);

                    // Build property contexts for this variant
                    var vprop_ctxs = try self.allocator.alloc(*template.Context, variant.properties.items.len);
                    for (variant.properties.items, 0..) |vprop, vpi| {
                        const vpc = try self.allocator.create(template.Context);
                        vpc.* = template.Context.init(self.allocator);
                        vpc.parent = vc;
                        try vpc.putString("name", vprop.name);
                        try vpc.putString("scala_name", try self.scalaSafeName(vprop.name));
                        var vp_pascal_buf: [256]u8 = undefined;
                        try vpc.putString("pascal_name", try self.allocator.dupe(u8, toPascalCaseStatic(vprop.name, &vp_pascal_buf)));
                        try vpc.putBool("required", vprop.required);
                        try vpc.putString("description", self.sanitiseOneLine(vprop.description orelse ""));
                        try vpc.putString("ts_type", self.resolveTypeTS(vprop));
                        try vpc.putString("rust_type", self.resolveTypeRust(vprop));
                        try vpc.putString("py_type", self.resolveTypePython(vprop));
                        try vpc.putString("go_type", self.resolveTypeGo(vprop));
                        try vpc.putString("java_type", self.resolveTypeJava(vprop));
                        try vpc.putString("kotlin_type", self.resolveTypeKotlin(vprop));
                        try vpc.putString("ruby_type", self.resolveTypeRuby(vprop));
                        try vpc.putString("php_type", self.resolveTypePhp(vprop));
                        try vpc.putString("csharp_type", self.resolveTypeCsharp(vprop));
                        try vpc.putString("dart_type", self.resolveTypeDart(vprop));
                        try vpc.putString("scala_type", self.resolveTypeScala(vprop));
                        try vpc.putString("swift_type", self.resolveTypeSwift(vprop));
                        // Rust-safe field name (escape keywords like type, ref, match, etc.)
                        const rust_safe = try self.rustSafeName(vprop.name);
                        try vpc.putString("rust_name", rust_safe);
                        try vpc.putBool("is_renamed", !std.mem.eql(u8, rust_safe, vprop.name));
                        try vpc.putBool("is_last", vpi == variant.properties.items.len - 1);
                        vprop_ctxs[vpi] = vpc;
                    }
                    try vc.putList("properties", @ptrCast(vprop_ctxs));

                    variant_ctxs[vi] = vc;
                }
                try m.putList("variants", @ptrCast(variant_ctxs));

                // Build deduplicated union_properties (for languages that flatten variants into one class)
                var seen_props = std.StringHashMap(*template.Context).init(self.allocator);
                defer seen_props.deinit();
                var dedup_list: std.ArrayList(*template.Context) = .{};
                for (schema.one_of_variants.items) |variant| {
                    for (variant.properties.items) |vprop| {
                        if (!seen_props.contains(vprop.name)) {
                            const vpc = try self.allocator.create(template.Context);
                            vpc.* = template.Context.init(self.allocator);
                            vpc.parent = m;
                            try vpc.putString("name", vprop.name);
                            try vpc.putString("scala_name", try self.scalaSafeName(vprop.name));
                            var up_pascal_buf: [256]u8 = undefined;
                            try vpc.putString("pascal_name", try self.allocator.dupe(u8, toPascalCaseStatic(vprop.name, &up_pascal_buf)));
                            try vpc.putBool("required", false); // union fields are always optional
                            try vpc.putString("description", self.sanitiseOneLine(vprop.description orelse ""));
                            try vpc.putString("ts_type", self.resolveTypeTS(vprop));
                            try vpc.putString("rust_type", self.resolveTypeRust(vprop));
                            try vpc.putString("py_type", self.resolveTypePython(vprop));
                            try vpc.putString("go_type", self.resolveTypeGo(vprop));
                            try vpc.putString("java_type", self.resolveTypeJava(vprop));
                            try vpc.putString("kotlin_type", self.resolveTypeKotlin(vprop));
                            try vpc.putString("ruby_type", self.resolveTypeRuby(vprop));
                            try vpc.putString("php_type", self.resolveTypePhp(vprop));
                            try vpc.putString("csharp_type", self.resolveTypeCsharp(vprop));
                            try vpc.putString("dart_type", self.resolveTypeDart(vprop));
                            try vpc.putString("scala_type", self.resolveTypeScala(vprop));
                            try vpc.putString("swift_type", self.resolveTypeSwift(vprop));
                            // Rust-safe field name (escape keywords like type, ref, match, etc.)
                            const dedup_rust_safe = try self.rustSafeName(vprop.name);
                            try vpc.putString("rust_name", dedup_rust_safe);
                            try vpc.putBool("is_renamed", !std.mem.eql(u8, dedup_rust_safe, vprop.name));
                            try seen_props.put(vprop.name, vpc);
                            try dedup_list.append(self.allocator, vpc);
                        }
                    }
                }
                const union_props = try dedup_list.toOwnedSlice(self.allocator);
                // Set is_last on the final union property
                for (union_props, 0..) |up, upi| {
                    const upc: *template.Context = @ptrCast(@alignCast(up));
                    try upc.putBool("is_last", upi == union_props.len - 1);
                }
                try m.putList("union_properties", @ptrCast(union_props));
            }

            // Properties (for struct types)
            if (schema.properties.items.len > 0) {
                var prop_ctxs = try self.allocator.alloc(*template.Context, schema.properties.items.len);
                var nested_type_list = std.array_list.Managed(*template.Context).init(self.allocator);
                for (schema.properties.items, 0..) |prop, pi| {
                    const pc = try self.allocator.create(template.Context);
                    pc.* = template.Context.init(self.allocator);
                    pc.parent = m;
                    try pc.putString("name", prop.name);
                    // Rust-safe name (prefix with r# if keyword)
                    try pc.putString("rust_name", try self.rustSafeName(prop.name));
                    // Scala-safe name (backtick reserved words)
                    try pc.putString("scala_name", try self.scalaSafeName(prop.name));
                    var prop_pascal_buf: [256]u8 = undefined;
                    const prop_pascal = try self.allocator.dupe(u8, toPascalCaseStatic(prop.name, &prop_pascal_buf));
                    try pc.putString("pascal_name", prop_pascal);
                    try pc.putBool("required", prop.required);
                    try pc.putString("description", self.sanitiseOneLine(prop.description orelse ""));

                    // Check for nested inline object
                    if (prop.is_nested_object) {
                        // Override types to reference the nested type
                        try pc.putString("ts_type", try std.fmt.allocPrint(self.allocator, "{s}.{s}", .{ name, prop_pascal }));
                        try pc.putString("rust_type", try std.fmt.allocPrint(self.allocator, "{s}::{s}", .{ toSnakeCaseStatic(name, &snake_buf), prop_pascal }));
                        try pc.putString("py_type", try std.fmt.allocPrint(self.allocator, "{s}.{s}", .{ name, prop_pascal }));
                        try pc.putString("go_type", try std.fmt.allocPrint(self.allocator, "{s}{s}", .{ name, prop_pascal }));
                        try pc.putBool("has_ref", false);

                        // Build nested type context
                        const ntc = try self.allocator.create(template.Context);
                        ntc.* = template.Context.init(self.allocator);
                        ntc.parent = m;
                        try ntc.putString("nested_name", prop_pascal);
                        var nested_snake_buf: [256]u8 = undefined;
                        try ntc.putString("nested_snake_name", try self.allocator.dupe(u8, toSnakeCaseStatic(prop.name, &nested_snake_buf)));
                        try ntc.putString("description", self.sanitiseOneLine(prop.description orelse ""));
                        // Go: prefixed name
                        try ntc.putString("go_prefixed_name", try std.fmt.allocPrint(self.allocator, "{s}{s}", .{ name, prop_pascal }));

                        // Build fields for the nested type
                        var nested_field_ctxs = try self.allocator.alloc(*template.Context, prop.nested_properties.len);
                        for (prop.nested_properties, 0..) |nprop, ni| {
                            const nfc = try self.allocator.create(template.Context);
                            nfc.* = template.Context.init(self.allocator);
                            nfc.parent = ntc;
                            try nfc.putString("name", nprop.name);
                            try nfc.putString("rust_name", try self.rustSafeName(nprop.name));
                            var nf_pascal_buf: [256]u8 = undefined;
                            try nfc.putString("pascal_name", try self.allocator.dupe(u8, toPascalCaseStatic(nprop.name, &nf_pascal_buf)));
                            try nfc.putBool("required", nprop.required);
                            try nfc.putString("description", self.sanitiseOneLine(nprop.description orelse ""));
                            try nfc.putString("ts_type", self.resolveTypeTS(nprop));
                            try nfc.putString("rust_type", self.resolveTypeRust(nprop));
                            try nfc.putString("py_type", self.resolveTypePython(nprop));
                            try nfc.putString("go_type", self.resolveTypeGo(nprop));
                            try nfc.putString("java_type", self.resolveTypeJava(nprop));
                            try nfc.putString("kotlin_type", self.resolveTypeKotlin(nprop));
                            try nfc.putString("ruby_type", self.resolveTypeRuby(nprop));
                            try nfc.putString("php_type", self.resolveTypePhp(nprop));
                            try nfc.putString("csharp_type", self.resolveTypeCsharp(nprop));
                            try nfc.putString("dart_type", self.resolveTypeDart(nprop));
                            try nfc.putString("scala_type", self.resolveTypeScala(nprop));
                            try nfc.putString("swift_type", self.resolveTypeSwift(nprop));
                            nested_field_ctxs[ni] = nfc;
                        }
                        try ntc.putList("fields", @ptrCast(nested_field_ctxs));
                        try nested_type_list.append(ntc);
                    } else {
                        // Resolve type for each language
                        try pc.putString("ts_type", self.resolveTypeTS(prop));
                        try pc.putString("rust_type", self.resolveTypeRust(prop));
                        try pc.putString("py_type", self.resolveTypePython(prop));
                        try pc.putString("go_type", self.resolveTypeGo(prop));
                        try pc.putString("java_type", self.resolveTypeJava(prop));
                        try pc.putString("kotlin_type", self.resolveTypeKotlin(prop));
                        try pc.putString("ruby_type", self.resolveTypeRuby(prop));
                        try pc.putString("php_type", self.resolveTypePhp(prop));
                        try pc.putString("csharp_type", self.resolveTypeCsharp(prop));
                        try pc.putString("dart_type", self.resolveTypeDart(prop));
                        try pc.putString("scala_type", self.resolveTypeScala(prop));
                        try pc.putString("swift_type", self.resolveTypeSwift(prop));

                        // Has $ref?
                        if (prop.ref) |ref| {
                            try pc.putString("ref", ref);
                            try pc.putBool("has_ref", true);
                        } else {
                            try pc.putBool("has_ref", false);
                        }
                    }

                    prop_ctxs[pi] = pc;
                }
                try m.putList("properties", @ptrCast(prop_ctxs));
                try m.putBool("has_properties", true);

                // Add nested types if any
                const nested_slice = try nested_type_list.toOwnedSlice();
                try m.putList("nested_types", @ptrCast(nested_slice));
                try m.putBool("has_nested_types", nested_slice.len > 0);
            } else {
                try m.putBool("has_properties", false);
                try m.putBool("has_nested_types", false);
            }

            models[idx] = m;
            idx += 1;
        }
        return @ptrCast(models[0..idx]);
    }

    // ── Type resolution per language ────────────────────────────────────

    fn resolveTypeTS(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "unknown";
        if (std.mem.eql(u8, t, "string")) return "string";
        if (std.mem.eql(u8, t, "integer")) return "number";
        if (std.mem.eql(u8, t, "number")) return "number";
        if (std.mem.eql(u8, t, "boolean")) return "boolean";
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "{s}[]", .{ir}) catch return "unknown[]";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "string[]";
                if (std.mem.eql(u8, it, "integer") or std.mem.eql(u8, it, "number")) return "number[]";
                if (std.mem.eql(u8, it, "boolean")) return "boolean[]";
                return "unknown[]";
            }
            return "unknown[]";
        }
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "Record<string, string>";
                if (std.mem.eql(u8, vt, "integer") or std.mem.eql(u8, vt, "number")) return "Record<string, number>";
                if (std.mem.eql(u8, vt, "boolean")) return "Record<string, boolean>";
            }
            return "Record<string, unknown>";
        }
        return "unknown";
    }

    fn resolveTypeRust(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "serde_json::Value";
        if (std.mem.eql(u8, t, "string")) {
            if (prop.format) |f| {
                if (std.mem.eql(u8, f, "uuid")) return "String";
            }
            return "String";
        }
        if (std.mem.eql(u8, t, "integer")) {
            if (prop.format) |f| {
                if (std.mem.eql(u8, f, "int32")) return "i32";
                if (std.mem.eql(u8, f, "int64")) return "i64";
            }
            return "i64";
        }
        if (std.mem.eql(u8, t, "number")) return "f64";
        if (std.mem.eql(u8, t, "boolean")) return "bool";
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "Vec<{s}>", .{ir}) catch return "Vec<serde_json::Value>";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "Vec<String>";
                if (std.mem.eql(u8, it, "integer")) return "Vec<i64>";
                if (std.mem.eql(u8, it, "number")) return "Vec<f64>";
                if (std.mem.eql(u8, it, "boolean")) return "Vec<bool>";
            }
            return "Vec<serde_json::Value>";
        }
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "std::collections::HashMap<String, String>";
                if (std.mem.eql(u8, vt, "integer")) return "std::collections::HashMap<String, i64>";
                if (std.mem.eql(u8, vt, "number")) return "std::collections::HashMap<String, f64>";
                if (std.mem.eql(u8, vt, "boolean")) return "std::collections::HashMap<String, bool>";
            }
            return "serde_json::Value";
        }
        return "serde_json::Value";
    }

    fn resolveTypePython(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "Any";
        if (std.mem.eql(u8, t, "string")) return "str";
        if (std.mem.eql(u8, t, "integer")) return "int";
        if (std.mem.eql(u8, t, "number")) return "float";
        if (std.mem.eql(u8, t, "boolean")) return "bool";
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "dict[str, str]";
                if (std.mem.eql(u8, vt, "integer")) return "dict[str, int]";
                if (std.mem.eql(u8, vt, "number")) return "dict[str, float]";
                if (std.mem.eql(u8, vt, "boolean")) return "dict[str, bool]";
            }
            return "dict";
        }
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "list[{s}]", .{ir}) catch return "list";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "list[str]";
                if (std.mem.eql(u8, it, "integer")) return "list[int]";
                if (std.mem.eql(u8, it, "number")) return "list[float]";
                if (std.mem.eql(u8, it, "boolean")) return "list[bool]";
            }
            return "list";
        }
        return "Any";
    }

    fn resolveTypeGo(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "interface{}";
        if (std.mem.eql(u8, t, "string")) return "string";
        if (std.mem.eql(u8, t, "integer")) {
            if (prop.format) |f| {
                if (std.mem.eql(u8, f, "int32")) return "int32";
            }
            return "int64";
        }
        if (std.mem.eql(u8, t, "number")) return "float64";
        if (std.mem.eql(u8, t, "boolean")) return "bool";
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "[]{s}", .{ir}) catch return "[]interface{}";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "[]string";
                if (std.mem.eql(u8, it, "integer")) return "[]int64";
                if (std.mem.eql(u8, it, "number")) return "[]float64";
                if (std.mem.eql(u8, it, "boolean")) return "[]bool";
            }
            return "[]interface{}";
        }
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "map[string]string";
                if (std.mem.eql(u8, vt, "integer")) return "map[string]int64";
                if (std.mem.eql(u8, vt, "number")) return "map[string]float64";
                if (std.mem.eql(u8, vt, "boolean")) return "map[string]bool";
            }
            return "map[string]interface{}";
        }
        return "interface{}";
    }

    // ── Type resolvers for new languages ────────────────────────────────

    fn resolveTypeJava(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "Object";
        if (std.mem.eql(u8, t, "string")) return "String";
        if (std.mem.eql(u8, t, "integer")) {
            if (prop.format) |f| {
                if (std.mem.eql(u8, f, "int32")) return "Integer";
            }
            return "Long";
        }
        if (std.mem.eql(u8, t, "number")) return "Double";
        if (std.mem.eql(u8, t, "boolean")) return "Boolean";
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "List<{s}>", .{ir}) catch return "List<Object>";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "List<String>";
                if (std.mem.eql(u8, it, "integer")) return "List<Long>";
                if (std.mem.eql(u8, it, "number")) return "List<Double>";
                if (std.mem.eql(u8, it, "boolean")) return "List<Boolean>";
            }
            return "List<Object>";
        }
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "Map<String, String>";
                if (std.mem.eql(u8, vt, "integer")) return "Map<String, Long>";
                if (std.mem.eql(u8, vt, "number")) return "Map<String, Double>";
                if (std.mem.eql(u8, vt, "boolean")) return "Map<String, Boolean>";
            }
            return "Map<String, Object>";
        }
        return "Object";
    }

    fn resolveTypeKotlin(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "kotlinx.serialization.json.JsonElement";
        if (std.mem.eql(u8, t, "string")) return "String";
        if (std.mem.eql(u8, t, "integer")) {
            if (prop.format) |f| {
                if (std.mem.eql(u8, f, "int32")) return "Int";
            }
            return "Long";
        }
        if (std.mem.eql(u8, t, "number")) return "Double";
        if (std.mem.eql(u8, t, "boolean")) return "Boolean";
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "List<{s}>", .{ir}) catch return "List<kotlinx.serialization.json.JsonElement>";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "List<String>";
                if (std.mem.eql(u8, it, "integer")) return "List<Long>";
                if (std.mem.eql(u8, it, "number")) return "List<Double>";
                if (std.mem.eql(u8, it, "boolean")) return "List<Boolean>";
            }
            return "List<kotlinx.serialization.json.JsonElement>";
        }
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "Map<String, String>";
                if (std.mem.eql(u8, vt, "integer")) return "Map<String, Long>";
                if (std.mem.eql(u8, vt, "number")) return "Map<String, Double>";
                if (std.mem.eql(u8, vt, "boolean")) return "Map<String, Boolean>";
            }
            return "Map<String, kotlinx.serialization.json.JsonElement>";
        }
        return "kotlinx.serialization.json.JsonElement";
    }

    fn resolveTypeRuby(_: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "Object";
        if (std.mem.eql(u8, t, "string")) return "String";
        if (std.mem.eql(u8, t, "integer")) return "Integer";
        if (std.mem.eql(u8, t, "number")) return "Float";
        if (std.mem.eql(u8, t, "boolean")) return "Boolean";
        if (std.mem.eql(u8, t, "array")) return "Array";
        if (std.mem.eql(u8, t, "object")) return "Hash";
        return "Object";
    }

    fn resolveTypePhp(_: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "object";
        if (std.mem.eql(u8, t, "string")) return "string";
        if (std.mem.eql(u8, t, "integer")) return "int";
        if (std.mem.eql(u8, t, "number")) return "float";
        if (std.mem.eql(u8, t, "boolean")) return "bool";
        if (std.mem.eql(u8, t, "array")) return "array";
        if (std.mem.eql(u8, t, "object")) return "array";
        return "string";
    }

    fn resolveTypeCsharp(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "object";
        if (std.mem.eql(u8, t, "string")) return "string";
        if (std.mem.eql(u8, t, "integer")) {
            if (prop.format) |f| {
                if (std.mem.eql(u8, f, "int32")) return "int";
            }
            return "long";
        }
        if (std.mem.eql(u8, t, "number")) return "double";
        if (std.mem.eql(u8, t, "boolean")) return "bool";
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "List<{s}>", .{ir}) catch return "List<object>";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "List<string>";
                if (std.mem.eql(u8, it, "integer")) return "List<long>";
                if (std.mem.eql(u8, it, "number")) return "List<double>";
                if (std.mem.eql(u8, it, "boolean")) return "List<bool>";
            }
            return "List<object>";
        }
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "Dictionary<string, string>";
                if (std.mem.eql(u8, vt, "integer")) return "Dictionary<string, long>";
                if (std.mem.eql(u8, vt, "number")) return "Dictionary<string, double>";
                if (std.mem.eql(u8, vt, "boolean")) return "Dictionary<string, bool>";
            }
            return "Dictionary<string, object>";
        }
        return "object";
    }

    // ── Type resolvers for Dart, Scala, Swift ─────────────────────────

    fn resolveTypeDart(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "Object";
        if (std.mem.eql(u8, t, "string")) return "String";
        if (std.mem.eql(u8, t, "integer")) return "int";
        if (std.mem.eql(u8, t, "number")) return "double";
        if (std.mem.eql(u8, t, "boolean")) return "bool";
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "List<{s}>", .{ir}) catch return "List<dynamic>";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "List<String>";
                if (std.mem.eql(u8, it, "integer")) return "List<int>";
                if (std.mem.eql(u8, it, "number")) return "List<double>";
                if (std.mem.eql(u8, it, "boolean")) return "List<bool>";
            }
            return "List<dynamic>";
        }
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "Map<String, String>";
                if (std.mem.eql(u8, vt, "integer")) return "Map<String, int>";
                if (std.mem.eql(u8, vt, "number")) return "Map<String, double>";
                if (std.mem.eql(u8, vt, "boolean")) return "Map<String, bool>";
            }
            return "Map<String, dynamic>";
        }
        return "dynamic";
    }

    fn resolveTypeScala(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "ujson.Value";
        if (std.mem.eql(u8, t, "string")) return "String";
        if (std.mem.eql(u8, t, "integer")) {
            if (prop.format) |f| {
                if (std.mem.eql(u8, f, "int32")) return "Int";
            }
            return "Long";
        }
        if (std.mem.eql(u8, t, "number")) return "Double";
        if (std.mem.eql(u8, t, "boolean")) return "Boolean";
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "Seq[{s}]", .{ir}) catch return "Seq[ujson.Value]";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "Seq[String]";
                if (std.mem.eql(u8, it, "integer")) return "Seq[Long]";
                if (std.mem.eql(u8, it, "number")) return "Seq[Double]";
                if (std.mem.eql(u8, it, "boolean")) return "Seq[Boolean]";
            }
            return "Seq[ujson.Value]";
        }
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "Map[String, String]";
                if (std.mem.eql(u8, vt, "integer")) return "Map[String, Long]";
                if (std.mem.eql(u8, vt, "number")) return "Map[String, Double]";
                if (std.mem.eql(u8, vt, "boolean")) return "Map[String, Boolean]";
            }
            return "Map[String, ujson.Value]";
        }
        return "ujson.Value";
    }

    fn resolveTypeSwift(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "String";
        if (std.mem.eql(u8, t, "string")) return "String";
        if (std.mem.eql(u8, t, "integer")) {
            if (prop.format) |f| {
                if (std.mem.eql(u8, f, "int32")) return "Int32";
            }
            return "Int";
        }
        if (std.mem.eql(u8, t, "number")) return "Double";
        if (std.mem.eql(u8, t, "boolean")) return "Bool";
        if (std.mem.eql(u8, t, "array")) {
            if (prop.items_ref) |ir| return std.fmt.allocPrint(self.allocator, "[{s}]", .{ir}) catch return "[String]";
            if (prop.items_type) |it| {
                if (std.mem.eql(u8, it, "string")) return "[String]";
                if (std.mem.eql(u8, it, "integer")) return "[Int]";
                if (std.mem.eql(u8, it, "number")) return "[Double]";
                if (std.mem.eql(u8, it, "boolean")) return "[Bool]";
            }
            return "[String]";
        }
        if (std.mem.eql(u8, t, "object")) {
            if (prop.additional_properties_type) |vt| {
                if (std.mem.eql(u8, vt, "string")) return "[String: String]";
                if (std.mem.eql(u8, vt, "integer")) return "[String: Int]";
                if (std.mem.eql(u8, vt, "number")) return "[String: Double]";
                if (std.mem.eql(u8, vt, "boolean")) return "[String: Bool]";
            }
            return "[String: String]";
        }
        return "String";
    }

    // ── Query param types for new languages ─────────────────────────────

    fn queryParamTypeJava(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "Boolean";
        if (std.mem.eql(u8, schema_type, "integer")) return "Long";
        if (std.mem.eql(u8, schema_type, "number")) return "Double";
        return "String";
    }

    fn queryParamTypeKotlin(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "Boolean?";
        if (std.mem.eql(u8, schema_type, "integer")) return "Long?";
        if (std.mem.eql(u8, schema_type, "number")) return "Double?";
        return "String?";
    }

    fn queryParamTypeRuby(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "Boolean";
        if (std.mem.eql(u8, schema_type, "integer")) return "Integer";
        if (std.mem.eql(u8, schema_type, "number")) return "Float";
        return "String";
    }

    fn queryParamTypePhp(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "?bool";
        if (std.mem.eql(u8, schema_type, "integer")) return "?int";
        if (std.mem.eql(u8, schema_type, "number")) return "?float";
        return "?string";
    }

    fn queryParamTypeCsharp(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "bool?";
        if (std.mem.eql(u8, schema_type, "integer")) return "long?";
        if (std.mem.eql(u8, schema_type, "number")) return "double?";
        return "string?";
    }

    fn queryParamTypeDart(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "bool?";
        if (std.mem.eql(u8, schema_type, "integer")) return "int?";
        if (std.mem.eql(u8, schema_type, "number")) return "double?";
        return "String?";
    }

    fn queryParamTypeScala(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "Option[Boolean]";
        if (std.mem.eql(u8, schema_type, "integer")) return "Option[Long]";
        if (std.mem.eql(u8, schema_type, "number")) return "Option[Double]";
        return "Option[String]";
    }

    fn queryParamTypeSwift(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "Bool?";
        if (std.mem.eql(u8, schema_type, "integer")) return "Int?";
        if (std.mem.eql(u8, schema_type, "number")) return "Double?";
        return "String?";
    }

    fn queryParamTypeZig(_: *SDKGenerator, schema_type: []const u8) []const u8 {
        if (std.mem.eql(u8, schema_type, "boolean")) return "?bool";
        if (std.mem.eql(u8, schema_type, "integer")) return "?u32";
        if (std.mem.eql(u8, schema_type, "number")) return "?f64";
        return "?[]const u8";
    }

    fn buildQueryParamStringZig(self: *SDKGenerator, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        var first = true;
        for (op.parameters.items) |param| {
            if (param.in == .query) {
                if (!first) try buf.appendSlice(", ");
                first = false;
                try buf.appendSlice(param.name);
                try buf.appendSlice(": ");
                try buf.appendSlice(self.queryParamTypeZig(param.schema_type orelse "string"));
            }
        }
        return try buf.toOwnedSlice();
    }

    // ── Resource grouping for TypeScript ─────────────────────────────────

    fn pathToResourceName(_: *SDKGenerator, path_str: []const u8) []const u8 {
        // Extract first segment after /api/v1/
        const prefix = "/api/v1/";
        if (!std.mem.startsWith(u8, path_str, prefix)) return "misc";
        const rest = path_str[prefix.len..];
        // Find end of first segment
        const slash_pos = std.mem.indexOfScalar(u8, rest, '/') orelse rest.len;
        const segment = rest[0..slash_pos];

        // Map segments to resource names
        if (std.mem.eql(u8, segment, "vm") or std.mem.eql(u8, segment, "vms")) return "vm";
        if (std.mem.eql(u8, segment, "repositories")) return "repositories";
        if (std.mem.eql(u8, segment, "commits")) return "commits";
        if (std.mem.eql(u8, segment, "commit_tags")) return "commitTags";
        if (std.mem.eql(u8, segment, "domains")) return "domains";
        if (std.mem.eql(u8, segment, "env_vars")) return "envVars";
        if (std.mem.eql(u8, segment, "public")) return "publicRepositories";
        return "misc";
    }

    fn resourceNameToClassName(_: *SDKGenerator, name: []const u8) []const u8 {
        if (std.mem.eql(u8, name, "vm")) return "VmResource";
        if (std.mem.eql(u8, name, "repositories")) return "RepositoriesResource";
        if (std.mem.eql(u8, name, "commits")) return "CommitsResource";
        if (std.mem.eql(u8, name, "commitTags")) return "CommitTagsResource";
        if (std.mem.eql(u8, name, "domains")) return "DomainsResource";
        if (std.mem.eql(u8, name, "envVars")) return "EnvVarsResource";
        if (std.mem.eql(u8, name, "publicRepositories")) return "PublicRepositoriesResource";
        return "MiscResource";
    }

    fn resourceNameToFileName(_: *SDKGenerator, name: []const u8) []const u8 {
        if (std.mem.eql(u8, name, "vm")) return "vm.ts";
        if (std.mem.eql(u8, name, "repositories")) return "repositories.ts";
        if (std.mem.eql(u8, name, "commits")) return "commits.ts";
        if (std.mem.eql(u8, name, "commitTags")) return "commit-tags.ts";
        if (std.mem.eql(u8, name, "domains")) return "domains.ts";
        if (std.mem.eql(u8, name, "envVars")) return "env-vars.ts";
        if (std.mem.eql(u8, name, "publicRepositories")) return "public-repositories.ts";
        return "misc.ts";
    }

    fn resourceNameToImportPath(_: *SDKGenerator, name: []const u8) []const u8 {
        if (std.mem.eql(u8, name, "vm")) return "./resources/vm";
        if (std.mem.eql(u8, name, "repositories")) return "./resources/repositories";
        if (std.mem.eql(u8, name, "commits")) return "./resources/commits";
        if (std.mem.eql(u8, name, "commitTags")) return "./resources/commit-tags";
        if (std.mem.eql(u8, name, "domains")) return "./resources/domains";
        if (std.mem.eql(u8, name, "envVars")) return "./resources/env-vars";
        if (std.mem.eql(u8, name, "publicRepositories")) return "./resources/public-repositories";
        return "./resources/misc";
    }

    fn buildResourceContexts(self: *SDKGenerator, base_ctx: *template.Context, ops: []const *template.Context) ![]const *template.Context {
        // Collect unique resource names preserving insertion order
        const resource_names = [_][]const u8{ "vm", "repositories", "commits", "commitTags", "domains", "envVars", "publicRepositories" };

        // Count how many resources actually have operations
        var active_count: usize = 0;
        for (&resource_names) |rn| {
            for (ops) |op| {
                const path_str = op.getString("path") orelse continue;
                if (std.mem.eql(u8, self.pathToResourceName(path_str), rn)) {
                    active_count += 1;
                    break;
                }
            }
        }

        var result = try self.allocator.alloc(*template.Context, active_count);
        var idx: usize = 0;

        for (&resource_names) |rn| {
            // Collect ops for this resource
            var res_ops = std.array_list.Managed(*template.Context).init(self.allocator);
            for (ops) |op| {
                const path_str = op.getString("path") orelse continue;
                if (std.mem.eql(u8, self.pathToResourceName(path_str), rn)) {
                    try res_ops.append(@constCast(op));
                }
            }
            if (res_ops.items.len == 0) continue;

            const rc = try self.allocator.create(template.Context);
            rc.* = template.Context.init(self.allocator);
            rc.parent = base_ctx;

            try rc.putString("resource_name", rn);
            try rc.putString("resource_class", self.resourceNameToClassName(rn));
            try rc.putString("file_name", self.resourceNameToFileName(rn));
            try rc.putString("import_path", self.resourceNameToImportPath(rn));
            // import_name: file_name without .ts extension for re-exports
            const fname = self.resourceNameToFileName(rn);
            if (std.mem.endsWith(u8, fname, ".ts")) {
                try rc.putString("import_name", fname[0 .. fname.len - 3]);
            } else {
                try rc.putString("import_name", fname);
            }
            try rc.putList("operations", try res_ops.toOwnedSlice());

            result[idx] = rc;
            idx += 1;
        }

        return @ptrCast(result[0..idx]);
    }

    fn buildParamsTypeContexts(self: *SDKGenerator, base_ctx: *template.Context, ops: []const *template.Context) ![]const *template.Context {
        // Count operations that have query params (and therefore params_type_name)
        var count: usize = 0;
        for (ops) |op| {
            if (op.getString("params_type_name") != null) count += 1;
        }

        var result = try self.allocator.alloc(*template.Context, count);
        var idx: usize = 0;

        for (ops) |op| {
            const type_name = op.getString("params_type_name") orelse continue;

            const pt = try self.allocator.create(template.Context);
            pt.* = template.Context.init(self.allocator);
            pt.parent = base_ctx;

            try pt.putString("name", type_name);

            // Collect query param fields from the operation's query_params list
            if (op.get("query_params")) |v| {
                switch (v) {
                    .list => |list| try pt.putList("fields", list),
                    else => {},
                }
            }

            result[idx] = pt;
            idx += 1;
        }

        return @ptrCast(result[0..idx]);
    }

    // ── Language generators ─────────────────────────────────────────────

    fn generateTypeScript(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};
        const resources_dir = try std.fmt.allocPrint(self.allocator, "{s}/src/resources", .{d});
        defer self.allocator.free(resources_dir);
        self.makeDirRecursive(resources_dir) catch {};
        const lib_ssh = try std.fmt.allocPrint(self.allocator, "{s}/src/lib/ssh", .{d});
        defer self.allocator.free(lib_ssh);
        self.makeDirRecursive(lib_ssh) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        // Build resource groups for TypeScript
        const resource_ctxs = try self.buildResourceContexts(ctx, ops);
        try ctx.putList("resources", resource_ctxs);

        // Render one file per resource group
        for (resource_ctxs) |rc| {
            const file_name = rc.getString("file_name") orelse continue;
            const out_rel = try std.fmt.allocPrint(self.allocator, "src/resources/{s}", .{file_name});
            defer self.allocator.free(out_rel);
            try self.renderTo("templates/typescript/resource.ts.template", d, out_rel, rc);
        }

        // Render resource index
        try self.renderTo("templates/typescript/resource-index.ts.template", d, "src/resources/index.ts", ctx);

        try self.renderTo("templates/typescript/request-options.ts.template", d, "src/request-options.ts", ctx);
        try self.renderTo("templates/typescript/api-promise.ts.template", d, "src/api-promise.ts", ctx);
        try self.renderTo("templates/typescript/shims.ts.template", d, "src/shims.ts", ctx);
        try self.renderTo("templates/typescript/client.ts.template", d, "src/client.ts", ctx);
        try self.renderTo("templates/typescript/models.ts.template", d, "src/models.ts", ctx);
        try self.renderTo("templates/typescript/errors.ts.template", d, "src/errors.ts", ctx);
        try self.renderTo("templates/typescript/index.ts.template", d, "src/index.ts", ctx);
        try self.renderTo("templates/typescript/tsconfig.json.template", d, "tsconfig.json", ctx);
        try self.renderTo("templates/typescript/package.json.template", d, "package.json", ctx);
        try self.renderTo("templates/typescript/README.md.template", d, "README.md", ctx);
        try self.renderTo("templates/typescript/.gitignore.template", d, ".gitignore", ctx);
        try self.renderTo("templates/typescript/lib/ssh/client.ts.template", d, "src/lib/ssh/client.ts", ctx);
        try self.renderTo("templates/typescript/lib/ssh/errors.ts.template", d, "src/lib/ssh/errors.ts", ctx);
        try self.renderTo("templates/typescript/lib/ssh/types.ts.template", d, "src/lib/ssh/types.ts", ctx);
        try self.renderTo("templates/typescript/lib/ssh/index.ts.template", d, "src/lib/ssh/index.ts", ctx);
        try self.renderTo("templates/typescript/lib/vm-ssh.ts.template", d, "src/lib/vm-ssh.ts", ctx);
        const tests_dir = try std.fmt.allocPrint(self.allocator, "{s}/tests", .{d});
        defer self.allocator.free(tests_dir);
        self.makeDirRecursive(tests_dir) catch {};
        try self.renderTo("templates/typescript/tests/client.test.ts.template", d, "tests/client.test.ts", ctx);
        std.debug.print("Generated TypeScript SDK at {s}\n", .{d});
    }

    fn generateRust(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/rust/client.rs.template", d, "src/client.rs", ctx);
        try self.renderTo("templates/rust/models.rs.template", d, "src/models.rs", ctx);
        try self.renderTo("templates/rust/errors.rs.template", d, "src/errors.rs", ctx);
        try self.renderTo("templates/rust/lib.rs.template", d, "src/lib.rs", ctx);
        try self.renderTo("templates/rust/cargo.toml.template", d, "Cargo.toml", ctx);
        try self.renderTo("templates/rust/README.md.template", d, "README.md", ctx);
        const tests_dir = try std.fmt.allocPrint(self.allocator, "{s}/tests", .{d});
        defer self.allocator.free(tests_dir);
        self.makeDirRecursive(tests_dir) catch {};
        try self.renderTo("templates/rust/tests/client_test.rs.template", d, "tests/client_test.rs", ctx);
        std.debug.print("Generated Rust SDK at {s}\n", .{d});
    }

    fn generatePython(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src/vers_sdk", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/python/client.py.template", d, "src/vers_sdk/client.py", ctx);
        try self.renderTo("templates/python/models.py.template", d, "src/vers_sdk/models.py", ctx);
        try self.renderTo("templates/python/errors.py.template", d, "src/vers_sdk/errors.py", ctx);
        try self.renderTo("templates/python/__init__.py.template", d, "src/vers_sdk/__init__.py", ctx);
        try self.renderTo("templates/python/pyproject.toml.template", d, "pyproject.toml", ctx);
        try self.renderTo("templates/python/README.md.template", d, "README.md", ctx);
        const tests_dir = try std.fmt.allocPrint(self.allocator, "{s}/tests", .{d});
        defer self.allocator.free(tests_dir);
        self.makeDirRecursive(tests_dir) catch {};
        try self.renderTo("templates/python/tests/test_client.py.template", d, "tests/test_client.py", ctx);
        std.debug.print("Generated Python SDK at {s}\n", .{d});
    }

    fn generateGo(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/go/client.go.template", d, "client.go", ctx);
        try self.renderTo("templates/go/models.go.template", d, "models.go", ctx);
        try self.renderTo("templates/go/errors.go.template", d, "errors.go", ctx);
        try self.renderTo("templates/go/go.mod.template", d, "go.mod", ctx);
        try self.renderTo("templates/go/README.md.template", d, "README.md", ctx);
        try self.renderTo("templates/go/client_test.go.template", d, "client_test.go", ctx);
        std.debug.print("Generated Go SDK at {s}\n", .{d});
    }

    fn generateZig(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};

        const ctx = try self.buildBaseContext();
        try ctx.putList("operations", try self.buildOperationContexts(ctx));

        // Zig module names use underscores, not hyphens
        const name = self.cfg.project.name;
        const zig_name = try self.allocator.alloc(u8, name.len);
        @memcpy(zig_name, name);
        for (zig_name) |*c| {
            if (c.* == '-') c.* = '_';
        }
        try ctx.putString("zig_module_name", zig_name);

        try self.renderTo("templates/zig/client.zig.template", d, "src/client.zig", ctx);
        try self.renderTo("templates/zig/build.zig.template", d, "build.zig", ctx);
        try self.renderTo("templates/zig/README.md.template", d, "README.md", ctx);
        std.debug.print("Generated Zig SDK at {s}\n", .{d});
    }

    fn generateJava(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src/main/java/sh/vers/sdk", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};
        const test_dir = try std.fmt.allocPrint(self.allocator, "{s}/src/test/java/sh/vers/sdk", .{d});
        defer self.allocator.free(test_dir);
        self.makeDirRecursive(test_dir) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/java/Client.java.template", d, "src/main/java/sh/vers/sdk/VersClient.java", ctx);
        try self.renderTo("templates/java/Models.java.template", d, "src/main/java/sh/vers/sdk/Models.java", ctx);
        try self.renderTo("templates/java/Errors.java.template", d, "src/main/java/sh/vers/sdk/Errors.java", ctx);
        try self.renderTo("templates/java/RequestOptions.java.template", d, "src/main/java/sh/vers/sdk/RequestOptions.java", ctx);
        try self.renderTo("templates/java/pom.xml.template", d, "pom.xml", ctx);
        try self.renderTo("templates/java/README.md.template", d, "README.md", ctx);
        try self.renderTo("templates/java/ClientTest.java.template", d, "src/test/java/sh/vers/sdk/VersClientTest.java", ctx);
        std.debug.print("Generated Java SDK at {s}\n", .{d});
    }

    fn generateKotlin(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src/main/kotlin/sh/vers/sdk", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};
        const test_dir = try std.fmt.allocPrint(self.allocator, "{s}/src/test/kotlin/sh/vers/sdk", .{d});
        defer self.allocator.free(test_dir);
        self.makeDirRecursive(test_dir) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/kotlin/Client.kt.template", d, "src/main/kotlin/sh/vers/sdk/VersClient.kt", ctx);
        try self.renderTo("templates/kotlin/Models.kt.template", d, "src/main/kotlin/sh/vers/sdk/Models.kt", ctx);
        try self.renderTo("templates/kotlin/Errors.kt.template", d, "src/main/kotlin/sh/vers/sdk/Errors.kt", ctx);
        try self.renderTo("templates/kotlin/RequestOptions.kt.template", d, "src/main/kotlin/sh/vers/sdk/RequestOptions.kt", ctx);
        try self.renderTo("templates/kotlin/build.gradle.kts.template", d, "build.gradle.kts", ctx);
        try self.renderTo("templates/kotlin/README.md.template", d, "README.md", ctx);
        try self.renderTo("templates/kotlin/ClientTest.kt.template", d, "src/test/kotlin/sh/vers/sdk/VersClientTest.kt", ctx);
        std.debug.print("Generated Kotlin SDK at {s}\n", .{d});
    }

    fn generateRuby(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const lib_dir = try std.fmt.allocPrint(self.allocator, "{s}/lib/vers_sdk", .{d});
        defer self.allocator.free(lib_dir);
        self.makeDirRecursive(lib_dir) catch {};
        const test_dir = try std.fmt.allocPrint(self.allocator, "{s}/test", .{d});
        defer self.allocator.free(test_dir);
        self.makeDirRecursive(test_dir) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/ruby/client.rb.template", d, "lib/vers_sdk/client.rb", ctx);
        try self.renderTo("templates/ruby/models.rb.template", d, "lib/vers_sdk/models.rb", ctx);
        try self.renderTo("templates/ruby/errors.rb.template", d, "lib/vers_sdk/errors.rb", ctx);
        try self.renderTo("templates/ruby/vers_sdk.rb.template", d, "lib/vers_sdk.rb", ctx);
        try self.renderTo("templates/ruby/gemspec.template", d, "vers-sdk.gemspec", ctx);
        try self.renderTo("templates/ruby/README.md.template", d, "README.md", ctx);
        try self.renderTo("templates/ruby/test_client.rb.template", d, "test/test_client.rb", ctx);
        std.debug.print("Generated Ruby SDK at {s}\n", .{d});
    }

    fn generatePhp(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};
        const test_dir = try std.fmt.allocPrint(self.allocator, "{s}/tests", .{d});
        defer self.allocator.free(test_dir);
        self.makeDirRecursive(test_dir) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/php/Client.php.template", d, "src/Client.php", ctx);
        try self.renderTo("templates/php/Models.php.template", d, "src/Models.php", ctx);
        try self.renderTo("templates/php/Errors.php.template", d, "src/Errors.php", ctx);
        try self.renderTo("templates/php/composer.json.template", d, "composer.json", ctx);
        try self.renderTo("templates/php/README.md.template", d, "README.md", ctx);
        try self.renderTo("templates/php/ClientTest.php.template", d, "tests/ClientTest.php", ctx);
        std.debug.print("Generated PHP SDK at {s}\n", .{d});
    }

    fn generateCsharp(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/csharp/VersClient.cs.template", d, "VersClient.cs", ctx);
        try self.renderTo("templates/csharp/Models.cs.template", d, "Models.cs", ctx);
        try self.renderTo("templates/csharp/Errors.cs.template", d, "Errors.cs", ctx);
        try self.renderTo("templates/csharp/VersSdk.csproj.template", d, "VersSdk.csproj", ctx);
        try self.renderTo("templates/csharp/README.md.template", d, "README.md", ctx);
        try self.renderTo("templates/csharp/VersClientTest.cs.template", d, "VersClientTest.cs", ctx);
        std.debug.print("Generated C# SDK at {s}\n", .{d});
    }

    fn generateDart(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const lib_dir = try std.fmt.allocPrint(self.allocator, "{s}/lib", .{d});
        defer self.allocator.free(lib_dir);
        self.makeDirRecursive(lib_dir) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));
        // Dart needs repository for pubspec and hyphen-free package name
        try ctx.putString("repository", target.repository);
        // Dart identifiers can't have hyphens
        const pkg = self.cfg.project.name;
        var dart_pkg_buf: [256]u8 = undefined;
        var dart_pkg_len: usize = 0;
        for (pkg) |c| {
            if (dart_pkg_len < 256) {
                dart_pkg_buf[dart_pkg_len] = if (c == '-') '_' else c;
                dart_pkg_len += 1;
            }
        }
        try ctx.putString("dart_package_name", try self.allocator.dupe(u8, dart_pkg_buf[0..dart_pkg_len]));

        try self.renderTo("templates/dart/client.dart.template", d, "lib/client.dart", ctx);
        try self.renderTo("templates/dart/models.dart.template", d, "lib/models.dart", ctx);
        try self.renderTo("templates/dart/errors.dart.template", d, "lib/errors.dart", ctx);
        try self.renderTo("templates/dart/index.dart.template", d, "lib/index.dart", ctx);
        try self.renderTo("templates/dart/pubspec.yaml.template", d, "pubspec.yaml", ctx);
        try self.renderTo("templates/dart/README.md.template", d, "README.md", ctx);
        try self.renderTo("templates/dart/LICENSE.template", d, "LICENSE", ctx);
        try self.renderTo("templates/dart/CHANGELOG.md.template", d, "CHANGELOG.md", ctx);
        std.debug.print("Generated Dart SDK at {s}\n", .{d});
    }

    fn generateScala(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src/main/scala/sh/vers/sdk", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};
        const project_dir = try std.fmt.allocPrint(self.allocator, "{s}/project", .{d});
        defer self.allocator.free(project_dir);
        self.makeDirRecursive(project_dir) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/scala/Client.scala.template", d, "src/main/scala/sh/vers/sdk/Client.scala", ctx);
        try self.renderTo("templates/scala/Models.scala.template", d, "src/main/scala/sh/vers/sdk/Models.scala", ctx);
        try self.renderTo("templates/scala/Errors.scala.template", d, "src/main/scala/sh/vers/sdk/Errors.scala", ctx);
        try self.renderTo("templates/scala/build.sbt.template", d, "build.sbt", ctx);
        try self.renderTo("templates/scala/project/plugins.sbt.template", d, "project/plugins.sbt", ctx);
        try self.renderTo("templates/scala/project/build.properties.template", d, "project/build.properties", ctx);
        try self.renderTo("templates/scala/README.md.template", d, "README.md", ctx);
        std.debug.print("Generated Scala SDK at {s}\n", .{d});
    }

    fn generateSwift(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/Sources", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};

        const ctx = try self.buildBaseContext();
        const ops = try self.buildOperationContexts(ctx);
        try ctx.putList("operations", ops);
        try ctx.putList("models", try self.buildModelContexts(ctx));
        try ctx.putList("params_types", try self.buildParamsTypeContexts(ctx, ops));

        try self.renderTo("templates/swift/Client.swift.template", d, "Sources/Client.swift", ctx);
        try self.renderTo("templates/swift/Models.swift.template", d, "Sources/Models.swift", ctx);
        try self.renderTo("templates/swift/Errors.swift.template", d, "Sources/Errors.swift", ctx);
        try self.renderTo("templates/swift/Package.swift.template", d, "Package.swift", ctx);
        try self.renderTo("templates/swift/README.md.template", d, "README.md", ctx);
        try self.renderTo("templates/swift/.gitignore.template", d, ".gitignore", ctx);
        std.debug.print("Generated Swift SDK at {s}\n", .{d});
    }

    // ── Helpers ─────────────────────────────────────────────────────────

    pub fn makeDirRecursive(_: *SDKGenerator, path: []const u8) !void {
        std.fs.cwd().makePath(path) catch |err| switch (err) {
            error.PathAlreadyExists => {},
            else => return err,
        };
    }

    fn renderTo(self: *SDKGenerator, tmpl_path: []const u8, out_dir: []const u8, rel: []const u8, ctx: *template.Context) !void {
        const out_path = try std.fmt.allocPrint(self.allocator, "{s}/{s}", .{ out_dir, rel });
        defer self.allocator.free(out_path);
        const tmpl = loadTemplate(self.allocator, tmpl_path) catch |err| {
            std.debug.print("Warning: template not found: {s} ({any})\n", .{ tmpl_path, err });
            return;
        };
        defer self.allocator.free(tmpl);
        var engine = template.Engine.init(self.allocator);
        const content = try engine.render(tmpl, ctx);

        // Optional LLM enhancement pass
        const final_content = if (self.enhance and self.enhancer != null) blk: {
            // Only enhance source code files, not configs/READMEs
            const is_code = std.mem.endsWith(u8, rel, ".ts") or
                std.mem.endsWith(u8, rel, ".rs") or
                std.mem.endsWith(u8, rel, ".py") or
                std.mem.endsWith(u8, rel, ".go") or
                std.mem.endsWith(u8, rel, ".zig") or
                std.mem.endsWith(u8, rel, ".java") or
                std.mem.endsWith(u8, rel, ".kt") or
                std.mem.endsWith(u8, rel, ".rb") or
                std.mem.endsWith(u8, rel, ".php") or
                std.mem.endsWith(u8, rel, ".cs") or
                std.mem.endsWith(u8, rel, ".dart") or
                std.mem.endsWith(u8, rel, ".scala") or
                std.mem.endsWith(u8, rel, ".swift");
            if (is_code) {
                const lang = if (std.mem.endsWith(u8, rel, ".ts")) "typescript"
                    else if (std.mem.endsWith(u8, rel, ".rs")) "rust"
                    else if (std.mem.endsWith(u8, rel, ".py")) "python"
                    else if (std.mem.endsWith(u8, rel, ".go")) "go"
                    else if (std.mem.endsWith(u8, rel, ".java")) "java"
                    else if (std.mem.endsWith(u8, rel, ".kt")) "kotlin"
                    else if (std.mem.endsWith(u8, rel, ".rb")) "ruby"
                    else if (std.mem.endsWith(u8, rel, ".php")) "php"
                    else if (std.mem.endsWith(u8, rel, ".cs")) "csharp"
                    else if (std.mem.endsWith(u8, rel, ".dart")) "dart"
                    else if (std.mem.endsWith(u8, rel, ".scala")) "scala"
                    else if (std.mem.endsWith(u8, rel, ".swift")) "swift"
                    else "zig";
                std.debug.print("  Enhancing {s}...\n", .{rel});
                break :blk self.enhancer.?.enhance(content, lang, rel);
            }
            break :blk content;
        } else content;
        defer self.allocator.free(final_content);

        const file = try std.fs.cwd().createFile(out_path, .{});
        defer file.close();
        try file.writeAll(final_content);
    }

    // ── String sanitisation ────────────────────────────────────────────

    /// Replace newlines and double-quotes in a string so it's safe for
    /// single-line doc comments (Rust `///`, Go `//`, Python `"""`).
    fn sanitiseOneLine(self: *SDKGenerator, input: []const u8) []const u8 {
        const has_nl = std.mem.indexOfScalar(u8, input, '\n') != null;
        const has_dq = std.mem.indexOfScalar(u8, input, '"') != null;
        if (!has_nl and !has_dq) return input;
        var buf = std.array_list.Managed(u8).init(self.allocator);
        for (input) |c| {
            if (c == '\n' or c == '\r') {
                buf.append(' ') catch return input;
            } else if (c == '"') {
                buf.append('\'') catch return input;
            } else {
                buf.append(c) catch return input;
            }
        }
        return buf.toOwnedSlice() catch input;
    }

    // ── Case conversion ─────────────────────────────────────────────────

    fn toSnakeCaseStatic(input: []const u8, buf: *[256]u8) []const u8 {
        var pos: usize = 0;
        for (input, 0..) |c, i| {
            if (std.ascii.isUpper(c)) {
                if (i > 0 and pos < 255) { buf[pos] = '_'; pos += 1; }
                if (pos < 256) { buf[pos] = std.ascii.toLower(c); pos += 1; }
            } else {
                if (pos < 256) { buf[pos] = c; pos += 1; }
            }
        }
        return buf[0..pos];
    }

    fn toPascalCaseStatic(input: []const u8, buf: *[256]u8) []const u8 {
        var pos: usize = 0;
        var cap = true;
        for (input) |c| {
            if (c == '_' or c == '-' or c == ' ') { cap = true; continue; }
            if (pos < 256) { buf[pos] = if (cap) std.ascii.toUpper(c) else c; pos += 1; cap = false; }
        }
        return buf[0..pos];
    }

    fn toCamelCaseStatic(input: []const u8, buf: *[256]u8) []const u8 {
        var pos: usize = 0;
        var cap = false;
        var first = true;
        for (input) |c| {
            if (c == '_' or c == '-' or c == ' ') { cap = true; continue; }
            if (first) {
                // First character always lowercase
                if (pos < 256) { buf[pos] = std.ascii.toLower(c); pos += 1; }
                first = false;
                cap = false;
            } else if (cap) {
                if (pos < 256) { buf[pos] = std.ascii.toUpper(c); pos += 1; }
                cap = false;
            } else {
                if (pos < 256) { buf[pos] = c; pos += 1; }
            }
        }
        return buf[0..pos];
    }

    fn toUpperStatic(input: []const u8, buf: *[256]u8) []const u8 {
        var pos: usize = 0;
        for (input) |c| {
            if (pos < 256) { buf[pos] = std.ascii.toUpper(c); pos += 1; }
        }
        return buf[0..pos];
    }
};
