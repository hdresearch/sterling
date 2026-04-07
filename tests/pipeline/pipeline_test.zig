const std = @import("std");
const pipeline = @import("pipeline");

test "pipeline step status" {
    const step = pipeline.PipelineStep{
        .name = "test-step",
        .status = .pending,
    };
    try std.testing.expectEqualStrings("test-step", step.name);
    try std.testing.expect(step.status == .pending);
}
