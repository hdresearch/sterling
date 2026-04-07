const std = @import("std");
const log = std.log.scoped(.webhook);

/// Represents a parsed GitHub webhook event
pub const WebhookEvent = struct {
    event_type: EventType,
    repo_name: []const u8,
    repo_owner: []const u8,
    ref: []const u8,
    changed_files: []const ChangedFile,
    sender: []const u8,

    pub const EventType = enum {
        push,
        pull_request,
        ping,
        unknown,
    };

    pub const ChangedFile = struct {
        filename: []const u8,
        status: FileStatus,

        pub const FileStatus = enum {
            added,
            modified,
            removed,
            renamed,
            unknown,
        };
    };
};

/// GitHub webhook handler for OpenAPI change detection
pub const WebhookHandler = struct {
    allocator: std.mem.Allocator,
    config: Config,

    pub const Config = struct {
        /// GitHub webhook secret for HMAC validation
        secret: ?[]const u8 = null,
        /// Target repository to watch (e.g., "hdresearch/chelsea")
        target_repo: []const u8 = "hdresearch/chelsea",
        /// File patterns that trigger pipeline (checked with contains)
        openapi_patterns: []const []const u8 = &default_patterns,
        /// Branch to watch
        target_branch: []const u8 = "refs/heads/main",
    };

    const default_patterns = [_][]const u8{
        "openapi.yaml",
        "openapi.json",
        "openapi.yml",
        "swagger.yaml",
        "swagger.json",
        "api-spec",
    };

    pub fn init(allocator: std.mem.Allocator) WebhookHandler {
        return initWithConfig(allocator, .{});
    }

    pub fn initWithConfig(allocator: std.mem.Allocator, config: Config) WebhookHandler {
        return .{
            .allocator = allocator,
            .config = config,
        };
    }

    /// Result of handling a webhook
    pub const HandleResult = struct {
        event: WebhookEvent,
        should_trigger_pipeline: bool,
        openapi_files: []const []const u8,
    };

    /// Parse and handle a GitHub webhook payload.
    /// Returns the parsed event and whether it should trigger the pipeline.
    pub fn handleWebhook(self: *WebhookHandler, payload: []const u8) !HandleResult {
        if (payload.len == 0) return error.EmptyPayload;

        const event = try self.parsePayload(payload);

        // Determine if this event should trigger the pipeline
        const openapi_files = try self.detectOpenAPIChanges(event);
        const should_trigger = openapi_files.len > 0 and self.isTargetRepo(event) and self.isTargetBranch(event);

        if (should_trigger) {
            log.info("OpenAPI changes detected in {s}/{s} on {s}: {d} file(s)", .{
                event.repo_owner, event.repo_name, event.ref, openapi_files.len,
            });
        }

        return .{
            .event = event,
            .should_trigger_pipeline = should_trigger,
            .openapi_files = openapi_files,
        };
    }

    /// Parse a raw JSON payload into a WebhookEvent
    pub fn parsePayload(self: *WebhookHandler, payload: []const u8) !WebhookEvent {
        const parsed = std.json.parseFromSlice(std.json.Value, self.allocator, payload, .{}) catch {
            return error.InvalidJSON;
        };
        defer parsed.deinit();
        const root = parsed.value;

        if (root != .object) return error.InvalidPayload;

        // Detect event type
        const event_type = blk: {
            if (root.object.get("zen")) |_| break :blk WebhookEvent.EventType.ping;
            if (root.object.get("pull_request")) |_| break :blk WebhookEvent.EventType.pull_request;
            if (root.object.get("commits")) |_| break :blk WebhookEvent.EventType.push;
            break :blk WebhookEvent.EventType.unknown;
        };

        // Extract repository info
        const repo = root.object.get("repository") orelse return error.MissingRepository;
        if (repo != .object) return error.InvalidPayload;

        const repo_name = blk: {
            const name_val = repo.object.get("name") orelse break :blk "unknown";
            break :blk switch (name_val) {
                .string => |s| s,
                else => "unknown",
            };
        };

        const repo_owner = blk: {
            const owner_val = repo.object.get("owner") orelse break :blk "unknown";
            if (owner_val != .object) break :blk "unknown";
            const login_val = owner_val.object.get("login") orelse break :blk "unknown";
            break :blk switch (login_val) {
                .string => |s| s,
                else => "unknown",
            };
        };

        // Extract ref
        const ref = blk: {
            const ref_val = root.object.get("ref") orelse break :blk "";
            break :blk switch (ref_val) {
                .string => |s| s,
                else => "",
            };
        };

        // Extract sender
        const sender = blk: {
            const sender_val = root.object.get("sender") orelse break :blk "unknown";
            if (sender_val != .object) break :blk "unknown";
            const login_val = sender_val.object.get("login") orelse break :blk "unknown";
            break :blk switch (login_val) {
                .string => |s| s,
                else => "unknown",
            };
        };

        // Extract changed files from commits
        const changed_files = try self.extractChangedFiles(root);

        // We need to dupe all strings since parsed will be freed
        const duped_repo_name = try self.allocator.dupe(u8, repo_name);
        const duped_repo_owner = try self.allocator.dupe(u8, repo_owner);
        const duped_ref = try self.allocator.dupe(u8, ref);
        const duped_sender = try self.allocator.dupe(u8, sender);

        return WebhookEvent{
            .event_type = event_type,
            .repo_name = duped_repo_name,
            .repo_owner = duped_repo_owner,
            .ref = duped_ref,
            .changed_files = changed_files,
            .sender = duped_sender,
        };
    }

    fn extractChangedFiles(self: *WebhookHandler, root: std.json.Value) ![]const WebhookEvent.ChangedFile {
        var files = std.array_list.Managed(WebhookEvent.ChangedFile).init(self.allocator);

        // For push events, files are in commits[].added/modified/removed
        if (root.object.get("commits")) |commits_val| {
            if (commits_val == .array) {
                for (commits_val.array.items) |commit| {
                    if (commit != .object) continue;
                    try self.extractFilesFromCommit(commit, &files);
                }
            }
        }

        return files.toOwnedSlice();
    }

    fn extractFilesFromCommit(
        self: *WebhookHandler,
        commit: std.json.Value,
        files: *std.array_list.Managed(WebhookEvent.ChangedFile),
    ) !void {
        const categories = [_]struct { key: []const u8, status: WebhookEvent.ChangedFile.FileStatus }{
            .{ .key = "added", .status = .added },
            .{ .key = "modified", .status = .modified },
            .{ .key = "removed", .status = .removed },
        };

        for (categories) |cat| {
            if (commit.object.get(cat.key)) |arr| {
                if (arr == .array) {
                    for (arr.array.items) |item| {
                        if (item == .string) {
                            try files.append(.{
                                .filename = try self.allocator.dupe(u8, item.string),
                                .status = cat.status,
                            });
                        }
                    }
                }
            }
        }
    }

    /// Detect OpenAPI spec files among changed files
    fn detectOpenAPIChanges(self: *WebhookHandler, event: WebhookEvent) ![]const []const u8 {
        var openapi_files = std.array_list.Managed([]const u8).init(self.allocator);

        for (event.changed_files) |file| {
            if (self.isOpenAPIFile(file.filename)) {
                try openapi_files.append(file.filename);
            }
        }

        return openapi_files.toOwnedSlice();
    }

    /// Check if a filename matches an OpenAPI spec pattern
    pub fn isOpenAPIFile(self: *WebhookHandler, filename: []const u8) bool {
        for (self.config.openapi_patterns) |pattern| {
            if (std.mem.indexOf(u8, filename, pattern) != null) return true;
        }
        return false;
    }

    fn isTargetRepo(self: *WebhookHandler, event: WebhookEvent) bool {
        // Build "owner/name" and compare
        const full_name = std.fmt.allocPrint(self.allocator, "{s}/{s}", .{ event.repo_owner, event.repo_name }) catch return false;
        defer self.allocator.free(full_name);
        return std.mem.eql(u8, full_name, self.config.target_repo);
    }

    fn isTargetBranch(self: *WebhookHandler, event: WebhookEvent) bool {
        if (event.ref.len == 0) return true; // No ref means not a push, don't filter
        return std.mem.eql(u8, event.ref, self.config.target_branch);
    }

    /// Validate HMAC-SHA256 signature from GitHub
    pub fn validateSignature(self: *WebhookHandler, payload: []const u8, signature_header: []const u8) bool {
        const secret = self.config.secret orelse return true; // No secret configured = skip validation

        // signature_header format: "sha256=<hex>"
        const prefix = "sha256=";
        if (!std.mem.startsWith(u8, signature_header, prefix)) return false;
        const hex_sig = signature_header[prefix.len..];

        var mac = std.crypto.auth.hmac.sha2.HmacSha256.init(secret.*);
        mac.update(payload);
        var computed: [32]u8 = undefined;
        mac.final(&computed);

        // Compare hex
        var expected_hex: [64]u8 = undefined;
        _ = std.fmt.bufPrint(&expected_hex, "{}", .{std.fmt.fmtSliceHexLower(&computed)}) catch return false;

        if (hex_sig.len != 64) return false;
        return std.crypto.utils.timingSafeEql([64]u8, expected_hex, hex_sig[0..64].*);
    }
};

// Tests
test "WebhookHandler init" {
    const handler = WebhookHandler.init(std.testing.allocator);
    _ = handler;
}

test "isOpenAPIFile detection" {
    var handler = WebhookHandler.init(std.testing.allocator);
    try std.testing.expect(handler.isOpenAPIFile("openapi.yaml"));
    try std.testing.expect(handler.isOpenAPIFile("specs/openapi.json"));
    try std.testing.expect(handler.isOpenAPIFile("api/swagger.yaml"));
    try std.testing.expect(!handler.isOpenAPIFile("README.md"));
    try std.testing.expect(!handler.isOpenAPIFile("src/main.zig"));
}

test "parsePayload push event" {
    var handler = WebhookHandler.init(std.testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/main",
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "testuser" },
        \\  "commits": [
        \\    {
        \\      "added": [],
        \\      "modified": ["openapi.yaml"],
        \\      "removed": []
        \\    }
        \\  ]
        \\}
    ;

    const event = try handler.parsePayload(payload);
    defer {
        std.testing.allocator.free(event.repo_name);
        std.testing.allocator.free(event.repo_owner);
        std.testing.allocator.free(event.ref);
        std.testing.allocator.free(event.sender);
        for (event.changed_files) |f| std.testing.allocator.free(f.filename);
        std.testing.allocator.free(event.changed_files);
    }

    try std.testing.expectEqual(WebhookEvent.EventType.push, event.event_type);
    try std.testing.expectEqualStrings("chelsea", event.repo_name);
    try std.testing.expectEqualStrings("hdresearch", event.repo_owner);
    try std.testing.expectEqualStrings("refs/heads/main", event.ref);
    try std.testing.expectEqual(@as(usize, 1), event.changed_files.len);
    try std.testing.expectEqualStrings("openapi.yaml", event.changed_files[0].filename);
}

test "handleWebhook triggers pipeline for OpenAPI changes" {
    var handler = WebhookHandler.init(std.testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/main",
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "testuser" },
        \\  "commits": [
        \\    {
        \\      "added": ["openapi.yaml"],
        \\      "modified": [],
        \\      "removed": []
        \\    }
        \\  ]
        \\}
    ;

    const result = try handler.handleWebhook(payload);
    defer {
        std.testing.allocator.free(result.event.repo_name);
        std.testing.allocator.free(result.event.repo_owner);
        std.testing.allocator.free(result.event.ref);
        std.testing.allocator.free(result.event.sender);
        for (result.event.changed_files) |f| std.testing.allocator.free(f.filename);
        std.testing.allocator.free(result.event.changed_files);
        std.testing.allocator.free(result.openapi_files);
    }

    try std.testing.expect(result.should_trigger_pipeline);
    try std.testing.expectEqual(@as(usize, 1), result.openapi_files.len);
}

test "handleWebhook ignores non-OpenAPI changes" {
    var handler = WebhookHandler.init(std.testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/main",
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "testuser" },
        \\  "commits": [
        \\    {
        \\      "added": [],
        \\      "modified": ["README.md", "src/main.rs"],
        \\      "removed": []
        \\    }
        \\  ]
        \\}
    ;

    const result = try handler.handleWebhook(payload);
    defer {
        std.testing.allocator.free(result.event.repo_name);
        std.testing.allocator.free(result.event.repo_owner);
        std.testing.allocator.free(result.event.ref);
        std.testing.allocator.free(result.event.sender);
        for (result.event.changed_files) |f| std.testing.allocator.free(f.filename);
        std.testing.allocator.free(result.event.changed_files);
        std.testing.allocator.free(result.openapi_files);
    }

    try std.testing.expect(!result.should_trigger_pipeline);
}

test "handleWebhook ignores wrong repo" {
    var handler = WebhookHandler.init(std.testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/main",
        \\  "repository": {
        \\    "name": "other-repo",
        \\    "owner": { "login": "someone" }
        \\  },
        \\  "sender": { "login": "testuser" },
        \\  "commits": [
        \\    {
        \\      "added": ["openapi.yaml"],
        \\      "modified": [],
        \\      "removed": []
        \\    }
        \\  ]
        \\}
    ;

    const result = try handler.handleWebhook(payload);
    defer {
        std.testing.allocator.free(result.event.repo_name);
        std.testing.allocator.free(result.event.repo_owner);
        std.testing.allocator.free(result.event.ref);
        std.testing.allocator.free(result.event.sender);
        for (result.event.changed_files) |f| std.testing.allocator.free(f.filename);
        std.testing.allocator.free(result.event.changed_files);
        std.testing.allocator.free(result.openapi_files);
    }

    try std.testing.expect(!result.should_trigger_pipeline);
}

test "handleWebhook ignores wrong branch" {
    var handler = WebhookHandler.init(std.testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/feature-branch",
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "testuser" },
        \\  "commits": [
        \\    {
        \\      "added": ["openapi.yaml"],
        \\      "modified": [],
        \\      "removed": []
        \\    }
        \\  ]
        \\}
    ;

    const result = try handler.handleWebhook(payload);
    defer {
        std.testing.allocator.free(result.event.repo_name);
        std.testing.allocator.free(result.event.repo_owner);
        std.testing.allocator.free(result.event.ref);
        std.testing.allocator.free(result.event.sender);
        for (result.event.changed_files) |f| std.testing.allocator.free(f.filename);
        std.testing.allocator.free(result.event.changed_files);
        std.testing.allocator.free(result.openapi_files);
    }

    try std.testing.expect(!result.should_trigger_pipeline);
}

test "handleWebhook rejects empty payload" {
    var handler = WebhookHandler.init(std.testing.allocator);
    const result = handler.handleWebhook("");
    try std.testing.expectError(error.EmptyPayload, result);
}

test "handleWebhook rejects invalid JSON" {
    var handler = WebhookHandler.init(std.testing.allocator);
    const result = handler.handleWebhook("not json at all");
    try std.testing.expectError(error.InvalidJSON, result);
}

test "parsePayload ping event" {
    var handler = WebhookHandler.init(std.testing.allocator);
    const payload =
        \\{
        \\  "zen": "Keep it logically awesome.",
        \\  "hook_id": 12345,
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "testuser" }
        \\}
    ;

    const event = try handler.parsePayload(payload);
    defer {
        std.testing.allocator.free(event.repo_name);
        std.testing.allocator.free(event.repo_owner);
        std.testing.allocator.free(event.ref);
        std.testing.allocator.free(event.sender);
        std.testing.allocator.free(event.changed_files);
    }

    try std.testing.expectEqual(WebhookEvent.EventType.ping, event.event_type);
}

test "parsePayload multiple commits with multiple files" {
    var handler = WebhookHandler.init(std.testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/main",
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "testuser" },
        \\  "commits": [
        \\    {
        \\      "added": ["openapi.yaml"],
        \\      "modified": ["README.md"],
        \\      "removed": []
        \\    },
        \\    {
        \\      "added": [],
        \\      "modified": ["specs/swagger.json"],
        \\      "removed": ["old-spec.yaml"]
        \\    }
        \\  ]
        \\}
    ;

    const event = try handler.parsePayload(payload);
    defer {
        std.testing.allocator.free(event.repo_name);
        std.testing.allocator.free(event.repo_owner);
        std.testing.allocator.free(event.ref);
        std.testing.allocator.free(event.sender);
        for (event.changed_files) |f| std.testing.allocator.free(f.filename);
        std.testing.allocator.free(event.changed_files);
    }

    try std.testing.expectEqual(@as(usize, 4), event.changed_files.len);
}
