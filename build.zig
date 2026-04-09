const std = @import("std");

pub fn build(b: *std.Build) void {
    const target = b.standardTargetOptions(.{});
    const optimize = b.standardOptimizeOption(.{});

    const exe = b.addExecutable(.{
        .name = "sterling",
        .target = target,
        .optimize = optimize,
    });
    exe.root_module.addSourceFile(.{ .path = "src/main.zig" });

    b.installArtifact(exe);
}
