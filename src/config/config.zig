const std = @import("std");

pub const Config = struct {
    targets: []const Target,
    llm: ?LLMConfig = null,

    pub const Target = struct {
        language: Language,
        repository: []const u8,
        output_dir: []const u8,
        branch: []const u8 = "main",

        pub const Language = enum {
            typescript,
            rust,
            python,
            go,
        };
    };

    pub const LLMConfig = struct {
        provider: Provider,
        api_key: []const u8,
        model: []const u8,

        pub const Provider = enum {
            anthropic,
            openai,
        };
    };
};

pub fn loadConfig(allocator: std.mem.Allocator, path: []const u8) !Config {
    _ = allocator;
    _ = path;
    
    // Mock config for now
    const targets = [_]Config.Target{
        Config.Target{
            .language = .typescript,
            .repository = "https://github.com/org/typescript-sdk",
            .output_dir = "./generated/typescript",
        },
    };
    
    return Config{
        .targets = targets[0..],
    };
}
