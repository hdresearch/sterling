const std = @import("std");
const json = std.json;

/// Configuration for the LLM API client.
pub const LLMConfig = struct {
    api_key: []const u8,
    model: []const u8 = "claude-3-5-sonnet-20241022",
    base_url: []const u8 = "https://api.anthropic.com/v1/messages",
    max_tokens: u32 = 4096,
    temperature: f32 = 0.1,
    max_retries: u32 = 3,
    retry_delay_ms: u64 = 1000,
};

/// Enhancement types supported by the enhancer.
pub const EnhancementType = enum {
    error_handling,
    documentation,
    performance,
    idiomatic,
    type_safety,

    pub fn toString(self: EnhancementType) []const u8 {
        return switch (self) {
            .error_handling => "error_handling",
            .documentation => "documentation",
            .performance => "performance",
            .idiomatic => "idiomatic",
            .type_safety => "type_safety",
        };
    }
};

/// Errors that can occur during LLM operations.
pub const LLMError = error{
    ApiKeyMissing,
    ApiRequestFailed,
    ApiResponseInvalid,
    ApiRateLimited,
    ApiServerError,
    ContentExtractionFailed,
    ProcessSpawnFailed,
    MaxRetriesExceeded,
    EmptyResponse,
    InvalidCodeBlock,
};

/// Result of an LLM API call with metadata.
pub const LLMResult = struct {
    content: []const u8,
    model: []const u8,
    input_tokens: u64,
    output_tokens: u64,
    allocator: std.mem.Allocator,

    pub fn deinit(self: *LLMResult) void {
        self.allocator.free(self.content);
        self.allocator.free(self.model);
    }
};

/// HTTP response structure.
pub const HttpResponse = struct {
    status_code: u16,
    body: []const u8,
    allocator: std.mem.Allocator,

    pub fn deinit(self: *HttpResponse) void {
        self.allocator.free(self.body);
    }
};

pub const LLMEnhancer = struct {
    allocator: std.mem.Allocator,
    config: LLMConfig,

    pub fn init(allocator: std.mem.Allocator, config: LLMConfig) LLMEnhancer {
        return LLMEnhancer{
            .allocator = allocator,
            .config = config,
        };
    }

    pub fn deinit(self: *LLMEnhancer) void {
        _ = self;
    }

    /// Fix compilation errors in generated code.
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

    /// Enhance code with better practices, error handling, and documentation.
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

    /// Generate comprehensive documentation for the SDK.
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

    /// Make HTTP request to LLM API using curl.
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

        // Use curl for HTTP request (more reliable than Zig's HTTP client)
        const curl_cmd = try std.fmt.allocPrint(self.allocator,
            \\curl -s -X POST "{s}" \
            \\  -H "Content-Type: application/json" \
            \\  -H "x-api-key: {s}" \
            \\  -H "anthropic-version: 2023-06-01" \
            \\  -d '{s}'
        , .{ self.config.base_url, self.config.api_key, request_body });
        defer self.allocator.free(curl_cmd);

        var child = std.process.Child.init(&[_][]const u8{ "sh", "-c", curl_cmd }, self.allocator);
        child.stdout_behavior = .Pipe;
        child.stderr_behavior = .Pipe;

        try child.spawn();
        const stdout = try child.stdout.?.readToEndAlloc(self.allocator, 1024 * 1024);
        defer self.allocator.free(stdout);
        
        const stderr = try child.stderr.?.readToEndAlloc(self.allocator, 1024 * 1024);
        defer self.allocator.free(stderr);

        const term = try child.wait();
        if (term != .Exited or term.Exited != 0) {
            std.debug.print("curl error: {s}\n", .{stderr});
            return LLMError.ApiRequestFailed;
        }

        // Parse JSON response
        var parsed = json.parseFromSlice(json.Value, self.allocator, stdout, .{}) catch {
            return LLMError.ApiResponseInvalid;
        };
        defer parsed.deinit();

        const content_array = parsed.value.object.get("content") orelse return LLMError.ContentExtractionFailed;
        if (content_array.array.items.len == 0) return LLMError.EmptyResponse;
        
        const text_content = content_array.array.items[0].object.get("text") orelse return LLMError.ContentExtractionFailed;
        
        return try self.allocator.dupe(u8, text_content.string);
    }
};
