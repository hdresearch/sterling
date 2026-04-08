const std = @import("std");
const testing = std.testing;
const enhancer = @import("../../src/llm/enhancer.zig");

test "LLM config initialization" {
    const config = enhancer.LLMConfig{
        .api_key = "test-key",
    };
    
    try testing.expect(std.mem.eql(u8, config.model, "claude-3-5-sonnet-20241022"));
    try testing.expect(config.max_tokens == 4096);
    try testing.expect(config.temperature == 0.1);
}

test "LLM enhancer initialization" {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const config = enhancer.LLMConfig{
        .api_key = "test-key",
    };

    var llm = enhancer.LLMEnhancer.init(allocator, config);
    defer llm.deinit();

    try testing.expect(std.mem.eql(u8, llm.config.api_key, "test-key"));
}

test "Enhancement type enum" {
    try testing.expect(std.mem.eql(u8, enhancer.EnhancementType.error_handling.toString(), "error_handling"));
    try testing.expect(std.mem.eql(u8, enhancer.EnhancementType.documentation.toString(), "documentation"));
    try testing.expect(std.mem.eql(u8, enhancer.EnhancementType.performance.toString(), "performance"));
}
