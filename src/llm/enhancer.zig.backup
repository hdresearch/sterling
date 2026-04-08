const std = @import("std");
const http = std.http;
const json = std.json;

pub const LLMConfig = struct {
    api_key: []const u8,
    model: []const u8 = "claude-3-5-sonnet-20241022",
    base_url: []const u8 = "https://api.anthropic.com/v1/messages",
    max_tokens: u32 = 4000,
    temperature: f32 = 0.1,
};

pub const LLMEnhancer = struct {
    allocator: std.mem.Allocator,
    config: LLMConfig,
    client: http.Client,

    pub fn init(allocator: std.mem.Allocator, config: LLMConfig) LLMEnhancer {
        return LLMEnhancer{
            .allocator = allocator,
            .config = config,
            .client = http.Client{ .allocator = allocator },
        };
    }

    pub fn deinit(self: *LLMEnhancer) void {
        self.client.deinit();
    }

    pub fn fixCompilationError(self: *LLMEnhancer, code: []const u8, error_message: []const u8, language: []const u8) ![]const u8 {
        const prompt = try std.fmt.allocPrint(self.allocator,
            \\Fix this {s} compilation error:
            \\
            \\ERROR: {s}
            \\
            \\CODE:
            \\```{s}
            \\{s}
            \\```
            \\
            \\Return only the corrected code without explanation.
        , .{ language, error_message, language, code });
        defer self.allocator.free(prompt);

        return self.callLLM(prompt);
    }

    pub fn enhanceCode(self: *LLMEnhancer, code: []const u8, language: []const u8, enhancement_type: []const u8) ![]const u8 {
        const prompt = try std.fmt.allocPrint(self.allocator,
            \\Enhance this {s} code for {s}:
            \\
            \\```{s}
            \\{s}
            \\```
            \\
            \\Improvements to make:
            \\- Add comprehensive error handling
            \\- Improve type safety
            \\- Add documentation comments
            \\- Follow language best practices
            \\- Optimize performance where possible
            \\
            \\Return only the enhanced code without explanation.
        , .{ language, enhancement_type, language, code });
        defer self.allocator.free(prompt);

        return self.callLLM(prompt);
    }

    pub fn generateDocumentation(self: *LLMEnhancer, code: []const u8, language: []const u8, api_spec: []const u8) ![]const u8 {
        const prompt = try std.fmt.allocPrint(self.allocator,
            \\Generate comprehensive documentation for this {s} SDK:
            \\
            \\API SPEC:
            \\{s}
            \\
            \\CODE:
            \\```{s}
            \\{s}
            \\```
            \\
            \\Generate:
            \\1. Installation instructions
            \\2. Quick start guide
            \\3. API reference with examples
            \\4. Error handling guide
            \\5. Authentication setup
            \\
            \\Format as Markdown suitable for Mintlify docs.
        , .{ language, api_spec, language, code });
        defer self.allocator.free(prompt);

        return self.callLLM(prompt);
    }

    fn callLLM(self: *LLMEnhancer, prompt: []const u8) ![]const u8 {
        const request_body = try std.fmt.allocPrint(self.allocator,
            \\{{
            \\  "model": "{s}",
            \\  "max_tokens": {d},
            \\  "temperature": {d:.1},
            \\  "messages": [
            \\    {{
            \\      "role": "user",
            \\      "content": "{s}"
            \\    }}
            \\  ]
            \\}}
        , .{ self.config.model, self.config.max_tokens, self.config.temperature, prompt });
        defer self.allocator.free(request_body);

        var headers = http.Headers{ .allocator = self.allocator };
        defer headers.deinit();

        try headers.append("Content-Type", "application/json");
        try headers.append("x-api-key", self.config.api_key);
        try headers.append("anthropic-version", "2023-06-01");

        const uri = try std.Uri.parse(self.config.base_url);
        var request = try self.client.open(.POST, uri, headers, .{});
        defer request.deinit();

        request.transfer_encoding = .{ .content_length = request_body.len };
        try request.send(.{});
        try request.writeAll(request_body);
        try request.finish();
        try request.wait();

        if (request.response.status != .ok) {
            return error.LLMRequestFailed;
        }

        const response_body = try request.reader().readAllAlloc(self.allocator, 1024 * 1024);
        defer self.allocator.free(response_body);

        // Parse JSON response and extract content
        var parsed = try json.parseFromSlice(json.Value, self.allocator, response_body, .{});
        defer parsed.deinit();

        const content = parsed.value.object.get("content").?.array.items[0].object.get("text").?.string;
        return try self.allocator.dupe(u8, content);
    }
};
