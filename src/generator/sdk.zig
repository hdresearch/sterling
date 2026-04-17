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

        var path_iter = self.spec.paths.iterator();
        while (path_iter.next()) |entry| {
            const path_str = entry.key_ptr.*;
            const pi = entry.value_ptr;

            inline for (.{ "get", "post", "put", "delete", "patch" }) |method| {
                if (@field(pi, method)) |op| {
                    if (op.operationId) |op_id| {
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
                            try c.putString("path_params_py", param_names.py_params);
                            try c.putString("path_params_go", param_names.go_params);
                            try c.putString("path_params_rust", param_names.rust_params);
                            try c.putString("path_interpolate_ts", param_names.ts_interpolate);
                            try c.putString("path_interpolate_py", param_names.py_interpolate);
                            try c.putString("path_interpolate_go", param_names.go_interpolate);
                            try c.putString("path_interpolate_rust", param_names.rust_interpolate);
                            try c.putString("go_format_args", param_names.go_format_args);
                            try c.putString("rust_format_args", param_names.rust_format_args);
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
                        var resp_iter = op.responses.iterator();
                        while (resp_iter.next()) |re| {
                            // Look for 200/201 response with schema ref
                            if (std.mem.startsWith(u8, re.key_ptr.*, "2")) {
                                if (re.value_ptr.schema_ref) |ref| {
                                    response_type = ref;
                                    break;
                                }
                            }
                        }
                        if (response_type.len > 0) {
                            try c.putString("response_type", response_type);
                            try c.putBool("has_typed_response", true);
                        } else {
                            try c.putBool("has_typed_response", false);
                        }

                        // Query params
                        var query_params_list = std.array_list.Managed(*template.Context).init(self.allocator);
                        for (op.parameters.items) |param| {
                            if (param.in == .query) {
                                const qc = try self.allocator.create(template.Context);
                                qc.* = template.Context.init(self.allocator);
                                qc.parent = c;
                                try qc.putString("name", param.name);
                                try qc.putString("description", self.sanitiseOneLine(param.description orelse ""));
                                try qc.putBool("required", param.required);
                                const schema_type = param.schema_type orelse "string";
                                try qc.putString("ts_type", self.queryParamTypeTS(schema_type));
                                try qc.putString("py_type", self.queryParamTypePython(schema_type));
                                try qc.putString("go_type", self.queryParamTypeGo(schema_type));
                                try qc.putString("rust_type", self.queryParamTypeRust(schema_type));
                                // Go format helper: how to convert to string for URL
                                try qc.putString("go_format", self.queryParamGoFormat(param.name, schema_type));
                                try query_params_list.append(qc);
                            }
                        }
                        const has_query_params = query_params_list.items.len > 0;
                        try c.putBool("has_query_params", has_query_params);
                        try c.putBool("query_needs_comma", has_query_params and (has_path_params or has_body));
                        if (has_query_params) {
                            const qp_slice = try query_params_list.toOwnedSlice();
                            try c.putList("query_params", @ptrCast(qp_slice));
                            // Build per-language param strings for function signatures
                            try c.putString("query_params_ts", try self.buildQueryParamStringTS(op));
                            try c.putString("query_params_py", try self.buildQueryParamStringPython(op));
                            try c.putString("query_params_go", try self.buildQueryParamStringGo(op));
                            try c.putString("query_params_rust", try self.buildQueryParamStringRust(op));
                        }

                        // Rust fn_params
                        try c.putString("fn_params", try self.buildRustFnParams(has_path_params, has_body, has_query_params, op));

                        ops[idx] = c;
                        idx += 1;
                    }
                }
            }
        }
        return @ptrCast(ops[0..idx]);
    }

    const PathParamInfo = struct {
        ts_params: []const u8,
        py_params: []const u8,
        go_params: []const u8,
        rust_params: []const u8,
        ts_interpolate: []const u8,
        py_interpolate: []const u8,
        go_interpolate: []const u8,
        rust_interpolate: []const u8,
        go_format_args: []const u8,
        rust_format_args: []const u8,
    };

    fn extractPathParamNames(self: *SDKGenerator, path: []const u8) !PathParamInfo {
        var ts_params = std.array_list.Managed(u8).init(self.allocator);
        var py_params = std.array_list.Managed(u8).init(self.allocator);
        var go_params = std.array_list.Managed(u8).init(self.allocator);
        var rust_params = std.array_list.Managed(u8).init(self.allocator);
        var ts_interp = std.array_list.Managed(u8).init(self.allocator);
        var py_interp = std.array_list.Managed(u8).init(self.allocator);
        var go_interp = std.array_list.Managed(u8).init(self.allocator);
        var rust_interp = std.array_list.Managed(u8).init(self.allocator);
        var go_fmt_args = std.array_list.Managed(u8).init(self.allocator);
        var rust_fmt_args = std.array_list.Managed(u8).init(self.allocator);

        // Build interpolated path and param lists
        var i: usize = 0;
        var param_count: usize = 0;
        while (i < path.len) {
            if (path[i] == '{') {
                const end = std.mem.indexOfScalarPos(u8, path, i + 1, '}') orelse break;
                const name = path[i + 1 .. end];

                if (param_count > 0) {
                    try ts_params.appendSlice(", ");
                    try py_params.appendSlice(", ");
                    try go_params.appendSlice(", ");
                    try rust_params.appendSlice(", ");
                }
                // TS: name: string
                try ts_params.appendSlice(name);
                try ts_params.appendSlice(": string");
                // Python: name: str
                try py_params.appendSlice(name);
                try py_params.appendSlice(": str");
                // Go: name string
                try go_params.appendSlice(name);
                try go_params.appendSlice(" string");
                // Rust: name: &str
                try rust_params.appendSlice(name);
                try rust_params.appendSlice(": &str");

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

                // Format arguments for Go and Rust
                if (param_count > 0) {
                    try go_fmt_args.appendSlice(", ");
                    try rust_fmt_args.appendSlice(", ");
                }
                try go_fmt_args.appendSlice(name);
                try rust_fmt_args.appendSlice(name);

                param_count += 1;
                i = end + 1;
            } else {
                try ts_interp.append(path[i]);
                try py_interp.append(path[i]);
                try go_interp.append(path[i]);
                try rust_interp.append(path[i]);
                i += 1;
            }
        }

        return .{
            .ts_params = try ts_params.toOwnedSlice(),
            .py_params = try py_params.toOwnedSlice(),
            .go_params = try go_params.toOwnedSlice(),
            .rust_params = try rust_params.toOwnedSlice(),
            .ts_interpolate = try ts_interp.toOwnedSlice(),
            .py_interpolate = try py_interp.toOwnedSlice(),
            .go_interpolate = try go_interp.toOwnedSlice(),
            .rust_interpolate = try rust_interp.toOwnedSlice(),
            .go_format_args = try go_fmt_args.toOwnedSlice(),
            .rust_format_args = try rust_fmt_args.toOwnedSlice(),
        };
    }

    fn buildRustFnParams(self: *SDKGenerator, has_path_params: bool, has_body: bool, has_query_params: bool, op: parser.Operation) ![]const u8 {
        var buf = std.array_list.Managed(u8).init(self.allocator);
        try buf.appendSlice("&self");
        if (has_path_params) {
            for (op.parameters.items) |param| {
                if (param.in == .path) {
                    try buf.appendSlice(", ");
                    try buf.appendSlice(param.name);
                    try buf.appendSlice(": &str");
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
            for (op.parameters.items) |param| {
                if (param.in == .query) {
                    try buf.appendSlice(", ");
                    try buf.appendSlice(param.name);
                    try buf.appendSlice(": Option<");
                    try buf.appendSlice(self.queryParamTypeRust(param.schema_type orelse "string"));
                    try buf.appendSlice(">");
                }
            }
        }
        return try buf.toOwnedSlice();
    }

    // ── Query param type helpers ─────────────────────────────────────────

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
                        var vp_pascal_buf: [256]u8 = undefined;
                        try vpc.putString("pascal_name", try self.allocator.dupe(u8, toPascalCaseStatic(vprop.name, &vp_pascal_buf)));
                        try vpc.putBool("required", vprop.required);
                        try vpc.putString("description", self.sanitiseOneLine(vprop.description orelse ""));
                        try vpc.putString("ts_type", self.resolveTypeTS(vprop));
                        try vpc.putString("rust_type", self.resolveTypeRust(vprop));
                        try vpc.putString("py_type", self.resolveTypePython(vprop));
                        try vpc.putString("go_type", self.resolveTypeGo(vprop));
                        // Rust-safe field name (avoid keyword 'ref')
                        if (std.mem.eql(u8, vprop.name, "ref")) {
                            try vpc.putString("rust_name", "ref_field");
                            try vpc.putBool("is_renamed", true);
                        } else {
                            try vpc.putString("rust_name", vprop.name);
                            try vpc.putBool("is_renamed", false);
                        }
                        vprop_ctxs[vpi] = vpc;
                    }
                    try vc.putList("properties", @ptrCast(vprop_ctxs));

                    variant_ctxs[vi] = vc;
                }
                try m.putList("variants", @ptrCast(variant_ctxs));
            }

            // Properties (for struct types)
            if (schema.properties.items.len > 0) {
                var prop_ctxs = try self.allocator.alloc(*template.Context, schema.properties.items.len);
                for (schema.properties.items, 0..) |prop, pi| {
                    const pc = try self.allocator.create(template.Context);
                    pc.* = template.Context.init(self.allocator);
                    pc.parent = m;
                    try pc.putString("name", prop.name);
                    // Rust-safe name (prefix with r# if keyword)
                    try pc.putString("rust_name", try self.rustSafeName(prop.name));
                    var prop_pascal_buf: [256]u8 = undefined;
                    try pc.putString("pascal_name", try self.allocator.dupe(u8, toPascalCaseStatic(prop.name, &prop_pascal_buf)));
                    try pc.putBool("required", prop.required);
                    try pc.putString("description", self.sanitiseOneLine(prop.description orelse ""));

                    // Resolve type for each language
                    try pc.putString("ts_type", self.resolveTypeTS(prop));
                    try pc.putString("rust_type", self.resolveTypeRust(prop));
                    try pc.putString("py_type", self.resolveTypePython(prop));
                    try pc.putString("go_type", self.resolveTypeGo(prop));

                    // Has $ref?
                    if (prop.ref) |ref| {
                        try pc.putString("ref", ref);
                        try pc.putBool("has_ref", true);
                    } else {
                        try pc.putBool("has_ref", false);
                    }

                    prop_ctxs[pi] = pc;
                }
                try m.putList("properties", @ptrCast(prop_ctxs));
                try m.putBool("has_properties", true);
            } else {
                try m.putBool("has_properties", false);
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
        if (std.mem.eql(u8, t, "object")) return "Record<string, unknown>";
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
        return "serde_json::Value";
    }

    fn resolveTypePython(self: *SDKGenerator, prop: parser.SchemaProperty) []const u8 {
        if (prop.ref) |ref| return ref;
        const t = prop.type_name orelse return "Any";
        if (std.mem.eql(u8, t, "string")) return "str";
        if (std.mem.eql(u8, t, "integer")) return "int";
        if (std.mem.eql(u8, t, "number")) return "float";
        if (std.mem.eql(u8, t, "boolean")) return "bool";
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
        return "interface{}";
    }

    // ── Language generators ─────────────────────────────────────────────

    fn generateTypeScript(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};

        const ctx = try self.buildBaseContext();
        try ctx.putList("operations", try self.buildOperationContexts(ctx));
        try ctx.putList("models", try self.buildModelContexts(ctx));

        try self.renderTo("templates/typescript/request-options.ts.template", d, "src/request-options.ts", ctx);
        try self.renderTo("templates/typescript/api-promise.ts.template", d, "src/api-promise.ts", ctx);
        try self.renderTo("templates/typescript/client.ts.template", d, "src/client.ts", ctx);
        try self.renderTo("templates/typescript/models.ts.template", d, "src/models.ts", ctx);
        try self.renderTo("templates/typescript/errors.ts.template", d, "src/errors.ts", ctx);
        try self.renderTo("templates/typescript/index.ts.template", d, "src/index.ts", ctx);
        try self.renderTo("templates/typescript/tsconfig.json.template", d, "tsconfig.json", ctx);
        try self.renderTo("templates/typescript/package.json.template", d, "package.json", ctx);
        try self.renderTo("templates/typescript/README.md.template", d, "README.md", ctx);
        std.debug.print("Generated TypeScript SDK at {s}\n", .{d});
    }

    fn generateRust(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};

        const ctx = try self.buildBaseContext();
        try ctx.putList("operations", try self.buildOperationContexts(ctx));
        try ctx.putList("models", try self.buildModelContexts(ctx));

        try self.renderTo("templates/rust/client.rs.template", d, "src/client.rs", ctx);
        try self.renderTo("templates/rust/models.rs.template", d, "src/models.rs", ctx);
        try self.renderTo("templates/rust/errors.rs.template", d, "src/errors.rs", ctx);
        try self.renderTo("templates/rust/lib.rs.template", d, "src/lib.rs", ctx);
        try self.renderTo("templates/rust/cargo.toml.template", d, "Cargo.toml", ctx);
        std.debug.print("Generated Rust SDK at {s}\n", .{d});
    }

    fn generatePython(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};
        const src = try std.fmt.allocPrint(self.allocator, "{s}/src", .{d});
        defer self.allocator.free(src);
        self.makeDirRecursive(src) catch {};

        const ctx = try self.buildBaseContext();
        try ctx.putList("operations", try self.buildOperationContexts(ctx));
        try ctx.putList("models", try self.buildModelContexts(ctx));

        try self.renderTo("templates/python/client.py.template", d, "src/client.py", ctx);
        try self.renderTo("templates/python/models.py.template", d, "src/models.py", ctx);
        try self.renderTo("templates/python/errors.py.template", d, "src/errors.py", ctx);
        try self.renderTo("templates/python/__init__.py.template", d, "src/__init__.py", ctx);
        try self.renderTo("templates/python/pyproject.toml.template", d, "pyproject.toml", ctx);
        try self.renderTo("templates/python/README.md.template", d, "README.md", ctx);
        std.debug.print("Generated Python SDK at {s}\n", .{d});
    }

    fn generateGo(self: *SDKGenerator, target: config.Config.Target) !void {
        const d = target.output_dir;
        self.makeDirRecursive(d) catch {};

        const ctx = try self.buildBaseContext();
        try ctx.putList("operations", try self.buildOperationContexts(ctx));
        try ctx.putList("models", try self.buildModelContexts(ctx));

        try self.renderTo("templates/go/client.go.template", d, "client.go", ctx);
        try self.renderTo("templates/go/models.go.template", d, "models.go", ctx);
        try self.renderTo("templates/go/errors.go.template", d, "errors.go", ctx);
        try self.renderTo("templates/go/go.mod.template", d, "go.mod", ctx);
        try self.renderTo("templates/go/README.md.template", d, "README.md", ctx);
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

        try self.renderTo("templates/zig/client.zig.template", d, "src/client.zig", ctx);
        try self.renderTo("templates/zig/build.zig.template", d, "build.zig", ctx);
        try self.renderTo("templates/zig/README.md.template", d, "README.md", ctx);
        std.debug.print("Generated Zig SDK at {s}\n", .{d});
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
                std.mem.endsWith(u8, rel, ".zig");
            if (is_code) {
                const lang = if (std.mem.endsWith(u8, rel, ".ts")) "typescript"
                    else if (std.mem.endsWith(u8, rel, ".rs")) "rust"
                    else if (std.mem.endsWith(u8, rel, ".py")) "python"
                    else if (std.mem.endsWith(u8, rel, ".go")) "go"
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
