const std = @import("std");
const parser = @import("openapi");
const config = @import("config");
const sdk_gen = @import("sdk");
const llm = @import("llm");
const github = @import("github");
const workflow = @import("workflow");
const docs = @import("docs");

pub fn main() !void {
    var gpa = std.heap.GeneralPurposeAllocator(.{}){};
    defer _ = gpa.deinit();
    const allocator = gpa.allocator();

    const args = try std.process.argsAlloc(allocator);
    defer std.process.argsFree(allocator, args);

    if (args.len < 2) {
        printUsage();
        return;
    }

    const command = args[1];
    
    if (std.mem.eql(u8, command, "generate")) {
        try handleGenerate(allocator, args);
    } else if (std.mem.eql(u8, command, "workflow")) {
        try handleWorkflow(allocator, args);
    } else if (std.mem.eql(u8, command, "init")) {
        try handleInit(allocator);
    } else if (std.mem.eql(u8, command, "version")) {
        printVersion();
    } else if (std.mem.eql(u8, command, "webhook")) {
        try handleWebhook(allocator, args);
    } else {
        std.debug.print("Unknown command: {s}\n", .{command});
        printUsage();
    }
}
