const std = @import("std");

pub const LLMConfig = struct {
    api_key: []const u8,
    model: []const u8 = "claude-sonnet-4-20250514",
    base_url: []const u8 = "https://api.anthropic.com/v1/messages",
    max_tokens: u32 = 4096,
};

pub const Enhancer = struct {
    allocator: std.mem.Allocator,
    config: LLMConfig,

    pub fn init(allocator: std.mem.Allocator, cfg: LLMConfig) Enhancer {
        return .{ .allocator = allocator, .config = cfg };
    }

    /// Post-process a generated SDK file through the LLM for polish.
    /// Returns the enhanced content, or the original on any failure.
    pub fn enhance(self: *Enhancer, code: []const u8, language: []const u8, filename: []const u8) []const u8 {
        return self.enhanceInner(code, language, filename) catch |err| {
            std.debug.print("LLM enhancement skipped for {s}: {}\n", .{ filename, err });
            return code;
        };
    }

    fn enhanceInner(self: *Enhancer, code: []const u8, language: []const u8, filename: []const u8) ![]const u8 {
        // Build the prompt — we write it to a temp file to avoid shell escaping issues
        const prompt = try std.fmt.allocPrint(self.allocator,
            \\You are a code quality enhancer. Improve this generated {s} SDK file ({s}).
            \\
            \\Rules:
            \\- Fix any type errors or missing imports
            \\- Add doc comments to public functions/types that lack them
            \\- Improve error handling (use language-idiomatic error types)
            \\- Do NOT change the public API surface (function names, parameter types)
            \\- Do NOT add new dependencies
            \\- Do NOT remove any existing functionality
            \\- Return ONLY the improved code, no explanation
            \\
            \\```{s}
            \\{s}
            \\```
        , .{ language, filename, language, code });
        defer self.allocator.free(prompt);

        // Write prompt to temp file (avoids shell quoting hell)
        const prompt_path = "/tmp/sterling_llm_prompt.txt";
        {
            const f = try std.fs.cwd().createFile(prompt_path, .{});
            defer f.close();
            try f.writeAll(prompt);
        }
        defer std.fs.cwd().deleteFile(prompt_path) catch {};

        // Build JSON request body using jq to handle escaping properly
        const body_cmd = try std.fmt.allocPrint(self.allocator,
            \\jq -n --rawfile prompt {s} \
            \\  --arg model "{s}" \
            \\  --argjson max_tokens {d} \
            \\  '{{model: $model, max_tokens: $max_tokens, messages: [{{role: "user", content: $prompt}}]}}'
        , .{ prompt_path, self.config.model, self.config.max_tokens });
        defer self.allocator.free(body_cmd);

        // Get the JSON body
        var jq_child = std.process.Child.init(&.{ "sh", "-c", body_cmd }, self.allocator);
        jq_child.stdout_behavior = .Pipe;
        jq_child.stderr_behavior = .Pipe;
        try jq_child.spawn();
        const json_body = try jq_child.stdout.?.readToEndAlloc(self.allocator, 512 * 1024);
        const jq_stderr = try jq_child.stderr.?.readToEndAlloc(self.allocator, 16 * 1024);
        defer self.allocator.free(jq_stderr);
        const jq_term = try jq_child.wait();
        if (jq_term.Exited != 0) {
            self.allocator.free(json_body);
            return error.ProcessSpawnFailed;
        }
        defer self.allocator.free(json_body);

        // Write body to file for curl
        const body_path = "/tmp/sterling_llm_body.json";
        {
            const f = try std.fs.cwd().createFile(body_path, .{});
            defer f.close();
            try f.writeAll(json_body);
        }
        defer std.fs.cwd().deleteFile(body_path) catch {};

        // Call the API
        const curl_cmd = try std.fmt.allocPrint(self.allocator,
            \\curl -s -X POST "{s}" \
            \\  -H "Content-Type: application/json" \
            \\  -H "x-api-key: {s}" \
            \\  -H "anthropic-version: 2023-06-01" \
            \\  -d @{s}
        , .{ self.config.base_url, self.config.api_key, body_path });
        defer self.allocator.free(curl_cmd);

        var curl_child = std.process.Child.init(&.{ "sh", "-c", curl_cmd }, self.allocator);
        curl_child.stdout_behavior = .Pipe;
        curl_child.stderr_behavior = .Pipe;
        try curl_child.spawn();
        const stdout = try curl_child.stdout.?.readToEndAlloc(self.allocator, 1024 * 1024);
        const curl_stderr = try curl_child.stderr.?.readToEndAlloc(self.allocator, 16 * 1024);
        defer self.allocator.free(curl_stderr);
        const curl_term = try curl_child.wait();
        if (curl_term.Exited != 0) {
            self.allocator.free(stdout);
            return error.ApiRequestFailed;
        }
        defer self.allocator.free(stdout);

        // Parse response — extract content[0].text
        return self.extractContent(stdout);
    }

    fn extractContent(self: *Enhancer, response: []const u8) ![]const u8 {
        // Use jq to extract the text content — more reliable than hand-parsing
        const input_path = "/tmp/sterling_llm_response.json";
        {
            const f = try std.fs.cwd().createFile(input_path, .{});
            defer f.close();
            try f.writeAll(response);
        }
        defer std.fs.cwd().deleteFile(input_path) catch {};

        const cmd = try std.fmt.allocPrint(self.allocator, "jq -r '.content[0].text // empty' {s}", .{input_path});
        defer self.allocator.free(cmd);

        var child = std.process.Child.init(&.{ "sh", "-c", cmd }, self.allocator);
        child.stdout_behavior = .Pipe;
        child.stderr_behavior = .Pipe;
        try child.spawn();
        const text = try child.stdout.?.readToEndAlloc(self.allocator, 1024 * 1024);
        const jq_err = try child.stderr.?.readToEndAlloc(self.allocator, 16 * 1024);
        defer self.allocator.free(jq_err);
        const term = try child.wait();
        if (term.Exited != 0 or text.len == 0) {
            self.allocator.free(text);
            return error.ContentExtractionFailed;
        }

        // Strip markdown code fences if present
        const trimmed = std.mem.trim(u8, text, " \t\n\r");
        if (std.mem.startsWith(u8, trimmed, "```")) {
            // Find end of first line (```typescript etc)
            const first_nl = std.mem.indexOfScalar(u8, trimmed, '\n') orelse return try self.allocator.dupe(u8, trimmed);
            const rest = trimmed[first_nl + 1 ..];
            // Find closing ```
            if (std.mem.lastIndexOf(u8, rest, "```")) |end| {
                return try self.allocator.dupe(u8, std.mem.trim(u8, rest[0..end], " \t\n\r"));
            }
            return try self.allocator.dupe(u8, rest);
        }

        return try self.allocator.dupe(u8, trimmed);
    }

    const error_set = error{
        ProcessSpawnFailed,
        ApiRequestFailed,
        ContentExtractionFailed,
    };
};
