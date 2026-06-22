const std = @import("std");

/// A single recorded span: a named phase with a start and elapsed duration.
pub const Span = struct {
    name: []const u8,
    elapsed_ns: u64,
};

/// A lightweight timing tracer. Call `start()` to begin a phase, `stop()` to
/// record it, then `print()` at the end of a run to emit the summary table.
///
/// Usage:
///   var t = Tracer.init(allocator);
///   defer t.deinit();
///
///   t.start("parse spec");
///   // ... work ...
///   try t.stop();
///
///   try t.print();
pub const Tracer = struct {
    allocator: std.mem.Allocator,
    spans: std.array_list.Managed(Span),
    timer: std.time.Timer,
    active_name: ?[]const u8 = null,

    pub fn init(allocator: std.mem.Allocator) Tracer {
        return .{
            .allocator = allocator,
            .spans = std.array_list.Managed(Span).init(allocator),
            .timer = std.time.Timer.start() catch @panic("failed to start timer"),
        };
    }

    pub fn deinit(self: *Tracer) void {
        self.spans.deinit();
    }

    /// Begin timing a named phase. Overwrites any previously active phase name.
    pub fn start(self: *Tracer, name: []const u8) void {
        self.active_name = name;
        self.timer.reset();
    }

    /// Stop the current phase and record its elapsed time.
    pub fn stop(self: *Tracer) !void {
        const elapsed = self.timer.read();
        const name = self.active_name orelse return;
        try self.spans.append(.{ .name = name, .elapsed_ns = elapsed });
        self.active_name = null;
    }

    /// Total elapsed nanoseconds across all recorded spans.
    pub fn totalNs(self: *const Tracer) u64 {
        var total: u64 = 0;
        for (self.spans.items) |s| total += s.elapsed_ns;
        return total;
    }

    /// Print a formatted timing summary table.
    pub fn print(self: *const Tracer) void {
        if (self.spans.items.len == 0) return;

        // Find the longest phase name for column alignment.
        var max_name_len: usize = 5; // "phase"
        for (self.spans.items) |s| {
            if (s.name.len > max_name_len) max_name_len = s.name.len;
        }

        const total = self.totalNs();
        const total_ms = @as(f64, @floatFromInt(total)) / 1_000_000.0;

        std.debug.print("\n⏱  Timing\n", .{});
        std.debug.print("  {s:<40}  {s:>10}  {s:>7}\n", .{ "phase", "duration", "share" });
        std.debug.print("  {s}  {s}  {s}\n", .{ "─" ** 40, "──────────", "───────" });

        for (self.spans.items) |s| {
            const ms = @as(f64, @floatFromInt(s.elapsed_ns)) / 1_000_000.0;
            const pct = if (total > 0)
                @as(f64, @floatFromInt(s.elapsed_ns)) / @as(f64, @floatFromInt(total)) * 100.0
            else
                0.0;
            // Left-pad the name field to max_name_len by printing then spaces.
            std.debug.print("  {s}", .{s.name});
            var pad: usize = 0;
            while (pad + s.name.len < 40) : (pad += 1) std.debug.print(" ", .{});
            std.debug.print("  {d:>9.1}ms  {d:>6.1}%\n", .{ ms, pct });
        }

        std.debug.print("  {s}  {s}  {s}\n", .{ "─" ** 40, "──────────", "───────" });
        std.debug.print("  total", .{});
        var pad: usize = 5;
        while (pad < 40) : (pad += 1) std.debug.print(" ", .{});
        std.debug.print("  {d:>9.1}ms  {d:>6.1}%\n\n", .{ total_ms, 100.0 });
    }
};
