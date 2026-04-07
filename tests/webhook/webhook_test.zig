const std = @import("std");
const testing = std.testing;
const webhook = @import("webhook");

test "WebhookHandler initialization" {
    const handler = webhook.WebhookHandler.init(testing.allocator);
    try testing.expectEqualStrings("hdresearch/chelsea", handler.config.target_repo);
    try testing.expectEqualStrings("refs/heads/main", handler.config.target_branch);
}

test "WebhookHandler custom config" {
    const handler = webhook.WebhookHandler.initWithConfig(testing.allocator, .{
        .target_repo = "myorg/myrepo",
        .target_branch = "refs/heads/develop",
    });
    try testing.expectEqualStrings("myorg/myrepo", handler.config.target_repo);
    try testing.expectEqualStrings("refs/heads/develop", handler.config.target_branch);
}

test "OpenAPI file detection" {
    var handler = webhook.WebhookHandler.init(testing.allocator);
    try testing.expect(handler.isOpenAPIFile("openapi.yaml"));
    try testing.expect(handler.isOpenAPIFile("openapi.json"));
    try testing.expect(handler.isOpenAPIFile("openapi.yml"));
    try testing.expect(handler.isOpenAPIFile("specs/openapi.yaml"));
    try testing.expect(handler.isOpenAPIFile("api/swagger.json"));
    try testing.expect(handler.isOpenAPIFile("api-spec/v2/spec.json"));
    try testing.expect(!handler.isOpenAPIFile("README.md"));
    try testing.expect(!handler.isOpenAPIFile("package.json"));
    try testing.expect(!handler.isOpenAPIFile("src/handler.zig"));
}

test "Parse push event payload" {
    var handler = webhook.WebhookHandler.init(testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/main",
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "developer" },
        \\  "commits": [
        \\    {
        \\      "added": ["openapi.yaml"],
        \\      "modified": ["README.md"],
        \\      "removed": []
        \\    }
        \\  ]
        \\}
    ;

    const event = try handler.parsePayload(payload);
    defer {
        testing.allocator.free(event.repo_name);
        testing.allocator.free(event.repo_owner);
        testing.allocator.free(event.ref);
        testing.allocator.free(event.sender);
        for (event.changed_files) |f| testing.allocator.free(f.filename);
        testing.allocator.free(event.changed_files);
    }

    try testing.expectEqual(webhook.WebhookEvent.EventType.push, event.event_type);
    try testing.expectEqualStrings("chelsea", event.repo_name);
    try testing.expectEqualStrings("hdresearch", event.repo_owner);
    try testing.expectEqualStrings("refs/heads/main", event.ref);
    try testing.expectEqualStrings("developer", event.sender);
    try testing.expectEqual(@as(usize, 2), event.changed_files.len);
}

test "Parse ping event" {
    var handler = webhook.WebhookHandler.init(testing.allocator);
    const payload =
        \\{
        \\  "zen": "Approachable is better than simple.",
        \\  "hook_id": 423456789,
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "admin" }
        \\}
    ;

    const event = try handler.parsePayload(payload);
    defer {
        testing.allocator.free(event.repo_name);
        testing.allocator.free(event.repo_owner);
        testing.allocator.free(event.ref);
        testing.allocator.free(event.sender);
        testing.allocator.free(event.changed_files);
    }

    try testing.expectEqual(webhook.WebhookEvent.EventType.ping, event.event_type);
}

test "Handle webhook triggers pipeline for OpenAPI changes" {
    var handler = webhook.WebhookHandler.init(testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/main",
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "developer" },
        \\  "commits": [
        \\    {
        \\      "added": [],
        \\      "modified": ["specs/openapi.yaml"],
        \\      "removed": []
        \\    }
        \\  ]
        \\}
    ;

    const result = try handler.handleWebhook(payload);
    defer {
        testing.allocator.free(result.event.repo_name);
        testing.allocator.free(result.event.repo_owner);
        testing.allocator.free(result.event.ref);
        testing.allocator.free(result.event.sender);
        for (result.event.changed_files) |f| testing.allocator.free(f.filename);
        testing.allocator.free(result.event.changed_files);
        testing.allocator.free(result.openapi_files);
    }

    try testing.expect(result.should_trigger_pipeline);
    try testing.expectEqual(@as(usize, 1), result.openapi_files.len);
}

test "Handle webhook does not trigger for non-OpenAPI changes" {
    var handler = webhook.WebhookHandler.init(testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/main",
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "developer" },
        \\  "commits": [
        \\    {
        \\      "added": ["src/lib.rs"],
        \\      "modified": ["Cargo.toml"],
        \\      "removed": []
        \\    }
        \\  ]
        \\}
    ;

    const result = try handler.handleWebhook(payload);
    defer {
        testing.allocator.free(result.event.repo_name);
        testing.allocator.free(result.event.repo_owner);
        testing.allocator.free(result.event.ref);
        testing.allocator.free(result.event.sender);
        for (result.event.changed_files) |f| testing.allocator.free(f.filename);
        testing.allocator.free(result.event.changed_files);
        testing.allocator.free(result.openapi_files);
    }

    try testing.expect(!result.should_trigger_pipeline);
}

test "Handle webhook rejects empty payload" {
    var handler = webhook.WebhookHandler.init(testing.allocator);
    try testing.expectError(error.EmptyPayload, handler.handleWebhook(""));
}

test "Handle webhook rejects invalid JSON" {
    var handler = webhook.WebhookHandler.init(testing.allocator);
    try testing.expectError(error.InvalidJSON, handler.handleWebhook("{invalid}"));
}

test "Handle webhook filters by repository" {
    var handler = webhook.WebhookHandler.init(testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/main",
        \\  "repository": {
        \\    "name": "unrelated-repo",
        \\    "owner": { "login": "other-org" }
        \\  },
        \\  "sender": { "login": "developer" },
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
        testing.allocator.free(result.event.repo_name);
        testing.allocator.free(result.event.repo_owner);
        testing.allocator.free(result.event.ref);
        testing.allocator.free(result.event.sender);
        for (result.event.changed_files) |f| testing.allocator.free(f.filename);
        testing.allocator.free(result.event.changed_files);
        testing.allocator.free(result.openapi_files);
    }

    try testing.expect(!result.should_trigger_pipeline);
}

test "Handle webhook filters by branch" {
    var handler = webhook.WebhookHandler.init(testing.allocator);
    const payload =
        \\{
        \\  "ref": "refs/heads/feature-xyz",
        \\  "repository": {
        \\    "name": "chelsea",
        \\    "owner": { "login": "hdresearch" }
        \\  },
        \\  "sender": { "login": "developer" },
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
        testing.allocator.free(result.event.repo_name);
        testing.allocator.free(result.event.repo_owner);
        testing.allocator.free(result.event.ref);
        testing.allocator.free(result.event.sender);
        for (result.event.changed_files) |f| testing.allocator.free(f.filename);
        testing.allocator.free(result.event.changed_files);
        testing.allocator.free(result.openapi_files);
    }

    try testing.expect(!result.should_trigger_pipeline);
}
