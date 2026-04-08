const std = @import("std");
const http = std.http;
const json = std.json;

pub const GitHubConfig = struct {
    token: []const u8,
    org: []const u8,
    base_url: []const u8 = "https://api.github.com",
};

pub const GitHubAutomation = struct {
    allocator: std.mem.Allocator,
    config: GitHubConfig,
    client: http.Client,

    pub fn init(allocator: std.mem.Allocator, config: GitHubConfig) GitHubAutomation {
        return GitHubAutomation{
            .allocator = allocator,
            .config = config,
            .client = http.Client{ .allocator = allocator },
        };
    }

    pub fn deinit(self: *GitHubAutomation) void {
        self.client.deinit();
    }

    pub fn createRepository(self: *GitHubAutomation, name: []const u8, description: []const u8, is_private: bool) ![]const u8 {
        const request_body = try std.fmt.allocPrint(self.allocator,
            \\{{
            \\  "name": "{s}",
            \\  "description": "{s}",
            \\  "private": {s},
            \\  "auto_init": true,
            \\  "gitignore_template": "Node",
            \\  "license_template": "mit"
            \\}}
        , .{ name, description, if (is_private) "true" else "false" });
        defer self.allocator.free(request_body);

        const url = try std.fmt.allocPrint(self.allocator, "{s}/orgs/{s}/repos", .{ self.config.base_url, self.config.org });
        defer self.allocator.free(url);

        return self.makeRequest("POST", url, request_body);
    }

    pub fn uploadFile(self: *GitHubAutomation, repo: []const u8, path: []const u8, content: []const u8, message: []const u8) ![]const u8 {
        // Base64 encode the content
        const encoder = std.base64.standard.Encoder;
        const encoded_len = encoder.calcSize(content.len);
        const encoded_content = try self.allocator.alloc(u8, encoded_len);
        defer self.allocator.free(encoded_content);
        _ = encoder.encode(encoded_content, content);

        const request_body = try std.fmt.allocPrint(self.allocator,
            \\{{
            \\  "message": "{s}",
            \\  "content": "{s}"
            \\}}
        , .{ message, encoded_content });
        defer self.allocator.free(request_body);

        const url = try std.fmt.allocPrint(self.allocator, "{s}/repos/{s}/{s}/contents/{s}", .{ self.config.base_url, self.config.org, repo, path });
        defer self.allocator.free(url);

        return self.makeRequest("PUT", url, request_body);
    }

    fn makeRequest(self: *GitHubAutomation, method_str: []const u8, url: []const u8, body: []const u8) ![]const u8 {
        var headers = http.Headers{ .allocator = self.allocator };
        defer headers.deinit();

        const auth_header = try std.fmt.allocPrint(self.allocator, "Bearer {s}", .{self.config.token});
        defer self.allocator.free(auth_header);

        try headers.append("Content-Type", "application/json");
        try headers.append("Authorization", auth_header);
        try headers.append("Accept", "application/vnd.github.v3+json");
        try headers.append("User-Agent", "Sterling-SDK-Generator");

        const method = if (std.mem.eql(u8, method_str, "POST")) http.Method.POST else if (std.mem.eql(u8, method_str, "PUT")) http.Method.PUT else http.Method.GET;

        const uri = try std.Uri.parse(url);
        var request = try self.client.open(method, uri, headers, .{});
        defer request.deinit();

        if (body.len > 0) {
            request.transfer_encoding = .{ .content_length = body.len };
            try request.send(.{});
            try request.writeAll(body);
        } else {
            try request.send(.{});
        }
        
        try request.finish();
        try request.wait();

        if (request.response.status.class() != .success) {
            return error.GitHubRequestFailed;
        }

        return try request.reader().readAllAlloc(self.allocator, 1024 * 1024);
    }
};
