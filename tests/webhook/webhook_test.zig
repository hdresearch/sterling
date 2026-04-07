const std = @import("std");
const webhook = @import("webhook");

test "webhook event type parsing" {
    try std.testing.expect(webhook.WebhookEvent.EventType.push == .push);
    try std.testing.expect(webhook.WebhookEvent.EventType.ping == .ping);
}
