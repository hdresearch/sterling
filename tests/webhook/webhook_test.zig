const std = @import("std");
const webhook = @import("webhook");

test "WebhookHandler init" {
    const handler = webhook.WebhookHandler.init(std.testing.allocator);
    _ = handler;
}

test "isOpenAPIFile detection" {
    var handler = webhook.WebhookHandler.init(std.testing.allocator);
    try std.testing.expect(handler.isOpenAPIFile("openapi.yaml"));
    try std.testing.expect(handler.isOpenAPIFile("specs/openapi.json"));
    try std.testing.expect(!handler.isOpenAPIFile("README.md"));
}

test "parsePayload push event" {
    var handler = webhook.WebhookHandler.init(std.testing.allocator);
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

    try std.testing.expectEqual(webhook.WebhookEvent.EventType.push, event.event_type);
    try std.testing.expectEqualStrings("chelsea", event.repo_name);
}

test "handleWebhook rejects empty payload" {
    var handler = webhook.WebhookHandler.init(std.testing.allocator);
    const result = handler.handleWebhook("");
    try std.testing.expectError(error.EmptyPayload, result);
}
