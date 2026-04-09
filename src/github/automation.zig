const std = @import("std");
const json = std.json;

pub const GitHubConfig = struct {
    token: []const u8,
    org: []const u8,
    base_url: []const u8 = "https://api.github.com",
};

pub const GitHubAutomation = struct {
    allocator: std.mem.Allocator,
    config: GitHubConfig,

    pub fn init(allocator: std.mem.Allocator, config: GitHubConfig) GitHubAutomation {
        return GitHubAutomation{
            .allocator = allocator,
            .config = config,
        };
    }

    pub fn deinit(self: *GitHubAutomation) void {
        _ = self;
    }

    /// Create a new repository on GitHub.
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

    /// Upload a file to a repository.
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

    /// Generate CI/CD workflow for a language.
    pub fn generateWorkflow(self: *GitHubAutomation, language: []const u8) ![]const u8 {
        return switch (std.hash_map.hashString(language)) {
            std.hash_map.hashString("rust") => try self.allocator.dupe(u8,
                \\name: Rust CI
                \\
                \\on:
                \\  push:
                \\    branches: [ main ]
                \\  pull_request:
                \\    branches: [ main ]
                \\
                \\jobs:
                \\  test:
                \\    runs-on: ubuntu-latest
                \\    steps:
                \\    - uses: actions/checkout@v3
                \\    - uses: actions-rs/toolchain@v1
                \\      with:
                \\        toolchain: stable
                \\    - run: cargo test
                \\    - run: cargo clippy
                \\    - run: cargo fmt --check
            ),
            std.hash_map.hashString("typescript") => try self.allocator.dupe(u8,
                \\name: TypeScript CI
                \\
                \\on:
                \\  push:
                \\    branches: [ main ]
                \\  pull_request:
                \\    branches: [ main ]
                \\
                \\jobs:
                \\  test:
                \\    runs-on: ubuntu-latest
                \\    steps:
                \\    - uses: actions/checkout@v3
                \\    - uses: actions/setup-node@v3
                \\      with:
                \\        node-version: '18'
                \\    - run: npm ci
                \\    - run: npm test
                \\    - run: npm run build
            ),
            else => try self.allocator.dupe(u8, "# No workflow template for this language"),
        };
    }

    /// Generate setup instructions for a repository.
    pub fn generateSetupInstructions(self: *GitHubAutomation, repo_name: []const u8, language: []const u8) ![]const u8 {
        return try std.fmt.allocPrint(self.allocator,
            \\# {s} SDK Setup Instructions
            \\
            \\## Prerequisites
            \\
            \\- GitHub account with access to this repository
            \\- {s} development environment
            \\
            \\## Installation
            \\
            \\```bash
            \\git clone https://github.com/{s}/{s}.git
            \\cd {s}
            \\```
            \\
            \\### {s} Setup
            \\
            \\{s}
            \\
            \\## Usage
            \\
            \\See the examples/ directory for usage examples.
            \\
            \\## Contributing
            \\
            \\1. Fork the repository
            \\2. Create a feature branch
            \\3. Make your changes
            \\4. Run tests
            \\5. Submit a pull request
        , .{ 
            repo_name, 
            language, 
            self.config.org, 
            repo_name, 
            repo_name, 
            language,
            self.getLanguageSetupInstructions(language),
        });
    }

    fn getLanguageSetupInstructions(self: *GitHubAutomation, language: []const u8) []const u8 {
        _ = self;
        return switch (std.hash_map.hashString(language)) {
            std.hash_map.hashString("rust") => 
                \\```bash
                \\# Install Rust if not already installed
                \\curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
                \\
                \\# Build and test
                \\cargo build
                \\cargo test
                \\```
            ,
            std.hash_map.hashString("typescript") => 
                \\```bash
                \\# Install Node.js dependencies
                \\npm install
                \\
                \\# Build and test
                \\npm run build
                \\npm test
                \\```
            ,
            std.hash_map.hashString("python") => 
                \\```bash
                \\# Create virtual environment
                \\python -m venv venv
                \\source venv/bin/activate  # On Windows: venv\Scripts\activate
                \\
                \\# Install dependencies
                \\pip install -e .
                \\
                \\# Run tests
                \\pytest
                \\```
            ,
            std.hash_map.hashString("go") => 
                \\```bash
                \\# Install dependencies
                \\go mod download
                \\
                \\# Build and test
                \\go build ./...
                \\go test ./...
                \\```
            ,
            else => "See language-specific documentation for setup instructions.",
        };
    }

    fn makeRequest(self: *GitHubAutomation, method: []const u8, url: []const u8, body: []const u8) ![]const u8 {
        const curl_cmd = try std.fmt.allocPrint(self.allocator,
            \\curl -s -X {s} "{s}" \
            \\  -H "Content-Type: application/json" \
            \\  -H "Authorization: Bearer {s}" \
            \\  -H "Accept: application/vnd.github.v3+json" \
            \\  -H "User-Agent: Sterling-SDK-Generator" \
            \\  -d '{s}'
        , .{ method, url, self.config.token, body });
        defer self.allocator.free(curl_cmd);

        var child = std.process.Child.init(&[_][]const u8{ "sh", "-c", curl_cmd }, self.allocator);
        child.stdout_behavior = .Pipe;
        child.stderr_behavior = .Pipe;

        try child.spawn();
        const stdout = try child.stdout.?.readToEndAlloc(self.allocator, 1024 * 1024);
        const stderr = try child.stderr.?.readToEndAlloc(self.allocator, 1024 * 1024);
        defer self.allocator.free(stderr);

        const term = try child.wait();
        if (term != .Exited or term.Exited != 0) {
            std.debug.print("GitHub API error: {s}\n", .{stderr});
            return error.GitHubRequestFailed;
        }

        return stdout;
    }
};
