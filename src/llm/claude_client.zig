const std = @import("std");
const http = std.http;

pub const ClaudeClient = struct {
    allocator: std.mem.Allocator,
    api_key: []const u8,
    base_url: []const u8,
    
    const Self = @This();
    
    pub fn init(allocator: std.mem.Allocator, api_key: []const u8) Self {
        return Self{
            .allocator = allocator,
            .api_key = api_key,
            .base_url = "https://api.anthropic.com/v1",
        };
    }
    
    pub fn enhanceCode(self: *Self, code: []const u8, language: []const u8, enhancement_type: []const u8) ![]const u8 {
        const prompt = try std.fmt.allocPrint(self.allocator,
            \\Please enhance this {s} code for {s}:
            \\
            \\```{s}
            \\{s}
            \\```
            \\
            \\Provide only the enhanced code without explanations.
        , .{ language, enhancement_type, language, code });
        defer self.allocator.free(prompt);
        
        const request_body = try std.fmt.allocPrint(self.allocator,
            \\{{
            \\  "model": "claude-3-5-sonnet-20241022",
            \\  "max_tokens": 4096,
            \\  "messages": [
            \\    {{
            \\      "role": "user",
            \\      "content": "{s}"
            \\    }}
            \\  ]
            \\}}
        , .{prompt});
        defer self.allocator.free(request_body);
        
        return self.makeRequest(request_body);
    }
    
    fn makeRequest(self: *Self, body: []const u8) ![]const u8 {
        var client = http.Client{ .allocator = self.allocator };
        defer client.deinit();
        
        const uri = try std.Uri.parse(try std.fmt.allocPrint(self.allocator, "{s}/messages", .{self.base_url}));
        defer self.allocator.free(uri.scheme);
        
        var headers = http.Headers{ .allocator = self.allocator };
        defer headers.deinit();
        
        try headers.append("Content-Type", "application/json");
        try headers.append("x-api-key", self.api_key);
        try headers.append("anthropic-version", "2023-06-01");
        
        var request = try client.request(.POST, uri, headers, .{});
        defer request.deinit();
        
        request.transfer_encoding = .chunked;
        try request.start();
        try request.writeAll(body);
        try request.finish();
        try request.wait();
        
        if (request.response.status != .ok) {
            return error.RequestFailed;
        }
        
        const response_body = try request.reader().readAllAlloc(self.allocator, 1024 * 1024);
        
        // Parse JSON response to extract content
        return self.extractContentFromResponse(response_body);
    }
    
    fn extractContentFromResponse(self: *Self, response: []const u8) ![]const u8 {
        // Simple JSON parsing to extract content
        // In a real implementation, use a proper JSON parser
        const content_start = std.mem.indexOf(u8, response, "\"content\":\"") orelse return error.InvalidResponse;
        const content_begin = content_start + 11;
        const content_end = std.mem.indexOf(u8, response[content_begin..], "\"") orelse return error.InvalidResponse;
        
        return try self.allocator.dupe(u8, response[content_begin..content_begin + content_end]);
    }
};
