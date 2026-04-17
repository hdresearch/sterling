const std = @import("std");

pub const ServerList = struct {
    items: []const []const u8 = &.{},
};

pub const OpenAPISpec = struct {
    allocator: std.mem.Allocator,
    openapi: []const u8,
    info: Info,
    paths: std.StringHashMap(PathItem),
    components: ?Components = null,
    servers: ServerList = .{},

    pub fn deinit(self: *OpenAPISpec) void {
        var path_iter = self.paths.iterator();
        while (path_iter.next()) |entry| {
            entry.value_ptr.deinit(self.allocator);
        }
        self.paths.deinit();
        if (self.components) |*comp| {
            comp.deinit();
        }
    }
};

pub const Info = struct {
    title: []const u8,
    version: []const u8,
    description: ?[]const u8 = null,
};

pub const PathItem = struct {
    get: ?Operation = null,
    post: ?Operation = null,
    put: ?Operation = null,
    delete: ?Operation = null,
    patch: ?Operation = null,

    pub fn deinit(self: *PathItem, allocator: std.mem.Allocator) void {
        if (self.get) |*op| op.deinit(allocator);
        if (self.post) |*op| op.deinit(allocator);
        if (self.put) |*op| op.deinit(allocator);
        if (self.delete) |*op| op.deinit(allocator);
        if (self.patch) |*op| op.deinit(allocator);
    }
};

pub const Operation = struct {
    operationId: ?[]const u8 = null,
    summary: ?[]const u8 = null,
    description: ?[]const u8 = null,
    parameters: std.array_list.Managed(Parameter),
    requestBody: ?RequestBody = null,
    responses: std.StringHashMap(Response),

    pub fn deinit(self: *Operation, _: std.mem.Allocator) void {
        self.parameters.deinit();
        self.responses.deinit();
    }
};

pub const Parameter = struct {
    name: []const u8,
    in: ParameterLocation,
    required: bool = false,
    description: ?[]const u8 = null,
    schema_type: ?[]const u8 = null,
    schema_format: ?[]const u8 = null,
};

pub const ParameterLocation = enum {
    query,
    header,
    path,
    cookie,
};

pub const RequestBody = struct {
    description: ?[]const u8 = null,
    required: bool = false,
    /// $ref or inline schema type name (e.g. "VmCreateRequest")
    schema_ref: ?[]const u8 = null,
};

pub const Response = struct {
    description: []const u8,
    schema_ref: ?[]const u8 = null,

    pub fn deinit(_: *Response, _: std.mem.Allocator) void {}
};

pub const Components = struct {
    schemas: std.StringHashMap(Schema),

    pub fn deinit(self: *Components) void {
        self.schemas.deinit();
    }
};

pub const SchemaProperty = struct {
    name: []const u8,
    type_name: ?[]const u8 = null,
    format: ?[]const u8 = null,
    description: ?[]const u8 = null,
    ref: ?[]const u8 = null,
    required: bool = false,
    /// For array items
    items_ref: ?[]const u8 = null,
    items_type: ?[]const u8 = null,
};

/// Represents one variant of a oneOf union type.
pub const OneOfVariant = struct {
    properties: std.array_list.Managed(SchemaProperty),
    required_fields: std.array_list.Managed([]const u8),

    pub fn deinit(self: *OneOfVariant) void {
        self.properties.deinit();
        self.required_fields.deinit();
    }
};

pub const Schema = struct {
    type_name: ?[]const u8 = null,
    format: ?[]const u8 = null,
    description: ?[]const u8 = null,
    properties: std.array_list.Managed(SchemaProperty),
    required_fields: std.array_list.Managed([]const u8),
    enum_values: std.array_list.Managed([]const u8),
    /// For $ref at schema level
    ref: ?[]const u8 = null,
    /// oneOf variants (union types)
    one_of_variants: std.array_list.Managed(OneOfVariant),

    pub fn deinit(self: *Schema) void {
        self.properties.deinit();
        self.required_fields.deinit();
        self.enum_values.deinit();
        for (self.one_of_variants.items) |*v| {
            v.deinit();
        }
        self.one_of_variants.deinit();
    }
};

// ── Parsing ──────────────────────────────────────────────────────────

pub fn parseOpenAPIFile(allocator: std.mem.Allocator, file_path: []const u8) !OpenAPISpec {
    const file_content = try std.fs.cwd().readFileAlloc(allocator, file_path, 10 * 1024 * 1024);
    defer allocator.free(file_content);

    // If it looks like JSON, parse directly; otherwise try YAML-to-JSON via python
    if (file_content.len > 0 and (file_content[0] == '{' or file_content[0] == '[')) {
        return parseOpenAPIJson(allocator, file_content);
    }

    // Try to find a .json sibling
    if (std.mem.endsWith(u8, file_path, ".yaml") or std.mem.endsWith(u8, file_path, ".yml")) {
        const base = file_path[0 .. file_path.len - if (std.mem.endsWith(u8, file_path, ".yaml")) @as(usize, 5) else @as(usize, 4)];
        const json_path = try std.fmt.allocPrint(allocator, "{s}.json", .{base});
        defer allocator.free(json_path);

        if (std.fs.cwd().readFileAlloc(allocator, json_path, 10 * 1024 * 1024)) |json_content| {
            defer allocator.free(json_content);
            return parseOpenAPIJson(allocator, json_content);
        } else |_| {}
    }

    // Fall back to python yaml→json conversion
    return parseOpenAPIYamlViaPython(allocator, file_path);
}

fn parseOpenAPIYamlViaPython(allocator: std.mem.Allocator, file_path: []const u8) !OpenAPISpec {
    const py_script = try std.fmt.allocPrint(allocator,
        "import sys, yaml, json; json.dump(yaml.safe_load(open('{s}')), sys.stdout)",
        .{file_path},
    );
    defer allocator.free(py_script);

    var child = std.process.Child.init(
        &[_][]const u8{ "python3", "-c", py_script },
        allocator,
    );
    child.stdout_behavior = .Pipe;
    child.stderr_behavior = .Ignore;

    try child.spawn();

    // Read all stdout
    const stdout_file = child.stdout.?;
    const stdout = try stdout_file.readToEndAlloc(allocator, 10 * 1024 * 1024);
    defer allocator.free(stdout);

    const term = try child.wait();
    if (term.Exited != 0 or stdout.len == 0) {
        return error.YamlConversionFailed;
    }

    return parseOpenAPIJson(allocator, stdout);
}

pub fn parseOpenAPIJson(allocator: std.mem.Allocator, json_content: []const u8) !OpenAPISpec {
    const parsed = try std.json.parseFromSlice(std.json.Value, allocator, json_content, .{});
    defer parsed.deinit();

    const root = parsed.value;
    if (root != .object) return error.InvalidOpenAPISpec;

    var spec = OpenAPISpec{
        .allocator = allocator,
        .openapi = "3.0.0",
        .info = Info{
            .title = "API",
            .version = "1.0.0",
        },
        .paths = std.StringHashMap(PathItem).init(allocator),
    };

    // Parse info
    if (root.object.get("info")) |info_val| {
        if (info_val == .object) {
            if (info_val.object.get("title")) |t| {
                if (t == .string) spec.info.title = try allocator.dupe(u8, t.string);
            }
            if (info_val.object.get("version")) |v| {
                if (v == .string) spec.info.version = try allocator.dupe(u8, v.string);
            }
            if (info_val.object.get("description")) |d| {
                if (d == .string) spec.info.description = try allocator.dupe(u8, d.string);
            }
        }
    }

    // Parse openapi version
    if (root.object.get("openapi")) |oa| {
        if (oa == .string) spec.openapi = try allocator.dupe(u8, oa.string);
    }

    // Parse servers
    if (root.object.get("servers")) |servers_val| {
        if (servers_val == .array) {
            var urls: std.ArrayListUnmanaged([]const u8) = .{};
            for (servers_val.array.items) |server| {
                if (server == .object) {
                    if (server.object.get("url")) |u| {
                        if (u == .string) try urls.append(allocator, try allocator.dupe(u8, u.string));
                    }
                }
            }
            spec.servers = .{ .items = try urls.toOwnedSlice(allocator) };
        }
    }

    // Parse paths
    if (root.object.get("paths")) |paths_val| {
        if (paths_val == .object) {
            var path_iter = paths_val.object.iterator();
            while (path_iter.next()) |entry| {
                const path_str = try allocator.dupe(u8, entry.key_ptr.*);
                var path_item = PathItem{};

                if (entry.value_ptr.* == .object) {
                    const methods = [_]struct { name: []const u8, setter: *const fn (*PathItem, Operation) void }{
                        .{ .name = "get", .setter = &setGet },
                        .{ .name = "post", .setter = &setPost },
                        .{ .name = "put", .setter = &setPut },
                        .{ .name = "delete", .setter = &setDelete },
                        .{ .name = "patch", .setter = &setPatch },
                    };

                    for (methods) |m| {
                        if (entry.value_ptr.object.get(m.name)) |op_val| {
                            if (op_val == .object) {
                                const op = try parseOperation(allocator, op_val.object);
                                m.setter(&path_item, op);
                            }
                        }
                    }
                }

                try spec.paths.put(path_str, path_item);
            }
        }
    }

    // Parse components/schemas
    if (root.object.get("components")) |comp_val| {
        if (comp_val == .object) {
            if (comp_val.object.get("schemas")) |schemas_val| {
                if (schemas_val == .object) {
                    var schemas = std.StringHashMap(Schema).init(allocator);
                    var schema_iter = schemas_val.object.iterator();
                    while (schema_iter.next()) |entry| {
                        const name = try allocator.dupe(u8, entry.key_ptr.*);
                        if (entry.value_ptr.* == .object) {
                            const schema = try parseSchema(allocator, entry.value_ptr.object);
                            try schemas.put(name, schema);
                        }
                    }
                    spec.components = Components{ .schemas = schemas };
                }
            }
        }
    }

    return spec;
}

fn parseOperation(allocator: std.mem.Allocator, obj: std.json.ObjectMap) !Operation {
    var op = Operation{
        .parameters = std.array_list.Managed(Parameter).init(allocator),
        .responses = std.StringHashMap(Response).init(allocator),
    };

    if (obj.get("operationId")) |v| {
        if (v == .string) op.operationId = try allocator.dupe(u8, v.string);
    }
    if (obj.get("summary")) |v| {
        if (v == .string) op.summary = try allocator.dupe(u8, v.string);
    }
    if (obj.get("description")) |v| {
        if (v == .string) op.description = try allocator.dupe(u8, v.string);
    }

    // Parse parameters
    if (obj.get("parameters")) |params_val| {
        if (params_val == .array) {
            for (params_val.array.items) |param_val| {
                if (param_val == .object) {
                    const param = try parseParameter(allocator, param_val.object);
                    try op.parameters.append(param);
                }
            }
        }
    }

    // Parse requestBody
    if (obj.get("requestBody")) |rb_val| {
        if (rb_val == .object) {
            op.requestBody = try parseRequestBody(allocator, rb_val.object);
        }
    }

    // Parse responses
    if (obj.get("responses")) |resp_val| {
        if (resp_val == .object) {
            var resp_iter = resp_val.object.iterator();
            while (resp_iter.next()) |entry| {
                const code = try allocator.dupe(u8, entry.key_ptr.*);
                if (entry.value_ptr.* == .object) {
                    const resp = try parseResponse(allocator, entry.value_ptr.object);
                    try op.responses.put(code, resp);
                }
            }
        }
    }

    return op;
}

fn parseParameter(allocator: std.mem.Allocator, obj: std.json.ObjectMap) !Parameter {
    var param = Parameter{
        .name = "",
        .in = .query,
    };

    if (obj.get("name")) |v| {
        if (v == .string) param.name = try allocator.dupe(u8, v.string);
    }
    if (obj.get("in")) |v| {
        if (v == .string) {
            if (std.mem.eql(u8, v.string, "path")) param.in = .path
            else if (std.mem.eql(u8, v.string, "query")) param.in = .query
            else if (std.mem.eql(u8, v.string, "header")) param.in = .header
            else if (std.mem.eql(u8, v.string, "cookie")) param.in = .cookie;
        }
    }
    if (obj.get("required")) |v| {
        if (v == .bool) param.required = v.bool;
    }
    if (obj.get("description")) |v| {
        if (v == .string) param.description = try allocator.dupe(u8, v.string);
    }
    if (obj.get("schema")) |schema_val| {
        if (schema_val == .object) {
            if (schema_val.object.get("type")) |t| {
                if (t == .string) param.schema_type = try allocator.dupe(u8, t.string);
            }
            if (schema_val.object.get("format")) |f| {
                if (f == .string) param.schema_format = try allocator.dupe(u8, f.string);
            }
        }
    }

    return param;
}

fn parseRequestBody(allocator: std.mem.Allocator, obj: std.json.ObjectMap) !RequestBody {
    var rb = RequestBody{};

    if (obj.get("description")) |v| {
        if (v == .string) rb.description = try allocator.dupe(u8, v.string);
    }
    if (obj.get("required")) |v| {
        if (v == .bool) rb.required = v.bool;
    }
    // Extract schema $ref from content.application/json.schema.$ref
    if (obj.get("content")) |content| {
        if (content == .object) {
            if (content.object.get("application/json")) |json_ct| {
                if (json_ct == .object) {
                    if (json_ct.object.get("schema")) |schema| {
                        if (schema == .object) {
                            if (schema.object.get("$ref")) |ref| {
                                if (ref == .string) {
                                    rb.schema_ref = try allocator.dupe(u8, extractRefName(ref.string));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    return rb;
}

fn parseResponse(allocator: std.mem.Allocator, obj: std.json.ObjectMap) !Response {
    var resp = Response{ .description = "" };

    if (obj.get("description")) |v| {
        if (v == .string) resp.description = try allocator.dupe(u8, v.string);
    }
    // Extract schema $ref from content.application/json.schema.$ref
    if (obj.get("content")) |content| {
        if (content == .object) {
            if (content.object.get("application/json")) |json_ct| {
                if (json_ct == .object) {
                    if (json_ct.object.get("schema")) |schema| {
                        if (schema == .object) {
                            if (schema.object.get("$ref")) |ref| {
                                if (ref == .string) {
                                    resp.schema_ref = try allocator.dupe(u8, extractRefName(ref.string));
                                }
                            }
                        }
                    }
                }
            }
        }
    }

    return resp;
}

fn parseSchema(allocator: std.mem.Allocator, obj: std.json.ObjectMap) !Schema {
    var schema = Schema{
        .properties = std.array_list.Managed(SchemaProperty).init(allocator),
        .required_fields = std.array_list.Managed([]const u8).init(allocator),
        .enum_values = std.array_list.Managed([]const u8).init(allocator),
        .one_of_variants = std.array_list.Managed(OneOfVariant).init(allocator),
    };

    if (obj.get("type")) |v| {
        if (v == .string) schema.type_name = try allocator.dupe(u8, v.string);
    }
    if (obj.get("format")) |v| {
        if (v == .string) schema.format = try allocator.dupe(u8, v.string);
    }
    if (obj.get("description")) |v| {
        if (v == .string) schema.description = try allocator.dupe(u8, v.string);
    }
    if (obj.get("$ref")) |v| {
        if (v == .string) schema.ref = try allocator.dupe(u8, extractRefName(v.string));
    }

    // Parse required fields
    if (obj.get("required")) |req_val| {
        if (req_val == .array) {
            for (req_val.array.items) |item| {
                if (item == .string) {
                    try schema.required_fields.append(try allocator.dupe(u8, item.string));
                }
            }
        }
    }

    // Parse enum values
    if (obj.get("enum")) |enum_val| {
        if (enum_val == .array) {
            for (enum_val.array.items) |item| {
                if (item == .string) {
                    try schema.enum_values.append(try allocator.dupe(u8, item.string));
                }
            }
        }
    }

    // Parse oneOf variants
    if (obj.get("oneOf")) |one_of_val| {
        if (one_of_val == .array) {
            for (one_of_val.array.items) |variant_val| {
                if (variant_val == .object) {
                    var variant = OneOfVariant{
                        .properties = std.array_list.Managed(SchemaProperty).init(allocator),
                        .required_fields = std.array_list.Managed([]const u8).init(allocator),
                    };
                    // Parse required fields for variant
                    if (variant_val.object.get("required")) |req_val| {
                        if (req_val == .array) {
                            for (req_val.array.items) |item| {
                                if (item == .string) {
                                    try variant.required_fields.append(try allocator.dupe(u8, item.string));
                                }
                            }
                        }
                    }
                    // Parse properties for variant
                    if (variant_val.object.get("properties")) |vprops| {
                        if (vprops == .object) {
                            var vprop_iter = vprops.object.iterator();
                            while (vprop_iter.next()) |ventry| {
                                var vprop = SchemaProperty{
                                    .name = try allocator.dupe(u8, ventry.key_ptr.*),
                                };
                                // Check if required
                                for (variant.required_fields.items) |req_name| {
                                    if (std.mem.eql(u8, req_name, ventry.key_ptr.*)) {
                                        vprop.required = true;
                                        break;
                                    }
                                }
                                if (ventry.value_ptr.* == .object) {
                                    const vpobj = ventry.value_ptr.object;
                                    if (vpobj.get("type")) |t| {
                                        if (t == .string) vprop.type_name = try allocator.dupe(u8, t.string);
                                    }
                                    if (vpobj.get("format")) |f| {
                                        if (f == .string) vprop.format = try allocator.dupe(u8, f.string);
                                    }
                                    if (vpobj.get("description")) |d| {
                                        if (d == .string) vprop.description = try allocator.dupe(u8, d.string);
                                    }
                                    if (vpobj.get("$ref")) |r| {
                                        if (r == .string) vprop.ref = try allocator.dupe(u8, extractRefName(r.string));
                                    }
                                }
                                try variant.properties.append(vprop);
                            }
                        }
                    }
                    try schema.one_of_variants.append(variant);
                }
            }
        }
    }

    // Parse properties
    if (obj.get("properties")) |props_val| {
        if (props_val == .object) {
            var prop_iter = props_val.object.iterator();
            while (prop_iter.next()) |entry| {
                var prop = SchemaProperty{
                    .name = try allocator.dupe(u8, entry.key_ptr.*),
                };

                // Check if this field is required
                for (schema.required_fields.items) |req_name| {
                    if (std.mem.eql(u8, req_name, entry.key_ptr.*)) {
                        prop.required = true;
                        break;
                    }
                }

                if (entry.value_ptr.* == .object) {
                    const pobj = entry.value_ptr.object;
                    if (pobj.get("type")) |t| {
                        if (t == .string) prop.type_name = try allocator.dupe(u8, t.string);
                    }
                    if (pobj.get("format")) |f| {
                        if (f == .string) prop.format = try allocator.dupe(u8, f.string);
                    }
                    if (pobj.get("description")) |d| {
                        if (d == .string) prop.description = try allocator.dupe(u8, d.string);
                    }
                    if (pobj.get("$ref")) |r| {
                        if (r == .string) prop.ref = try allocator.dupe(u8, extractRefName(r.string));
                    }
                    // Array items
                    if (pobj.get("items")) |items| {
                        if (items == .object) {
                            if (items.object.get("$ref")) |r| {
                                if (r == .string) prop.items_ref = try allocator.dupe(u8, extractRefName(r.string));
                            }
                            if (items.object.get("type")) |t| {
                                if (t == .string) prop.items_type = try allocator.dupe(u8, t.string);
                            }
                        }
                    }
                }

                try schema.properties.append(prop);
            }
        }
    }

    return schema;
}

/// Extract the type name from a $ref string like "#/components/schemas/VmCreateRequest"
fn extractRefName(ref: []const u8) []const u8 {
    if (std.mem.lastIndexOfScalar(u8, ref, '/')) |idx| {
        return ref[idx + 1 ..];
    }
    return ref;
}

// ── PathItem setters (needed for function pointer array) ─────────────

fn setGet(pi: *PathItem, op: Operation) void { pi.get = op; }
fn setPost(pi: *PathItem, op: Operation) void { pi.post = op; }
fn setPut(pi: *PathItem, op: Operation) void { pi.put = op; }
fn setDelete(pi: *PathItem, op: Operation) void { pi.delete = op; }
fn setPatch(pi: *PathItem, op: Operation) void { pi.patch = op; }
